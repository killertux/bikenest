//! The client address used as a rate-limit key.
//!
//! Rate limits are only worth anything if the identity they are keyed on cannot
//! be chosen by the caller. `X-Forwarded-For` is a plain request header: anyone
//! can send one, and anyone can send a different one on every request. So the
//! *only* address trusted by default is the TCP peer — the address axum's
//! `ConnectInfo` reports — and forwarded headers are read at all only when the
//! operator has declared how many reverse proxies sit in front of the app
//! (`TRUSTED_PROXY_HOPS`, see [`Config::trusted_proxy_hops`]).
//!
//! `X-Real-IP` is ignored entirely: it is not standardised, carries no notion
//! of a hop count, and is trivially spoofable — a single header would let one
//! client occupy an unbounded number of rate-limit buckets.
//!
//! [`Config::trusted_proxy_hops`]: bikenest_infrastructure::Config::trusted_proxy_hops

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use crate::http::AppState;

/// Stand-in identity when the peer address is unavailable.
///
/// In production `ConnectInfo` is always present (the server is built with
/// `into_make_service_with_connect_info`). It is absent when the router is
/// driven directly as a `tower::Service` — which is how the HTTP test suite
/// calls it — where a single shared bucket is exactly what the tests want.
pub const UNKNOWN_PEER: &str = "test-peer";

/// The resolved client address, as an extractor.
///
/// Handlers take `ClientIp(ip): ClientIp`; it never fails, and it must appear
/// before any body extractor (`Form`, `Multipart`) in the argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIp(pub String);

/// The rule, as a pure function so it can be tested exhaustively.
///
/// * `hops == 0` — no proxy is trusted: the peer address, and nothing else.
/// * `hops == N` — the app sits behind N reverse proxies, each of which
///   appends the address it saw to `X-Forwarded-For`. The outermost trusted
///   proxy's entry is therefore the *last* one, so the address it received the
///   request from is the Nth entry counting from the right. Anything a client
///   prepends lands to the left of that and is ignored.
/// * Fewer than N entries, or an entry that is not an IP address, means the
///   chain is not what was configured — fall back to the peer address rather
///   than trusting a value the client may have written.
pub fn resolve_client_ip(peer: Option<IpAddr>, xff: Option<&str>, hops: u8) -> String {
    let peer_ip = || {
        peer.map(|p| p.to_string())
            .unwrap_or_else(|| UNKNOWN_PEER.to_string())
    };
    if hops == 0 {
        return peer_ip();
    }
    let Some(xff) = xff else {
        return peer_ip();
    };
    let entries: Vec<&str> = xff
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(index) = entries.len().checked_sub(hops as usize) else {
        return peer_ip();
    };
    match entries.get(index) {
        // Only a bare IP is accepted; `ip:port` forms and hostnames are not,
        // so a malformed chain degrades to the peer instead of minting a
        // caller-chosen bucket key.
        Some(candidate) if candidate.parse::<IpAddr>().is_ok() => (*candidate).to_string(),
        _ => peer_ip(),
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip());
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());
        Ok(ClientIp(resolve_client_ip(
            peer,
            forwarded,
            state.config.trusted_proxy_hops,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> Option<IpAddr> {
        Some("203.0.113.9".parse().unwrap())
    }

    #[test]
    fn without_a_trusted_proxy_the_header_is_ignored() {
        assert_eq!(
            resolve_client_ip(peer(), Some("1.2.3.4"), 0),
            "203.0.113.9",
            "a spoofed X-Forwarded-For must not choose the bucket"
        );
        assert_eq!(resolve_client_ip(peer(), None, 0), "203.0.113.9");
    }

    #[test]
    fn one_hop_takes_the_last_entry() {
        // The single trusted proxy appended "198.51.100.7"; everything to its
        // left was supplied by the client.
        assert_eq!(
            resolve_client_ip(peer(), Some("1.2.3.4, 198.51.100.7"), 1),
            "198.51.100.7"
        );
        assert_eq!(
            resolve_client_ip(peer(), Some("198.51.100.7"), 1),
            "198.51.100.7"
        );
    }

    #[test]
    fn two_hops_take_the_second_entry_from_the_right() {
        assert_eq!(
            resolve_client_ip(peer(), Some("1.2.3.4, 198.51.100.7, 10.0.0.1"), 2),
            "198.51.100.7"
        );
    }

    #[test]
    fn too_few_entries_fall_back_to_the_peer() {
        assert_eq!(
            resolve_client_ip(peer(), Some("198.51.100.7"), 2),
            "203.0.113.9"
        );
        assert_eq!(resolve_client_ip(peer(), Some(""), 1), "203.0.113.9");
        assert_eq!(resolve_client_ip(peer(), None, 2), "203.0.113.9");
    }

    #[test]
    fn a_non_address_entry_falls_back_to_the_peer() {
        assert_eq!(
            resolve_client_ip(peer(), Some("not-an-ip"), 1),
            "203.0.113.9"
        );
        // `ip:port` is not a bare address either.
        assert_eq!(
            resolve_client_ip(peer(), Some("198.51.100.7:443"), 1),
            "203.0.113.9"
        );
    }

    #[test]
    fn whitespace_and_empty_entries_are_tolerated() {
        assert_eq!(
            resolve_client_ip(peer(), Some("  1.2.3.4 ,,  198.51.100.7  "), 1),
            "198.51.100.7"
        );
    }

    #[test]
    fn ipv6_peers_and_entries_work() {
        let v6: Option<IpAddr> = Some("2001:db8::1".parse().unwrap());
        assert_eq!(resolve_client_ip(v6, Some("1.2.3.4"), 0), "2001:db8::1");
        assert_eq!(
            resolve_client_ip(peer(), Some("1.2.3.4, 2001:db8::2"), 1),
            "2001:db8::2"
        );
    }

    #[test]
    fn a_missing_peer_uses_the_placeholder() {
        assert_eq!(resolve_client_ip(None, Some("1.2.3.4"), 0), UNKNOWN_PEER);
        assert_eq!(resolve_client_ip(None, None, 3), UNKNOWN_PEER);
    }
}
