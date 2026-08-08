//! SSRF guard: refuse to fetch URLs that resolve to loopback, private,
//! link-local, or otherwise non-public addresses.
//!
//! `distill` is exposed as an MCP tool that agents can call with arbitrary
//! URLs, so the fetch path must not become a pivot into the local network or
//! the cloud metadata endpoint (`169.254.169.254`). The guard is on by default
//! and can be disabled for trusted contexts (e.g. distilling a local dev
//! server) with `DISTILL_ALLOW_PRIVATE_HOSTS=1`.

use anyhow::{bail, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use url::{Host, Url};

/// Is the guard currently active? Disabled when `DISTILL_ALLOW_PRIVATE_HOSTS`
/// is set to a truthy value (`1`, `true`, `yes`, case-insensitive).
pub fn guard_enabled() -> bool {
    match std::env::var("DISTILL_ALLOW_PRIVATE_HOSTS") {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => true,
    }
}

/// Validate a URL before fetching it. When the guard is enabled this rejects
/// non-HTTP(S) schemes and any host that resolves entirely to blocked
/// addresses. A hostname is resolved via DNS and *every* returned address is
/// checked, so a name that points at a private IP is caught too.
pub fn check_url(url: &Url) -> Result<()> {
    if !guard_enabled() {
        return Ok(());
    }

    match url.scheme() {
        "http" | "https" => {}
        other => bail!("blocked non-HTTP(S) scheme '{other}' (SSRF guard)"),
    }

    let host = url
        .host()
        .ok_or_else(|| anyhow::anyhow!("URL has no host (SSRF guard)"))?;

    match host {
        Host::Ipv4(ip) => reject_if_blocked(IpAddr::V4(ip), url)?,
        Host::Ipv6(ip) => reject_if_blocked(IpAddr::V6(ip), url)?,
        Host::Domain(name) => {
            // Port is irrelevant to the address classification; use 0 so we can
            // resolve names that don't carry an explicit port.
            let port = url.port_or_known_default().unwrap_or(80);
            let mut resolved = (name, port)
                .to_socket_addrs()
                .map_err(|e| anyhow::anyhow!("cannot resolve host '{name}': {e}"))?
                .peekable();
            if resolved.peek().is_none() {
                bail!("host '{name}' did not resolve to any address (SSRF guard)");
            }
            for addr in resolved {
                reject_if_blocked(addr.ip(), url)?;
            }
        }
    }
    Ok(())
}

fn reject_if_blocked(ip: IpAddr, url: &Url) -> Result<()> {
    if is_blocked_ip(ip) {
        bail!("blocked request to non-public address {ip} for {url} (SSRF guard; set DISTILL_ALLOW_PRIVATE_HOSTS=1 to override)");
    }
    Ok(())
}

/// Classify an IP as non-public (and therefore blocked). Covers loopback,
/// private ranges, link-local (incl. the `169.254.169.254` metadata address),
/// carrier-grade NAT, unspecified/broadcast, and their IPv6 equivalents.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        // An IPv4-mapped IPv6 address (::ffff:a.b.c.d) must be judged by its
        // embedded v4 address, otherwise `::ffff:127.0.0.1` would slip through.
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_blocked_v4(v4),
            None => is_blocked_v6(v6),
        },
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()            // 127.0.0.0/8
        || ip.is_private()      // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local()   // 169.254/16 (includes cloud metadata)
        || ip.is_unspecified()  // 0.0.0.0
        || ip.is_broadcast()    // 255.255.255.255
        || ip.is_documentation()
        || is_shared_v4(ip)     // 100.64/10 CGNAT
}

/// 100.64.0.0/10 — carrier-grade NAT shared space (RFC 6598).
fn is_shared_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return true;
    }
    let first = ip.octets()[0];
    let seg0 = ip.segments()[0];
    // fc00::/7 unique local addresses.
    (first & 0xfe) == 0xfc
        // fe80::/10 link-local.
        || (seg0 & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn blocked(s: &str) -> bool {
        is_blocked_ip(IpAddr::from_str(s).unwrap())
    }

    #[test]
    fn blocks_loopback_private_and_metadata() {
        assert!(blocked("127.0.0.1"));
        assert!(blocked("10.0.0.5"));
        assert!(blocked("172.16.4.9"));
        assert!(blocked("192.168.1.1"));
        assert!(blocked("169.254.169.254")); // cloud metadata
        assert!(blocked("100.100.0.1")); // CGNAT
        assert!(blocked("0.0.0.0"));
        assert!(blocked("::1"));
        assert!(blocked("fe80::1")); // link-local v6
        assert!(blocked("fd00::1")); // unique-local v6
        assert!(blocked("::ffff:127.0.0.1")); // v4-mapped loopback
    }

    #[test]
    fn allows_public_addresses() {
        assert!(!blocked("1.1.1.1"));
        assert!(!blocked("8.8.8.8"));
        assert!(!blocked("140.82.121.4")); // github
        assert!(!blocked("2606:4700:4700::1111")); // cloudflare v6
    }

    #[test]
    fn non_http_scheme_is_rejected() {
        let url = Url::parse("file:///etc/passwd").unwrap();
        assert!(check_url(&url).is_err());
    }

    #[test]
    fn ip_literal_loopback_url_is_rejected() {
        let url = Url::parse("http://127.0.0.1:8080/admin").unwrap();
        assert!(check_url(&url).is_err());
    }
}
