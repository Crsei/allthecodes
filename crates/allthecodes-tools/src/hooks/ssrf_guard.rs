//! SSRF guard for HTTP hooks.
//!
//! Blocks private, link-local, and other non-routable address ranges to prevent
//! project-configured HTTP hooks from reaching cloud metadata endpoints
//! (169.254.169.254) or internal infrastructure.
//!
//! Loopback (127.0.0.0/8, ::1) is intentionally ALLOWED — local dev policy
//! servers are a primary HTTP hook use case.
//!
//! Port of TypeScript `ssrfGuard.ts`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Returns true if the address is in a range that HTTP hooks should not reach.
///
/// Blocked IPv4:
///   0.0.0.0/8        "this" network
///   10.0.0.0/8       private
///   100.64.0.0/10    shared address space / CGNAT
///   169.254.0.0/16   link-local (cloud metadata)
///   172.16.0.0/12    private
///   192.168.0.0/16   private
///
/// Blocked IPv6:
///   ::               unspecified
///   fc00::/7         unique local
///   fe80::/10        link-local
pub fn is_blocked_address(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    let [a, b, _, _] = octets;

    // Loopback explicitly allowed
    if a == 127 {
        return false;
    }

    // 0.0.0.0/8
    if a == 0 {
        return true;
    }
    // 10.0.0.0/8
    if a == 10 {
        return true;
    }
    // 169.254.0.0/16 — link-local, cloud metadata
    if a == 169 && b == 254 {
        return true;
    }
    // 172.16.0.0/12
    if a == 172 && (16..=31).contains(&b) {
        return true;
    }
    // 100.64.0.0/10 — shared address space (RFC 6598, CGNAT).
    // Some cloud providers use this range for metadata endpoints.
    if a == 100 && (64..=127).contains(&b) {
        return true;
    }
    // 192.168.0.0/16
    if a == 192 && b == 168 {
        return true;
    }

    false
}

fn is_blocked_v6(addr: Ipv6Addr) -> bool {
    let segments = addr.segments();

    // ::1 loopback explicitly allowed
    if addr.is_loopback() {
        return false;
    }

    // :: unspecified
    if addr.is_unspecified() {
        return true;
    }

    // IPv4-mapped IPv6 (::ffff:x.x.x.x)
    if let Some(v4_addr) = addr.to_ipv4_mapped() {
        return is_blocked_v4(v4_addr);
    }

    // fc00::/7 — unique local addresses
    if segments[0] & 0xfe00 == 0xfc00 {
        return true;
    }

    // fe80::/10 — link-local
    if segments[0] & 0xffc0 == 0xfe80 {
        return true;
    }

    false
}

/// SSRF guard that validates DNS resolution results.
pub struct SsrfGuard;

impl SsrfGuard {
    /// Check if a hostname resolves to a blocked address.
    /// Returns `Ok(())` if the address is allowed, `Err` with a message if blocked.
    pub fn check_address(addr: IpAddr) -> Result<(), String> {
        if is_blocked_address(addr) {
            Err(format!(
                "HTTP hook blocked: resolves to {addr} (private/link-local address). \
                 Loopback (127.0.0.1, ::1) is allowed for local dev."
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_loopback_v4_allowed() {
        assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            127, 255, 255, 255
        ))));
    }

    #[test]
    fn test_loopback_v6_allowed() {
        assert!(!is_blocked_address(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn test_private_v4_blocked() {
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            10, 255, 255, 255
        ))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            192, 168, 0, 1
        ))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            192, 168, 255, 255
        ))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            172, 31, 255, 255
        ))));
    }

    #[test]
    fn test_cloud_metadata_blocked() {
        // AWS/GCP/Azure metadata endpoint
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            169, 254, 169, 254
        ))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            169, 254, 1, 1
        ))));
        // Alibaba Cloud metadata
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            100, 100, 100, 200
        ))));
    }

    #[test]
    fn test_link_local_blocked() {
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            169, 254, 0, 1
        ))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            169, 254, 255, 255
        ))));
    }

    #[test]
    fn test_this_network_blocked() {
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            0, 255, 255, 255
        ))));
    }

    #[test]
    fn test_public_v4_allowed() {
        assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            203, 0, 113, 1
        ))));
    }

    #[test]
    fn test_unique_local_v6_blocked() {
        assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
            0xfd00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
            0xfdff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff
        ))));
    }

    #[test]
    fn test_link_local_v6_blocked() {
        assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn test_unspecified_v6_blocked() {
        assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
    }

    #[test]
    fn test_public_v6_allowed() {
        assert!(!is_blocked_address(IpAddr::V6(Ipv6Addr::new(
            0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888
        ))));
    }

    #[test]
    fn test_ipv4_mapped_ipv6_checked_against_v4_ranges() {
        // ::ffff:169.254.169.254 should be blocked (cloud metadata)
        let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xa9fe, 0xa9fe));
        assert!(is_blocked_address(mapped));

        // ::ffff:10.0.0.1 should be blocked (private)
        let mapped_private = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0a00, 0x0001));
        assert!(is_blocked_address(mapped_private));

        // ::ffff:8.8.8.8 should be allowed
        let mapped_public = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0808, 0x0808));
        assert!(!is_blocked_address(mapped_public));
    }

    #[test]
    fn test_cgnat_range_blocked() {
        // 100.64.0.0/10 includes 100.64.0.1 through 100.127.255.255
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 255
        ))));
        assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(
            100, 128, 0, 1
        ))));
    }
}
