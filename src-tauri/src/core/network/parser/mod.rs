pub mod dns;
pub mod tls_sni;

use super::types::{L4Proto, ParsedPacket, TcpFlags};
use etherparse::{NetSlice, SlicedPacket, TransportSlice};

pub fn parse_packet(data: &[u8]) -> Option<ParsedPacket> {
    let sp = SlicedPacket::from_ethernet(data).ok()?;

    let (src_ip, dst_ip) = match sp.net? {
        NetSlice::Ipv4(ip) => {
            let h = ip.header();
            (
                std::net::IpAddr::V4(h.source_addr()),
                std::net::IpAddr::V4(h.destination_addr()),
            )
        }
        NetSlice::Ipv6(ip) => {
            let h = ip.header();
            (
                std::net::IpAddr::V6(h.source_addr()),
                std::net::IpAddr::V6(h.destination_addr()),
            )
        }
    };

    let transport = sp.transport?;
    match transport {
        TransportSlice::Tcp(tcp) => {
            let l4_payload = tcp.payload().to_vec();
            Some(ParsedPacket {
                src_ip,
                dst_ip,
                src_port: tcp.source_port(),
                dst_port: tcp.destination_port(),
                proto: L4Proto::Tcp,
                tcp_flags: Some(TcpFlags {
                    syn: tcp.syn(),
                    fin: tcp.fin(),
                    rst: tcp.rst(),
                    ack: tcp.ack(),
                }),
                payload_offset: 0,
                payload_len: l4_payload.len(),
                total_len: data.len(),
                l4_payload,
            })
        }
        TransportSlice::Udp(udp) => {
            let l4_payload = udp.payload().to_vec();
            Some(ParsedPacket {
                src_ip,
                dst_ip,
                src_port: udp.source_port(),
                dst_port: udp.destination_port(),
                proto: L4Proto::Udp,
                tcp_flags: None,
                payload_offset: 0,
                payload_len: l4_payload.len(),
                total_len: data.len(),
                l4_payload,
            })
        }
        _ => None,
    }
}
