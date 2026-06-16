pub mod capture;
pub mod parser;
pub mod stats;
pub mod tracker;
pub mod types;

use capture::create_source;
use stats::StatsAggregator;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracker::ConnectionTracker;
use types::{ConnectionView, NetworkInterface, NetworkTraceSnapshot};

pub struct NetworkMonitor {
    is_tracing: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    interface: Arc<Mutex<String>>,
    error: Arc<Mutex<Option<String>>>,
    packets_captured: Arc<Mutex<u64>>,
    tracker: Arc<Mutex<ConnectionTracker>>,
    stats: Arc<Mutex<StatsAggregator>>,
    capture_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl NetworkMonitor {
    pub fn new() -> Self {
        Self {
            is_tracing: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            interface: Arc::new(Mutex::new(String::new())),
            error: Arc::new(Mutex::new(None)),
            packets_captured: Arc::new(Mutex::new(0)),
            tracker: Arc::new(Mutex::new(ConnectionTracker::new())),
            stats: Arc::new(Mutex::new(StatsAggregator::new(60, 20))),
            capture_thread: Arc::new(Mutex::new(None)),
        }
    }

    pub fn list_interfaces() -> Vec<NetworkInterface> {
        capture::list_interfaces()
            .into_iter()
            .map(|(name, description)| NetworkInterface { name, description })
            .collect()
    }

    pub fn is_tracing(&self) -> bool {
        self.is_tracing.load(Ordering::Relaxed)
    }

    pub fn start(&self, interface: Option<String>) -> Result<(), String> {
        if self.is_tracing.load(Ordering::Relaxed) {
            return Err("网络追踪已在运行中".to_string());
        }

        let iface = match interface {
            Some(i) if !i.is_empty() => i,
            _ => capture::pcap::PcapSource::default_interface()?,
        };

        {
            let mut stored = self.interface.lock().map_err(|e| e.to_string())?;
            *stored = iface.clone();
        }
        {
            let mut err = self.error.lock().map_err(|e| e.to_string())?;
            *err = None;
        }
        {
            let mut count = self.packets_captured.lock().map_err(|e| e.to_string())?;
            *count = 0;
        }
        {
            let mut tracker = self.tracker.lock().map_err(|e| e.to_string())?;
            *tracker = ConnectionTracker::new();
        }
        {
            let mut stats = self.stats.lock().map_err(|e| e.to_string())?;
            stats.reset();
        }

        self.stop_flag.store(false, Ordering::Relaxed);
        self.is_tracing.store(true, Ordering::Relaxed);

        let stop_flag = Arc::clone(&self.stop_flag);
        let is_tracing = Arc::clone(&self.is_tracing);
        let error = Arc::clone(&self.error);
        let packets_captured = Arc::clone(&self.packets_captured);
        let tracker = Arc::clone(&self.tracker);
        let stats = Arc::clone(&self.stats);

        let handle = thread::spawn(move || {
            let mut source = match create_source(&iface) {
                Ok(s) => s,
                Err(e) => {
                    if let Ok(mut err) = error.lock() {
                        *err = Some(e.clone());
                    }
                    is_tracing.store(false, Ordering::Relaxed);
                    return;
                }
            };

            let mut last_gc = Instant::now();

            while !stop_flag.load(Ordering::Relaxed) {
                match source.next() {
                    Ok(event) => {
                        if let Ok(mut count) = packets_captured.lock() {
                            *count += 1;
                        }

                        match &event {
                            types::RawEvent::Packet { data, .. } => {
                                if let Some(pkt) = parser::parse_packet(data) {
                                    if let Ok(mut t) = tracker.lock() {
                                        t.on_packet(&pkt, &pkt.l4_payload);
                                    }
                                    if let Ok(mut s) = stats.lock() {
                                        s.record_packet(
                                            pkt.total_len as u64,
                                            0,
                                            None,
                                            pkt.dst_ip,
                                        );
                                    }
                                }
                            }
                            types::RawEvent::TcpConnect { src, dst, pid, .. } => {
                                if let Ok(mut t) = tracker.lock() {
                                    t.on_tcp_connect(*src, *dst, *pid);
                                    if let Ok(mut s) = stats.lock() {
                                        s.record_connection_new();
                                    }
                                }
                            }
                            types::RawEvent::TcpClose { src, dst, .. } => {
                                if let Ok(mut t) = tracker.lock() {
                                    t.on_tcp_close(*src, *dst);
                                    if let Ok(mut s) = stats.lock() {
                                        s.record_connection_closed();
                                    }
                                }
                            }
                        }
                    }
                    Err(e) if e == "timeout" => {}
                    Err(e) => {
                        if !stop_flag.load(Ordering::Relaxed) {
                            log::warn!("[network_trace] capture error: {}", e);
                        }
                    }
                }

                if last_gc.elapsed() >= Duration::from_secs(30) {
                    if let Ok(mut t) = tracker.lock() {
                        t.gc();
                    }
                    last_gc = Instant::now();
                }
            }

            is_tracing.store(false, Ordering::Relaxed);
        });

        {
            let mut thread_handle = self.capture_thread.lock().map_err(|e| e.to_string())?;
            *thread_handle = Some(handle);
        }

        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        if !self.is_tracing.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Ok(mut handle) = self.capture_thread.lock() {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }

        self.is_tracing.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<NetworkTraceSnapshot, String> {
        let interface = self
            .interface
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        let error = self.error.lock().map_err(|e| e.to_string())?.clone();
        let packets_captured = *self.packets_captured.lock().map_err(|e| e.to_string())?;

        let connections_raw = {
            let tracker = self.tracker.lock().map_err(|e| e.to_string())?;
            tracker.snapshot()
        };

        let active_count = connections_raw
            .iter()
            .filter(|c| c.state != types::ConnState::Closed)
            .count() as u64;

        let stats = {
            let mut stats = self.stats.lock().map_err(|e| e.to_string())?;
            stats.view(active_count)
        };

        let top_domains = {
            let stats = self.stats.lock().map_err(|e| e.to_string())?;
            stats.top_domains()
        };

        let top_ips = {
            let stats = self.stats.lock().map_err(|e| e.to_string())?;
            stats.top_ips()
        };

        let connections: Vec<ConnectionView> = connections_raw
            .into_iter()
            .map(|c| connection_to_view(c))
            .collect();

        Ok(NetworkTraceSnapshot {
            is_tracing: self.is_tracing.load(Ordering::Relaxed),
            interface,
            connections,
            stats,
            top_domains,
            top_ips,
            packets_captured,
            error,
        })
    }
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn connection_to_view(conn: types::Connection) -> ConnectionView {
    let id = format!(
        "{}-{}-{}-{}-{}",
        conn.key.proto.as_str(),
        conn.key.src_ip,
        conn.key.src_port,
        conn.key.dst_ip,
        conn.key.dst_port
    );
    ConnectionView {
        id,
        protocol: conn.key.proto.as_str().to_string(),
        src_ip: conn.key.src_ip.to_string(),
        src_port: conn.key.src_port,
        dst_ip: conn.key.dst_ip.to_string(),
        dst_port: conn.key.dst_port,
        domain: conn.domain,
        state: conn.state.as_str().to_string(),
        bytes_up: conn.bytes_up,
        bytes_down: conn.bytes_down,
        first_seen: format_instant(conn.first_seen),
        last_seen: format_instant(conn.last_seen),
    }
}

fn format_instant(_instant: Instant) -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
