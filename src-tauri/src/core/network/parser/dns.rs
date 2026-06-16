use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct DnsInfo {
    pub queries: Vec<String>,
    pub answers: Vec<(String, IpAddr)>,
}

fn read_name(data: &[u8], mut offset: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut jump_offset = 0usize;
    let mut steps = 0usize;

    loop {
        if offset >= data.len() || steps > 128 {
            return None;
        }
        steps += 1;
        let len = data[offset];
        if len == 0 {
            offset += 1;
            break;
        }
        if len & 0xC0 == 0xC0 {
            if offset + 1 >= data.len() {
                return None;
            }
            let ptr = (((len & 0x3F) as usize) << 8) | data[offset + 1] as usize;
            if !jumped {
                jump_offset = offset + 2;
            }
            offset = ptr;
            jumped = true;
            continue;
        }
        offset += 1;
        if offset + len as usize > data.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&data[offset..offset + len as usize]).to_string());
        offset += len as usize;
    }

    let next = if jumped { jump_offset } else { offset };
    Some((labels.join("."), next))
}

fn parse_rdata(data: &[u8], rtype: u16, rdlength: usize) -> Option<IpAddr> {
    if data.len() < rdlength {
        return None;
    }
    match rtype {
        1 if rdlength == 4 => Some(IpAddr::from([
            data[0], data[1], data[2], data[3],
        ])),
        28 if rdlength == 16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&data[..16]);
            Some(IpAddr::from(octets))
        }
        _ => None,
    }
}

/// Parse DNS query/response from UDP payload.
pub fn parse_dns(payload: &[u8]) -> Option<DnsInfo> {
    if payload.len() < 12 {
        return None;
    }

    let qdcount = u16::from_be_bytes([payload[4], payload[5]]) as usize;
    let ancount = u16::from_be_bytes([payload[6], payload[7]]) as usize;

    let mut offset = 12usize;
    let mut queries = Vec::new();

    for _ in 0..qdcount {
        let (name, next) = read_name(payload, offset)?;
        queries.push(name);
        offset = next + 4;
        if offset > payload.len() {
            return None;
        }
    }

    let mut answers = Vec::new();
    for _ in 0..ancount {
        let (name, next) = read_name(payload, offset)?;
        offset = next;
        if offset + 10 > payload.len() {
            break;
        }
        let rtype = u16::from_be_bytes([payload[offset], payload[offset + 1]]);
        let rdlength = u16::from_be_bytes([payload[offset + 8], payload[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlength > payload.len() {
            break;
        }
        if let Some(ip) = parse_rdata(&payload[offset..offset + rdlength], rtype, rdlength) {
            answers.push((name, ip));
        }
        offset += rdlength;
    }

    Some(DnsInfo { queries, answers })
}
