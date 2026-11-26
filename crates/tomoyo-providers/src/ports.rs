//! Zero-subprocess native TCP listening port scanner (Win32 `GetExtendedTcpTable` / Linux `/proc/net/tcp*`).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListeningPort {
    pub port: u16,
    pub proto: &'static str,
    pub local_addr: String,
    pub pid: u32,
}

/// Lists active IPv4 & IPv6 TCP listening sockets sorted by `(port, pid)`.
pub fn list_listening_tcp_ports() -> Vec<ListeningPort> {
    let mut out = platform_tcp_ports();
    dedup_and_sort(&mut out);
    out
}

pub(crate) fn dedup_and_sort(list: &mut Vec<ListeningPort>) {
    list.sort_by(|a, b| {
        a.port
            .cmp(&b.port)
            .then_with(|| a.pid.cmp(&b.pid))
            .then_with(|| a.proto.cmp(b.proto))
            .then_with(|| a.local_addr.cmp(&b.local_addr))
    });
    list.dedup_by(|a, b| a.port == b.port && a.pid == b.pid && a.proto == b.proto && a.local_addr == b.local_addr);
}

#[cfg(windows)]
fn platform_tcp_ports() -> Vec<ListeningPort> {
    use std::ffi::c_void;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_LISTEN, TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    fn query_table(af: u32) -> Option<Vec<u8>> {
        let mut size = 0u32;
        let err = unsafe {
            GetExtendedTcpTable(None, &mut size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0)
        };
        if err != ERROR_INSUFFICIENT_BUFFER.0 && size == 0 {
            return None;
        }
        let mut buf = vec![0u8; (size as usize).max(64)];
        let rc = unsafe {
            GetExtendedTcpTable(
                Some(buf.as_mut_ptr() as *mut c_void),
                &mut size,
                false,
                af,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc == 0 {
            buf.truncate(size as usize);
            Some(buf)
        } else {
            None
        }
    }

    let mut out = Vec::with_capacity(64);
    if let Some(buf) = query_table(AF_INET.0 as u32)
        && buf.len() >= std::mem::size_of::<u32>()
    {
        let table = unsafe { &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
        let count = table.dwNumEntries as usize;
        let hdr = std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table);
        let row_sz = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        if hdr + count.saturating_mul(row_sz) <= buf.len() {
            let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), count) };
            for r in rows {
                if r.dwState != MIB_TCP_STATE_LISTEN.0 as u32 {
                    continue;
                }
                let port = u16::from_be((r.dwLocalPort & 0xFFFF) as u16);
                let addr = Ipv4Addr::from(u32::from_be(r.dwLocalAddr)).to_string();
                out.push(ListeningPort { port, proto: "TCP4", local_addr: addr, pid: r.dwOwningPid });
            }
        }
    }

    if let Some(buf) = query_table(AF_INET6.0 as u32)
        && buf.len() >= std::mem::size_of::<u32>()
    {
        let table = unsafe { &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID) };
        let count = table.dwNumEntries as usize;
        let hdr = std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table);
        let row_sz = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
        if hdr + count.saturating_mul(row_sz) <= buf.len() {
            let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), count) };
            for r in rows {
                if r.dwState != MIB_TCP_STATE_LISTEN.0 as u32 {
                    continue;
                }
                let port = u16::from_be((r.dwLocalPort & 0xFFFF) as u16);
                let addr = Ipv6Addr::from(r.ucLocalAddr).to_string();
                out.push(ListeningPort { port, proto: "TCP6", local_addr: addr, pid: r.dwOwningPid });
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn platform_tcp_ports() -> Vec<ListeningPort> {
    use std::collections::HashMap;
    let mut entries = Vec::new();
    if let Ok(raw) = std::fs::read_to_string("/proc/net/tcp") {
        entries.extend(parse_proc_net_tcp_listen(&raw, false));
    }
    if let Ok(raw6) = std::fs::read_to_string("/proc/net/tcp6") {
        entries.extend(parse_proc_net_tcp_listen(&raw6, true));
    }
    if entries.is_empty() {
        return Vec::new();
    }
    let mut inode_pid: HashMap<u64, u32> = HashMap::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for ent in rd.flatten() {
            let Ok(pid) = ent.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(fds) = std::fs::read_dir(ent.path().join("fd")) else { continue };
            for fd in fds.flatten() {
                if let Ok(link) = std::fs::read_link(fd.path())
                    && let Some(s) = link.to_str()
                    && let Some(rest) = s.strip_prefix("socket:[").and_then(|v| v.strip_suffix(']'))
                    && let Ok(inode) = rest.parse::<u64>()
                {
                    inode_pid.entry(inode).or_insert(pid);
                }
            }
        }
    }
    entries
        .into_iter()
        .filter_map(|(proto, addr, port, inode)| {
            inode_pid.get(&inode).map(|&pid| ListeningPort {
                port,
                proto,
                local_addr: addr,
                pid,
            })
        })
        .collect()
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn platform_tcp_ports() -> Vec<ListeningPort> {
    Vec::new()
}

#[allow(dead_code)]
pub(crate) fn parse_proc_net_tcp_listen(raw: &str, is_v6: bool) -> Vec<(&'static str, String, u16, u64)> {
    let proto = if is_v6 { "TCP6" } else { "TCP4" };
    let mut out = Vec::new();
    for line in raw.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 || cols[3] != "0A" {
            continue;
        }
        let Some((ip_hex, port_hex)) = cols[1].split_once(':') else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        let Ok(inode) = cols[9].parse::<u64>() else {
            continue;
        };
        let addr = decode_proc_ip(ip_hex, is_v6);
        out.push((proto, addr, port, inode));
    }
    out
}

#[allow(dead_code)]
fn decode_proc_ip(hex: &str, is_v6: bool) -> String {
    if !is_v6 && hex.len() == 8 && let Ok(v) = u32::from_str_radix(hex, 16) {
        return std::net::Ipv4Addr::from(u32::from_be(v)).to_string();
    }
    if is_v6 && hex.len() == 32 {
        let mut bytes = [0u8; 16];
        for g in 0..4 {
            if let Ok(w) = u32::from_str_radix(&hex[g * 8..(g + 1) * 8], 16) {
                bytes[g * 4..(g + 1) * 4].copy_from_slice(&w.to_ne_bytes());
            }
        }
        return std::net::Ipv6Addr::from(bytes).to_string();
    }
    hex.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_net_tcp_listen_lines() {
        let sample = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                       0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 12345 1\n\
                       1: 00000000:0050 00000000:0000 01 00000000:00000000 00:00000000 00000000  1000 0 99999 1\n";
        let rows = parse_proc_net_tcp_listen(sample, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], ("TCP4", "127.0.0.1".into(), 8080, 12345));
    }

    #[test]
    fn dedup_and_sort_listening_ports_orders_by_port() {
        let mut list = vec![
            ListeningPort { port: 8080, proto: "TCP4", local_addr: "0.0.0.0".into(), pid: 400 },
            ListeningPort { port: 80, proto: "TCP4", local_addr: "0.0.0.0".into(), pid: 100 },
            ListeningPort { port: 8080, proto: "TCP4", local_addr: "0.0.0.0".into(), pid: 400 },
        ];
        dedup_and_sort(&mut list);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].port, 80);
        assert_eq!(list[1].port, 8080);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn tcp_listener_is_detected_by_port_scanner() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
        let port = listener.local_addr().unwrap().port();
        let pid = std::process::id();
        let hits = list_listening_tcp_ports();
        assert!(hits.iter().any(|p| p.port == port && p.pid == pid && p.proto == "TCP4"));
        drop(listener);
    }
}
