//! Name lookups without glibc. A static glibc binary resolves a name by loading the system's
//! NSS modules, which crashes when they come from another glibc version. So on Linux the
//! HTTP client reads /etc/hosts, then asks the nameservers of /etc/resolv.conf over UDP.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::Duration;

const A: u16 = 1;
const AAAA: u16 = 28;

/// The addresses of host, IPv4 first, IPv6 only when it has no IPv4 address.
pub fn lookup(host: &str, timeout: Duration) -> Result<Vec<IpAddr>, String> {
    if let Ok(ip) = host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let hosts = in_hosts(&std::fs::read_to_string("/etc/hosts").unwrap_or_default(), host);
    if !hosts.is_empty() {
        return Ok(hosts);
    }
    let servers = nameservers(&std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default());
    if servers.is_empty() {
        return Err("no nameserver in /etc/resolv.conf".into());
    }
    let mut last = format!("no address for {host}");
    for qtype in [A, AAAA] {
        for server in &servers {
            match ask(*server, host, qtype, timeout) {
                Ok(ips) if !ips.is_empty() => return Ok(ips),
                Ok(_) => {}
                Err(e) => last = e,
            }
        }
    }
    Err(last)
}

fn in_hosts(text: &str, host: &str) -> Vec<IpAddr> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let ip = words.next()?.parse::<IpAddr>().ok()?;
            words.any(|n| n.eq_ignore_ascii_case(host)).then_some(ip)
        })
        .collect()
}

fn nameservers(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            (words.next()? == "nameserver").then(|| words.next()?.split('%').next()?.parse().ok())?
        })
        .collect()
}

fn query(id: u16, host: &str, qtype: u16) -> Vec<u8> {
    let mut q = Vec::with_capacity(32 + host.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);    // recursion desired, one question
    for label in host.trim_end_matches('.').split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    q
}

/// The offset just past a name at i, which ends in a zero label or a compression pointer.
fn skip_name(b: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let len = *b.get(i)? as usize;
        if len == 0 {
            return Some(i + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(i + 2);
        }
        i += 1 + len;
    }
}

/// The addresses of type qtype in an answer to the query id. A CNAME on the way is passed
/// over, since a recursive server sends the addresses it leads to in the same answer.
fn answers(b: &[u8], id: u16, qtype: u16) -> Result<Vec<IpAddr>, String> {
    let word = |i: usize| -> Option<u16> { Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])) };
    let bad = || "a nameserver sent an answer that cannot be read".to_string();
    if word(0) != Some(id) {
        return Err(bad());
    }
    match word(2).ok_or_else(bad)? & 0x000F {
        0 => {}
        3 => return Ok(Vec::new()),    // no such name
        code => return Err(format!("a nameserver refused the lookup with code {code}")),
    }
    let (questions, count) = (word(4).ok_or_else(bad)?, word(6).ok_or_else(bad)?);
    let mut i = 12;
    for _ in 0..questions {
        i = skip_name(b, i).ok_or_else(bad)? + 4;
    }
    let mut out = Vec::new();
    for _ in 0..count {
        i = skip_name(b, i).ok_or_else(bad)?;
        let (ty, len) = (word(i).ok_or_else(bad)?, word(i + 8).ok_or_else(bad)? as usize);
        let data = b.get(i + 10..i + 10 + len).ok_or_else(bad)?;
        match (ty, len) {
            (t, 4) if t == qtype => out.push(IpAddr::V4(Ipv4Addr::new(data[0], data[1], data[2], data[3]))),
            (t, 16) if t == qtype => out.push(IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(data).unwrap()))),
            _ => {}
        }
        i += 10 + len;
    }
    Ok(out)
}

fn ask(server: IpAddr, host: &str, qtype: u16, timeout: Duration) -> Result<Vec<IpAddr>, String> {
    let bind: SocketAddr = if server.is_ipv4() { "0.0.0.0:0".parse().unwrap() } else { "[::]:0".parse().unwrap() };
    let socket = UdpSocket::bind(bind).map_err(|e| format!("cannot open a socket for a lookup: {e}"))?;
    socket.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    socket.connect(SocketAddr::new(server, 53)).map_err(|e| format!("cannot reach the nameserver {server}: {e}"))?;
    let id = (std::process::id() as u16) ^ (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u16);
    socket.send(&query(id, host, qtype)).map_err(|e| format!("cannot reach the nameserver {server}: {e}"))?;
    let mut buf = [0u8; 1500];
    let n = socket.recv(&mut buf).map_err(|_| format!("the nameserver {server} did not answer"))?;
    answers(&buf[..n], id, qtype)
}

/// The resolver the HTTP client uses on Linux.
#[derive(Debug)]
pub struct OwnResolver;

impl ureq::unversioned::resolver::Resolver for OwnResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let host = uri.host().ok_or(ureq::Error::HostNotFound)?;
        let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("http") { 80 } else { 443 });
        let ips = lookup(host, Duration::from_secs(5)).map_err(|_| ureq::Error::HostNotFound)?;
        let mut out = self.empty();
        for ip in ips.into_iter().take(16) {
            out.push(SocketAddr::new(ip, port));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_resolv_conf_are_read() {
        let hosts = "127.0.0.1 localhost\n# 10.0.0.1 nope\n10.0.0.2 karbon.lan karbon # lab\n";
        assert_eq!(in_hosts(hosts, "KARBON"), vec!["10.0.0.2".parse::<IpAddr>().unwrap()]);
        assert!(in_hosts(hosts, "nope").is_empty());
        let conf = "search lan\nnameserver 127.0.0.53\nnameserver fe80::1%eth0\noptions edns0\n";
        assert_eq!(nameservers(conf), vec!["127.0.0.53".parse::<IpAddr>().unwrap(), "fe80::1".parse().unwrap()]);
    }

    #[test]
    fn an_answer_through_a_cname_gives_its_addresses() {
        let mut b = query(7, "api.github.com", A);
        b[2] = 0x81;
        b[3] = 0x80;
        b[7] = 2;    // two answers
        // a CNAME to a name, compressed to point at the question
        b.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 12]);
        b.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 140, 82, 112, 6]);
        assert_eq!(answers(&b, 7, A).unwrap(), vec!["140.82.112.6".parse::<IpAddr>().unwrap()]);
        assert!(answers(&b, 8, A).is_err(), "another query's answer");
    }
}
