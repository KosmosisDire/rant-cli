//! This machine's addresses, which tell a peer on it from one elsewhere.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Every address of every interface, loopback included.
pub fn local_addresses() -> Vec<IpAddr> {
    let mut out = vec![IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V6(Ipv6Addr::LOCALHOST)];
    out.extend(os::interface_addresses());
    out
}

#[cfg(windows)]
mod os {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6};

    pub fn interface_addresses() -> Vec<IpAddr> {
        let mut size = 16 * 1024u32;
        let mut buf: Vec<u64> = Vec::new();
        let mut rc = ERROR_BUFFER_OVERFLOW;
        for _ in 0..3 {
            if rc != ERROR_BUFFER_OVERFLOW {
                break;
            }
            buf.resize((size as usize).div_ceil(8), 0);
            let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
            // SAFETY: buf holds size bytes, aligned for the list.
            rc = unsafe { GetAdaptersAddresses(AF_UNSPEC as u32, flags, std::ptr::null(), buf.as_mut_ptr().cast(), &mut size) };
        }
        let mut out = Vec::new();
        if rc != NO_ERROR {
            return out;
        }
        // SAFETY: on success the buffer holds linked lists Windows wrote, valid while buf lives.
        unsafe {
            let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
            while !a.is_null() {
                let mut u = (*a).FirstUnicastAddress;
                while !u.is_null() {
                    out.extend(address((*u).Address.lpSockaddr));
                    u = (*u).Next;
                }
                a = (*a).Next;
            }
        }
        out
    }

    /// SAFETY: sa is null or a socket address of its family's size.
    unsafe fn address(sa: *const SOCKADDR) -> Option<IpAddr> {
        if sa.is_null() {
            return None;
        }
        unsafe {
            match (*sa).sa_family {
                AF_INET => Some(IpAddr::V4(Ipv4Addr::from(u32::from_be((*(sa as *const SOCKADDR_IN)).sin_addr.S_un.S_addr)))),
                AF_INET6 => Some(IpAddr::V6(Ipv6Addr::from((*(sa as *const SOCKADDR_IN6)).sin6_addr.u.Byte))),
                _ => None,
            }
        }
    }
}

#[cfg(unix)]
mod os {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    pub fn interface_addresses() -> Vec<IpAddr> {
        let mut out = Vec::new();
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        // SAFETY: getifaddrs fills a list freed below, each address sized by its family.
        unsafe {
            if libc::getifaddrs(&mut list) != 0 {
                return out;
            }
            let mut i = list;
            while !i.is_null() {
                let sa = (*i).ifa_addr;
                if !sa.is_null() {
                    match (*sa).sa_family as libc::c_int {
                        libc::AF_INET => out.push(IpAddr::V4(Ipv4Addr::from(u32::from_be((*(sa as *const libc::sockaddr_in)).sin_addr.s_addr)))),
                        libc::AF_INET6 => out.push(IpAddr::V6(Ipv6Addr::from((*(sa as *const libc::sockaddr_in6)).sin6_addr.s6_addr))),
                        _ => {}
                    }
                }
                i = (*i).ifa_next;
            }
            libc::freeifaddrs(list);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_is_always_local() {
        let all = super::local_addresses();
        assert!(all.contains(&"127.0.0.1".parse().unwrap()) && all.contains(&"::1".parse().unwrap()));
    }
}
