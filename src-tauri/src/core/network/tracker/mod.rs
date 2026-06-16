use super::parser::{dns, tls_sni};
use super::types::{ConnState, Connection, FiveTuple, L4Proto, ParsedPacket};
use ahash::AHashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

pub struct ConnectionTracker {
    by_tuple: AHashMap<FiveTuple, Connection>,
    dns_to_ip: AHashMap<String, Vec<(IpAddr, Instant)>>,
    stale_after: Duration,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            by_tuple: AHashMap::new(),
            dns_to_ip: AHashMap::new(),
            stale_after: Duration::from_secs(300),
        }
    }

    pub fn on_tcp_connect(&mut self, src: SocketAddr, dst: SocketAddr, pid: Option<u32>) {
        let key = FiveTuple {
            proto: L4Proto::Tcp,
            src_ip: src.ip(),
            src_port: src.port(),
            dst_ip: dst.ip(),
            dst_port: dst.port(),
        };
        let now = Instant::now();
        let domain = self.lookup_domain(dst.ip());

        if let Some(conn) = self.by_tuple.get_mut(&key) {
            conn.state = ConnState::Established;
            conn.last_seen = now;
            if pid.is_some() {
                conn.pid = pid;
            }
        } else {
            self.by_tuple.insert(
                key,
                Connection {
                    key,
                    state: ConnState::Established,
                    first_seen: now,
                    last_seen: now,
                    bytes_up: 0,
                    bytes_down: 0,
                    domain,
                    pid,
                },
            );
        }
    }

    pub fn on_tcp_close(&mut self, src: SocketAddr, dst: SocketAddr) {
        let key = FiveTuple {
            proto: L4Proto::Tcp,
            src_ip: src.ip(),
            src_port: src.port(),
            dst_ip: dst.ip(),
            dst_port: dst.port(),
        };
        if let Some(conn) = self.by_tuple.get_mut(&key) {
            conn.state = ConnState::Closed;
            conn.last_seen = Instant::now();
        }
    }

    pub fn on_packet(&mut self, pkt: &ParsedPacket, payload: &[u8]) {
        let now = Instant::now();
        let key = FiveTuple {
            proto: pkt.proto,
            src_ip: pkt.src_ip,
            src_port: pkt.src_port,
            dst_ip: pkt.dst_ip,
            dst_port: pkt.dst_port,
        };

        let reverse_key = FiveTuple {
            proto: pkt.proto,
            src_ip: pkt.dst_ip,
            src_port: pkt.dst_port,
            dst_ip: pkt.src_ip,
            dst_port: pkt.src_port,
        };

        let pkt_len = pkt.total_len as u64;

        if let Some(conn) = self.by_tuple.get_mut(&key) {
            conn.bytes_up += pkt_len;
            conn.last_seen = now;
            apply_tcp_state(conn, pkt);
        } else if let Some(conn) = self.by_tuple.get_mut(&reverse_key) {
            conn.bytes_down += pkt_len;
            conn.last_seen = now;
        } else {
            let mut state = ConnState::Established;
            if let Some(flags) = pkt.tcp_flags {
                if flags.syn && !flags.ack {
                    state = ConnState::SynSent;
                }
            }
            let domain = self.lookup_domain(pkt.dst_ip);
            self.by_tuple.insert(
                key,
                Connection {
                    key,
                    state,
                    first_seen: now,
                    last_seen: now,
                    bytes_up: pkt_len,
                    bytes_down: 0,
                    domain,
                    pid: None,
                },
            );
        }

        if pkt.proto == L4Proto::Tcp && (pkt.dst_port == 443 || pkt.dst_port == 8443) {
            if let Some(sni) = tls_sni::extract_sni(payload) {
                self.on_sni(pkt.dst_ip, pkt.dst_port, sni);
            }
        }

        if pkt.proto == L4Proto::Udp && (pkt.src_port == 53 || pkt.dst_port == 53) {
            if let Some(info) = dns::parse_dns(payload) {
                for (name, ip) in info.answers {
                    self.on_dns_response(&name, &[ip]);
                }
                for query in info.queries {
                    self.dns_to_ip
                        .entry(query)
                        .or_default()
                        .push((pkt.dst_ip, now));
                }
            }
        }
    }

    pub fn on_dns_response(&mut self, queried: &str, resolved: &[IpAddr]) {
        let now = Instant::now();
        for ip in resolved {
            self.dns_to_ip
                .entry(ip.to_string())
                .or_default()
                .push((*ip, now));

            for conn in self.by_tuple.values_mut() {
                if conn.key.dst_ip == *ip && conn.domain.is_none() {
                    conn.domain = Some(queried.to_string());
                }
            }
        }
    }

    pub fn on_sni(&mut self, dst_ip: IpAddr, dst_port: u16, sni: String) {
        for conn in self.by_tuple.values_mut() {
            if conn.key.dst_ip == dst_ip && conn.key.dst_port == dst_port {
                conn.domain = Some(sni.clone());
            }
        }
        self.dns_to_ip
            .entry(sni)
            .or_default()
            .push((dst_ip, Instant::now()));
    }

    fn lookup_domain(&self, ip: IpAddr) -> Option<String> {
        self.by_tuple
            .values()
            .find(|c| c.key.dst_ip == ip)
            .and_then(|c| c.domain.clone())
    }

    pub fn gc(&mut self) {
        let cutoff = Instant::now() - self.stale_after;
        self.by_tuple
            .retain(|_, c| c.last_seen >= cutoff && c.state != ConnState::Closed);
    }

    pub fn snapshot(&self) -> Vec<Connection> {
        self.by_tuple.values().cloned().collect()
    }

    pub fn active_count(&self) -> u64 {
        self.by_tuple
            .values()
            .filter(|c| c.state != ConnState::Closed)
            .count() as u64
    }
}

fn apply_tcp_state(conn: &mut Connection, pkt: &ParsedPacket) {
    if let Some(flags) = pkt.tcp_flags {
        if flags.fin || flags.rst {
            conn.state = ConnState::Closing;
        } else if flags.syn && flags.ack {
            conn.state = ConnState::Established;
        }
    }
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}
