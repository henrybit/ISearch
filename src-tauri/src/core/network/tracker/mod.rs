use super::parser::{dns, tls_sni};
use super::types::{ConnState, Connection, FiveTuple, L4Proto, ParsedPacket, TopEntry};
use ahash::AHashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

pub struct ConnectionTracker {
    by_tuple: AHashMap<FiveTuple, Connection>,
    ip_to_domain: AHashMap<IpAddr, String>,
    stale_after: Duration,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            by_tuple: AHashMap::new(),
            ip_to_domain: AHashMap::new(),
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
        let domain = self.resolve_domain(dst.ip());

        if let Some(conn) = self.by_tuple.get_mut(&key) {
            conn.state = ConnState::Established;
            conn.last_seen = now;
            if pid.is_some() {
                conn.pid = pid;
            }
            if conn.domain.is_none() {
                conn.domain = domain;
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
        let resolved_domain = self.resolve_domain(pkt.dst_ip);

        if let Some(conn) = self.by_tuple.get_mut(&key) {
            conn.bytes_up += pkt_len;
            conn.last_seen = now;
            if conn.domain.is_none() {
                conn.domain = resolved_domain.clone();
            }
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
            self.by_tuple.insert(
                key,
                Connection {
                    key,
                    state,
                    first_seen: now,
                    last_seen: now,
                    bytes_up: pkt_len,
                    bytes_down: 0,
                    domain: resolved_domain,
                    pid: None,
                },
            );
        }

        if pkt.proto == L4Proto::Tcp {
            let looks_like_tls =
                payload.len() >= 5 && payload[0] == 0x16 && payload[1] == 0x03;
            let tls_port = matches!(pkt.dst_port, 443 | 853 | 8443 | 9443 | 4443);
            if tls_port || looks_like_tls {
                if let Some(sni) = tls_sni::extract_sni(payload) {
                    self.associate_domain(pkt.dst_ip, pkt.dst_port, sni);
                }
            }
        }

        if pkt.proto == L4Proto::Udp && (pkt.src_port == 53 || pkt.dst_port == 53) {
            if let Some(info) = dns::parse_dns(payload) {
                let query_name = info.queries.first().cloned();
                for (answer_name, ip) in info.answers {
                    let domain = query_name.clone().unwrap_or(answer_name);
                    self.associate_domain(ip, 0, domain);
                }
            }
        }
    }

    fn associate_domain(&mut self, ip: IpAddr, port: u16, domain: String) {
        if domain.is_empty() {
            return;
        }
        self.ip_to_domain.insert(ip, domain.clone());

        for conn in self.by_tuple.values_mut() {
            if conn.key.dst_ip == ip && (port == 0 || conn.key.dst_port == port) {
                conn.domain = Some(domain.clone());
            }
        }
    }

    pub fn resolve_domain(&self, ip: IpAddr) -> Option<String> {
        self.ip_to_domain.get(&ip).cloned()
    }

    pub fn gc(&mut self) {
        let cutoff = Instant::now() - self.stale_after;
        self.by_tuple
            .retain(|_, c| c.last_seen >= cutoff && c.state != ConnState::Closed);
    }

    pub fn snapshot(&self) -> Vec<Connection> {
        self.by_tuple.values().cloned().collect()
    }

    pub fn top_domains(&self, top_n: usize) -> Vec<TopEntry> {
        let mut bytes_by_domain: AHashMap<String, (u64, u64)> = AHashMap::new();
        for conn in self.by_tuple.values() {
            if let Some(domain) = &conn.domain {
                if domain.is_empty() {
                    continue;
                }
                let entry = bytes_by_domain.entry(domain.clone()).or_insert((0, 0));
                entry.0 += conn.bytes_up + conn.bytes_down;
                entry.1 += 1;
            }
        }
        let mut entries: Vec<TopEntry> = bytes_by_domain
            .into_iter()
            .map(|(key, (bytes, connections))| TopEntry {
                key,
                bytes,
                connections,
            })
            .collect();
        entries.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        entries.truncate(top_n);
        entries
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
