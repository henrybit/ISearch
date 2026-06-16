fn skip_u8_len_prefixed(data: &[u8]) -> Option<&[u8]> {
    if data.is_empty() {
        return None;
    }
    let len = data[0] as usize;
    if data.len() < 1 + len {
        return None;
    }
    Some(&data[1 + len..])
}

fn skip_u16_len_prefixed(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 2 {
        return None;
    }
    let len = u16::from_be_bytes([data[0], data[1]]) as usize;
    if data.len() < 2 + len {
        return None;
    }
    Some(&data[2 + len..])
}

fn parse_sni_extension(data: &[u8]) -> Option<String> {
    if data.len() < 5 {
        return None;
    }
    // server_name_list length (2 bytes) + entry type (1) + name length (2)
    if data[2] != 0x00 {
        return None;
    }
    let name_len = u16::from_be_bytes([data[3], data[4]]) as usize;
    if data.len() < 5 + name_len {
        return None;
    }
    std::str::from_utf8(&data[5..5 + name_len])
        .ok()
        .map(String::from)
}

/// Extract TLS SNI hostname from a TCP payload (ClientHello).
pub fn extract_sni(tcp_payload: &[u8]) -> Option<String> {
    let mut p = tcp_payload;
    if p.len() < 5 || p[0] != 0x16 || p[1] != 0x03 {
        return None;
    }
    p = &p[5..];
    if p.is_empty() || p[0] != 0x01 {
        return None;
    }
    if p.len() < 4 {
        return None;
    }
    p = &p[4..];
    if p.len() < 34 {
        return None;
    }
    p = &p[34..];
    p = skip_u8_len_prefixed(p)?;
    p = skip_u16_len_prefixed(p)?;
    p = skip_u8_len_prefixed(p)?;

    if p.len() < 2 {
        return None;
    }
    let ext_total = u16::from_be_bytes([p[0], p[1]]) as usize;
    p = &p[2..];
    let ext_end = ext_total.min(p.len());
    let mut consumed = 0usize;

    while p.len() >= 4 && consumed < ext_end {
        let ext_type = u16::from_be_bytes([p[0], p[1]]);
        let ext_len = u16::from_be_bytes([p[2], p[3]]) as usize;
        p = &p[4..];
        consumed += 4;
        if ext_type == 0x0000 {
            return parse_sni_extension(&p[..ext_len.min(p.len())]);
        }
        if p.len() < ext_len {
            break;
        }
        p = &p[ext_len..];
        consumed += ext_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_tls() {
        assert!(extract_sni(&[0x00, 0x01, 0x02]).is_none());
    }
}
