use super::super::types::RawEvent;
use super::PacketSource;
use pcap::{Capture, Device, Error};

pub fn list_devices() -> Vec<(String, String)> {
    Device::list()
        .unwrap_or_default()
        .into_iter()
        .map(|d| {
            let desc = d.desc.unwrap_or_else(|| d.name.clone());
            (d.name, desc)
        })
        .collect()
}

pub struct PcapSource {
    cap: Capture<pcap::Active>,
}

impl PcapSource {
    pub fn new(interface: &str) -> Result<Self, String> {
        let mut cap = Capture::from_device(interface)
            .map_err(|e| format!("无法打开网卡 {}: {}", interface, e))?
            .snaplen(65535)
            .promisc(true)
            .timeout(200)
            .open()
            .map_err(|e| format!("无法启动抓包: {} (可能需要管理员/root权限)", e))?;

        cap.filter("tcp or (udp and port 53)", true)
            .map_err(|e| format!("无法设置 BPF 过滤器: {}", e))?;

        Ok(Self { cap })
    }

    pub fn default_interface() -> Result<String, String> {
        Device::list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|d| !d.addresses.is_empty())
            .map(|d| d.name)
            .ok_or_else(|| "未找到可用网络接口".to_string())
    }
}

impl PacketSource for PcapSource {
    fn next(&mut self) -> Result<RawEvent, String> {
        match self.cap.next_packet() {
            Ok(packet) => Ok(RawEvent::Packet {
                ts: std::time::Instant::now(),
                data: packet.data.to_vec(),
            }),
            Err(Error::TimeoutExpired) => Err("timeout".to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn name(&self) -> &'static str {
        "pcap"
    }
}
