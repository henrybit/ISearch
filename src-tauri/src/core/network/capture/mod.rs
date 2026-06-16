pub mod pcap;

use super::types::RawEvent;
use std::time::Instant;

pub trait PacketSource: Send {
    fn next(&mut self) -> Result<RawEvent, String>;
    fn name(&self) -> &'static str;
}

pub fn list_interfaces() -> Vec<(String, String)> {
    pcap::list_devices()
}

pub fn create_source(interface: &str) -> Result<Box<dyn PacketSource>, String> {
    Ok(Box::new(pcap::PcapSource::new(interface)?))
}
