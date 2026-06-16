//! [`ListenUrl`] — parsed representation of the `--listen` CLI argument.
//!
//! This is the entry-point type that maps the user-facing `--listen` string
//! into a structured enum. Downstream code (in particular [`ServerMode`]) uses
//! the enum rather than re-parsing the string.
//!
//! # Syntax
//!
//! | Example | Meaning |
//! |---------|---------|
//! | `off` | No HTTP server |
//! | `stdio://` | stdio-based IPC (headless) |
//! | `web://127.0.0.1:17322` | Web UI server only |
//! | `daemon://127.0.0.1:19836` | Daemon API server only |
//! | `all://web=127.0.0.1:17322,daemon=127.0.0.1:19836` | Both servers |

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// Internal transport family for connection lifecycle events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportKind {
    HeadlessStdio,
    IpcWebSocket,
    ApiRpcWebSocket,
}

/// Opaque internal connection identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectionId(String);

impl ConnectionId {
    pub fn next() -> Self {
        Self(format!(
            "connection-{}",
            NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    pub fn from_static(value: &'static str) -> Self {
        Self(value.to_string())
    }

    pub fn from_string(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Origin metadata captured at connection open time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionOrigin {
    /// Non-browser/native client. WebSocket clients that omit Origin fall here.
    LocalNative,
    /// Browser WebSocket with an accepted loopback Origin header.
    Browser { origin: String },
}

impl ConnectionOrigin {
    pub fn local_native() -> Self {
        Self::LocalNative
    }

    pub fn browser(origin: impl Into<String>) -> Self {
        Self::Browser {
            origin: origin.into(),
        }
    }

    pub fn from_websocket_origin(origin: Option<&str>) -> Result<Self, OriginRejection> {
        match origin {
            None => Ok(Self::LocalNative),
            Some(origin) if websocket_origin_is_allowed(origin) => Ok(Self::browser(origin)),
            Some(origin) => Err(OriginRejection {
                origin: origin.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginRejection {
    pub origin: String,
}

impl std::fmt::Display for OriginRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "websocket origin is not allowed: {}", self.origin)
    }
}

impl std::error::Error for OriginRejection {}

/// Reason a transport connection closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionClosedReason {
    ClientClosed,
    StdinEof,
    ProtocolQuit,
    ServerShutdown,
    TransportError(String),
}

/// Transport-neutral inbound connection event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportEvent<T> {
    ConnectionOpened {
        connection_id: ConnectionId,
        kind: TransportKind,
        origin: ConnectionOrigin,
    },
    IncomingMessage {
        connection_id: ConnectionId,
        kind: TransportKind,
        message: T,
    },
    ConnectionClosed {
        connection_id: ConnectionId,
        kind: TransportKind,
        reason: ConnectionClosedReason,
    },
}

/// Transport-neutral outbound message addressed to one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundEnvelope<T> {
    pub connection_id: ConnectionId,
    pub message: T,
}

impl<T> OutboundEnvelope<T> {
    pub fn new(connection_id: ConnectionId, message: T) -> Self {
        Self {
            connection_id,
            message,
        }
    }
}

/// A parsed `--listen` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenUrl {
    /// Web UI server (chat, static files, protocol handlers).
    Web(SocketAddr),
    /// Daemon API server (KAIROS API, SSE, webhook, gateway).
    Daemon(SocketAddr),
    /// Run both servers in the same process.
    All {
        web_addr: SocketAddr,
        daemon_addr: SocketAddr,
    },
    /// stdio mode (headless IPC).
    Stdio,
    /// No HTTP server (TUI or headless mode).
    Off,
}

impl ListenUrl {
    /// Parse a `--listen` argument into a [`ListenUrl`].
    ///
    /// Returns an error when the input does not match any known format.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let trimmed = input.trim();

        // "off" (case-insensitive)
        if trimmed.eq_ignore_ascii_case("off") {
            return Ok(Self::Off);
        }

        // "stdio://"
        if trimmed.starts_with("stdio://") {
            return Ok(Self::Stdio);
        }

        // "web://addr:port"
        if let Some(addr_str) = trimmed.strip_prefix("web://") {
            let addr = parse_socket_addr(addr_str, "web")?;
            return Ok(Self::Web(addr));
        }

        // "daemon://addr:port"
        if let Some(addr_str) = trimmed.strip_prefix("daemon://") {
            let addr = parse_socket_addr(addr_str, "daemon")?;
            return Ok(Self::Daemon(addr));
        }

        // "all://web=addr1:port1,daemon=addr2:port2"
        if let Some(params) = trimmed.strip_prefix("all://") {
            let mut web_addr: Option<SocketAddr> = None;
            let mut daemon_addr: Option<SocketAddr> = None;

            for pair in params.split(',') {
                let pair = pair.trim();
                if let Some((key, value)) = pair.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    if key == "web" {
                        web_addr = Some(parse_socket_addr(value, "all web")?);
                    } else if key == "daemon" {
                        daemon_addr = Some(parse_socket_addr(value, "all daemon")?);
                    }
                }
            }

            match (web_addr, daemon_addr) {
                (Some(web_addr), Some(daemon_addr)) => {
                    let listen = Self::All {
                        web_addr,
                        daemon_addr,
                    };
                    listen.validate()?;
                    Ok(listen)
                }
                _ => anyhow::bail!(
                    "invalid --listen value `{input}`: all:// requires web=addr:port and daemon=addr:port"
                ),
            }
        } else {
            anyhow::bail!("invalid --listen value `{input}`")
        }
    }

    /// Validate non-syntax invariants for a parsed listen URL.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Self::All {
            web_addr,
            daemon_addr,
        } = self
        {
            validate_distinct_all_addrs(*web_addr, *daemon_addr)?;
        }
        Ok(())
    }
}

