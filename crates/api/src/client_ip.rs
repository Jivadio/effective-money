// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's IP address, for the login rate limit.
//!
//! `X-Forwarded-For` is trusted only when the TCP peer is one of the
//! configured reverse proxies (`SOLDE_TRUSTED_PROXIES`). Otherwise anyone
//! could pick their own address by sending the header, so it is ignored.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use ipnet::IpNet;

use crate::state::AppState;

/// The reverse proxies allowed to report the client address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<IpNet>);

/// An entry of the trusted proxy list is neither an IP address nor a CIDR
/// range. Carries the entry, which is configuration, not personal data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidProxy(pub String);

impl fmt::Display for InvalidProxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not an IP address or CIDR range (e.g. 10.0.0.0/8 or 172.18.0.2)",
            self.0
        )
    }
}

impl std::error::Error for InvalidProxy {}

impl TrustedProxies {
    /// Parses a comma- or space-separated list of CIDR ranges or single
    /// addresses (`"10.0.0.0/8, 172.18.0.2, fd00::/8"`). Empty: no proxy.
    ///
    /// ```
    /// let proxies = solde_api::TrustedProxies::parse("10.0.0.0/8, 192.0.2.1");
    /// assert!(proxies.is_ok_and(|p| !p.is_empty()));
    /// assert!(solde_api::TrustedProxies::parse("not-an-ip").is_err());
    /// ```
    pub fn parse(list: &str) -> Result<Self, InvalidProxy> {
        list.split(|c: char| c == ',' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .parse::<IpNet>()
                    .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                    .map(|net| net.trunc())
                    .map_err(|_| InvalidProxy(entry.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(TrustedProxies)
    }

    /// Whether no proxy is trusted (the header is then always ignored).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.0.iter().any(|net| net.contains(&ip))
    }

    /// The client address: the TCP peer, unless the peer is a trusted proxy,
    /// in which case the right-most `X-Forwarded-For` hop that is not itself
    /// a trusted proxy (an unparsable hop stops the walk).
    pub(crate) fn client_ip(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let peer = peer.to_canonical();
        if !self.contains(peer) {
            return peer;
        }
        let hops: Vec<&str> = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .collect();
        let mut client = peer;
        for hop in hops.iter().rev() {
            let Some(ip) = parse_hop(hop) else {
                break;
            };
            client = ip.to_canonical();
            if !self.contains(client) {
                break;
            }
        }
        client
    }
}

/// One `X-Forwarded-For` hop: a bare address, or one with a port
/// (`192.0.2.1:443`, `[2001:db8::1]:443`) or in brackets (`[2001:db8::1]`),
/// as some proxies write them.
fn parse_hop(hop: &str) -> Option<IpAddr> {
    let address = if let Some(rest) = hop.strip_prefix('[') {
        let (inside, after) = rest.split_once(']')?;
        if !(after.is_empty() || after.strip_prefix(':').is_some_and(is_port)) {
            return None;
        }
        inside
    } else {
        match hop.split_once(':') {
            // Exactly one colon: IPv4 with a port. More: a bare IPv6.
            Some((host, port)) if !port.contains(':') => {
                if !is_port(port) {
                    return None;
                }
                host
            }
            _ => hop,
        }
    };
    address.parse().ok()
}

fn is_port(text: &str) -> bool {
    !text.is_empty() && text.len() <= 5 && text.bytes().all(|b| b.is_ascii_digit())
}

/// The client address as seen by [`TrustedProxies::client_ip`]. Without
/// connection info (only in tests that skip it) the peer counts as
/// `0.0.0.0`.
pub(crate) struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |info| info.0.ip());
        Ok(ClientIp(
            state.config.trusted_proxies.client_ip(peer, &parts.headers),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    fn forwarded(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_str(value).expect("header"),
        );
        headers
    }

    #[test]
    fn parses_lists() {
        let proxies = TrustedProxies::parse(" 10.0.0.0/8,192.0.2.1  fd00::/8 ").expect("valid");
        assert_eq!(proxies.0.len(), 3);
        assert!(TrustedProxies::parse("").expect("empty").is_empty());
        assert_eq!(
            TrustedProxies::parse("10.0.0.0/8, nope"),
            Err(InvalidProxy("nope".to_owned()))
        );
    }

    #[test]
    fn header_ignored_without_trusted_peer() {
        let none = TrustedProxies::default();
        let headers = forwarded("198.51.100.7");
        assert_eq!(
            none.client_ip(ip("203.0.113.9"), &headers),
            ip("203.0.113.9")
        );
        let some = TrustedProxies::parse("10.0.0.0/8").expect("valid");
        assert_eq!(
            some.client_ip(ip("203.0.113.9"), &headers),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn walks_trusted_hops_from_the_right() {
        let proxies = TrustedProxies::parse("10.0.0.0/8").expect("valid");
        let peer = ip("10.0.0.2");
        // A client-supplied left-most entry cannot win over the real hop.
        let headers = forwarded("192.0.2.66, 198.51.100.7, 10.0.0.5");
        assert_eq!(proxies.client_ip(peer, &headers), ip("198.51.100.7"));
        assert_eq!(proxies.client_ip(peer, &HeaderMap::new()), peer);
        assert_eq!(
            proxies.client_ip(peer, &forwarded("198.51.100.7, garbage")),
            peer
        );
        // Ports and brackets are stripped.
        for hop in [
            "198.51.100.7:4711",
            "[2001:db8::7]:443",
            "[2001:db8::7]",
            "2001:db8::7",
        ] {
            let expected = if hop.contains("::") {
                ip("2001:db8::7")
            } else {
                ip("198.51.100.7")
            };
            assert_eq!(proxies.client_ip(peer, &forwarded(hop)), expected, "{hop}");
        }
        for bad in ["198.51.100.7:http", "[2001:db8::7]x", "[2001:db8::7"] {
            assert_eq!(proxies.client_ip(peer, &forwarded(bad)), peer, "{bad}");
        }
        // IPv4-mapped IPv6 peers match IPv4 ranges.
        assert_eq!(
            proxies.client_ip(ip("::ffff:10.0.0.2"), &forwarded("198.51.100.7")),
            ip("198.51.100.7")
        );
    }
}
