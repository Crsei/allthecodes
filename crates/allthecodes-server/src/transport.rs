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
}