impl FromStr for ListenUrl {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

pub(crate) fn validate_distinct_all_addrs(
    web_addr: SocketAddr,
    daemon_addr: SocketAddr,
) -> anyhow::Result<()> {
    if web_addr == daemon_addr && web_addr.port() != 0 {
        anyhow::bail!("web and daemon listen addresses must be distinct in all mode: {web_addr}");
    }
    Ok(())
}

fn parse_socket_addr(value: &str, label: &str) -> anyhow::Result<SocketAddr> {
    value
        .parse()
        .map_err(|err| anyhow::anyhow!("invalid {label} listen address `{value}`: {err}"))
}

fn websocket_origin_is_allowed(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };

    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(ipv6) = authority.strip_prefix('[') {
        let Some((host, _)) = ipv6.split_once(']') else {
            return false;
        };
        host
    } else {
        authority.split(':').next().unwrap_or_default()
    };

    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    host.parse::<std::net::IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn v4(octets: [u8; 4], port: u16) -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3])),
            port,
        )
    }

    #[test]
    fn parse_web_url() {
        let result = ListenUrl::parse("web://127.0.0.1:17322");
        assert_eq!(result.unwrap(), ListenUrl::Web(v4([127, 0, 0, 1], 17322)));
    }

    #[test]
    fn parse_daemon_url() {
        let result = ListenUrl::parse("daemon://127.0.0.1:19836");
        assert_eq!(
            result.unwrap(),
            ListenUrl::Daemon(v4([127, 0, 0, 1], 19836))
        );
    }

    #[test]
    fn parse_all_url() {
        let result = ListenUrl::parse("all://web=127.0.0.1:17322,daemon=127.0.0.1:19836");
        assert_eq!(
            result.unwrap(),
            ListenUrl::All {
                web_addr: v4([127, 0, 0, 1], 17322),
                daemon_addr: v4([127, 0, 0, 1], 19836),
            }
        );
    }

    #[test]
    fn parse_all_rejects_duplicate_non_zero_addr() {
        let result = ListenUrl::parse("all://web=127.0.0.1:17322,daemon=127.0.0.1:17322");
        assert!(result.is_err());
    }

    #[test]
    fn parse_all_allows_duplicate_zero_addr() {
        let result = ListenUrl::parse("all://web=127.0.0.1:0,daemon=127.0.0.1:0");
        assert_eq!(
            result.unwrap(),
            ListenUrl::All {
                web_addr: v4([127, 0, 0, 1], 0),
                daemon_addr: v4([127, 0, 0, 1], 0),
            }
        );
    }

    #[test]
    fn parse_stdio() {
        assert_eq!(ListenUrl::parse("stdio://").unwrap(), ListenUrl::Stdio);
    }

    #[test]
    fn parse_off() {
        assert_eq!(ListenUrl::parse("off").unwrap(), ListenUrl::Off);
    }

    #[test]
    fn parse_off_case_insensitive() {
        assert_eq!(ListenUrl::parse("OFF").unwrap(), ListenUrl::Off);
        assert_eq!(ListenUrl::parse("Off").unwrap(), ListenUrl::Off);
    }

    #[test]
    fn parse_empty_string() {
        assert!(ListenUrl::parse("").is_err());
    }

    #[test]
    fn parse_invalid_protocol() {
        assert!(ListenUrl::parse("garbage://xyz").is_err());
    }

    #[test]
    fn parse_invalid_port() {
        assert!(ListenUrl::parse("web://127.0.0.1:99999").is_err());
    }

    #[test]
    fn parse_invalid_addr() {
        assert!(ListenUrl::parse("web://not-an-address").is_err());
    }

    #[test]
    fn parse_all_missing_key() {
        // Only "web" given, missing "daemon"
        assert!(ListenUrl::parse("all://web=127.0.0.1:17322").is_err());
    }

    #[test]
    fn connection_ids_are_unique_and_displayable() {
        let first = ConnectionId::next();
        let second = ConnectionId::next();

        assert_ne!(first, second);
        assert!(first.as_str().starts_with("connection-"));
        assert_eq!(first.to_string(), first.as_str());
    }

    #[test]
    fn connection_origin_allows_missing_and_loopback_origins() {
        assert_eq!(
            ConnectionOrigin::from_websocket_origin(None).unwrap(),
            ConnectionOrigin::LocalNative
        );
        assert_eq!(
            ConnectionOrigin::from_websocket_origin(Some("http://localhost:17322")).unwrap(),
            ConnectionOrigin::browser("http://localhost:17322")
        );
        assert!(ConnectionOrigin::from_websocket_origin(Some("https://127.0.0.1:17322")).is_ok());
        assert!(ConnectionOrigin::from_websocket_origin(Some("http://[::1]:17322")).is_ok());
    }

    #[test]
    fn connection_origin_rejects_non_loopback_browser_origins() {
        assert!(ConnectionOrigin::from_websocket_origin(Some("https://example.com")).is_err());
        assert!(ConnectionOrigin::from_websocket_origin(Some("file://local")).is_err());
        assert!(ConnectionOrigin::from_websocket_origin(Some("null")).is_err());
    }

    #[test]
    fn transport_event_and_outbound_envelope_are_plain_values() {
        let id = ConnectionId::from_static("test-connection");
        let opened: TransportEvent<&str> = TransportEvent::ConnectionOpened {
            connection_id: id.clone(),
            kind: TransportKind::HeadlessStdio,
            origin: ConnectionOrigin::local_native(),
        };
        let incoming = TransportEvent::IncomingMessage {
            connection_id: id.clone(),
            kind: TransportKind::HeadlessStdio,
            message: "hello",
        };
        let closed: TransportEvent<&str> = TransportEvent::ConnectionClosed {
            connection_id: id.clone(),
            kind: TransportKind::HeadlessStdio,
            reason: ConnectionClosedReason::StdinEof,
        };
        let outbound = OutboundEnvelope::new(id, "world");

        assert_eq!(opened.clone(), opened);
        assert!(format!("{incoming:?}").contains("IncomingMessage"));
        assert_eq!(closed.clone(), closed);
        assert_eq!(outbound.message, "world");
    }
}
