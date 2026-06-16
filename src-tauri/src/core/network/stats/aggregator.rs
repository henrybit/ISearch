use super::super::types::{TopEntry, WindowStatsView};
use ahash::AHashMap;
use std::net::IpAddr;
use std::time::Instant;

pub struct StatsAggregator {
    window_start: Instant,
    bytes_total: u64,
    bytes_up: u64,
    bytes_down: u64,
    bytes_per_domain: AHashMap<String, u64>,
    bytes_per_remote_ip: AHashMap<IpAddr, u64>,
    new_connections: u64,
    closed_connections: u64,
    top_n: usize,
    prev_bytes_total: u64,
    prev_tick: Instant,
}

impl StatsAggregator {
    pub fn new(_window_secs: u64, top_n: usize) -> Self {
        let now = Instant::now();
        Self {
            window_start: now,
            bytes_total: 0,
            bytes_up: 0,
            bytes_down: 0,
            bytes_per_domain: AHashMap::new(),
            bytes_per_remote_ip: AHashMap::new(),
            new_connections: 0,
            closed_connections: 0,
            top_n,
            prev_bytes_total: 0,
            prev_tick: now,
        }
    }

    pub fn record_packet(
        &mut self,
        bytes_up: u64,
        bytes_down: u64,
        domain: Option<&str>,
        remote_ip: IpAddr,
    ) {
        let total = bytes_up + bytes_down;
        self.bytes_total += total;
        self.bytes_up += bytes_up;
        self.bytes_down += bytes_down;
        if let Some(d) = domain {
            *self.bytes_per_domain.entry(d.to_string()).or_insert(0) += total;
        }
        *self.bytes_per_remote_ip.entry(remote_ip).or_insert(0) += total;
    }

    pub fn record_connection_new(&mut self) {
        self.new_connections += 1;
    }

    pub fn record_connection_closed(&mut self) {
        self.closed_connections += 1;
    }

    pub fn view(&mut self, active_count: u64) -> WindowStatsView {
        let now = Instant::now();
        let elapsed = now.duration_since(self.prev_tick).as_secs_f64().max(0.001);
        let bytes_delta = self.bytes_total.saturating_sub(self.prev_bytes_total);
        let bytes_per_sec = bytes_delta as f64 / elapsed;
        self.prev_bytes_total = self.bytes_total;
        self.prev_tick = now;

        WindowStatsView {
            bytes_total: self.bytes_total,
            bytes_up: self.bytes_up,
            bytes_down: self.bytes_down,
            new_connections: self.new_connections,
            closed_connections: self.closed_connections,
            active_connections: active_count,
            bytes_per_sec,
        }
    }

    pub fn top_domains(&self) -> Vec<TopEntry> {
        let mut entries: Vec<TopEntry> = self
            .bytes_per_domain
            .iter()
            .map(|(k, v)| TopEntry {
                key: k.clone(),
                bytes: *v,
                connections: 1,
            })
            .collect();
        entries.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        entries.truncate(self.top_n);
        entries
    }

    pub fn top_ips(&self) -> Vec<TopEntry> {
        let mut entries: Vec<TopEntry> = self
            .bytes_per_remote_ip
            .iter()
            .map(|(k, v)| TopEntry {
                key: k.to_string(),
                bytes: *v,
                connections: 1,
            })
            .collect();
        entries.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        entries.truncate(self.top_n);
        entries
    }

    pub fn reset(&mut self) {
        let now = Instant::now();
        self.window_start = now;
        self.bytes_total = 0;
        self.bytes_up = 0;
        self.bytes_down = 0;
        self.bytes_per_domain.clear();
        self.bytes_per_remote_ip.clear();
        self.new_connections = 0;
        self.closed_connections = 0;
        self.prev_bytes_total = 0;
        self.prev_tick = now;
    }
}
