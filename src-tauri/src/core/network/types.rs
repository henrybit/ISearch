use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum L4Proto {
    Tcp,
    Udp,
}

impl L4Proto {
    pub fn as_str(&self) -> &'static str {
        match self {
            L4Proto::Tcp => "TCP",
            L4Proto::Udp => "UDP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiveTuple {
    pub proto: L4Proto,
    pub src_ip: IpAddr,
    pub src_port: u16,
    pub dst_ip: IpAddr,
    pub dst_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    SynSent,
    Established,
    Closing,
    Closed,
}

impl ConnState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnState::SynSent => "syn_sent",
            ConnState::Established => "established",
            ConnState::Closing => "closing",
            ConnState::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub key: FiveTuple,
    pub state: ConnState,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub domain: Option<String>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ParsedPacket {
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: L4Proto,
    pub tcp_flags: Option<TcpFlags>,
    pub payload_offset: usize,
    pub payload_len: usize,
    pub total_len: usize,
    pub l4_payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct TcpFlags {
    pub syn: bool,
    pub fin: bool,
    pub rst: bool,
    pub ack: bool,
}

#[derive(Debug, Clone)]
pub enum RawEvent {
    Packet {
        ts: Instant,
        data: Vec<u8>,
    },
    TcpConnect {
        ts: Instant,
        src: SocketAddr,
        dst: SocketAddr,
        pid: Option<u32>,
    },
    TcpClose {
        ts: Instant,
        src: SocketAddr,
        dst: SocketAddr,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionView {
    pub id: String,
    pub protocol: String,
    pub src_ip: String,
    pub src_port: u16,
    pub dst_ip: String,
    pub dst_port: u16,
    pub domain: Option<String>,
    pub state: String,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopEntry {
    pub key: String,
    pub bytes: u64,
    pub connections: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowStatsView {
    pub bytes_total: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub new_connections: u64,
    pub closed_connections: u64,
    pub active_connections: u64,
    pub bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTraceSnapshot {
    pub is_tracing: bool,
    pub interface: String,
    pub connections: Vec<ConnectionView>,
    pub stats: WindowStatsView,
    pub top_domains: Vec<TopEntry>,
    pub top_ips: Vec<TopEntry>,
    pub packets_captured: u64,
    pub error: Option<String>,
}
