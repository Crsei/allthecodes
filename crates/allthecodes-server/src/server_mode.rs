//! [`ServerMode`] — describes which Axum servers run in this process.
//!
//! Constructed from the optional `--listen` value and the legacy CLI flags
//! (`--web`, `--daemon`, `--web-port`, `--port`) via
//! [`ServerMode::from_listen_and_fallback`].

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::transport::{validate_distinct_all_addrs, ListenUrl};

/// Describes which server(s) run in this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMode {
    /// Web UI server only.
    Web { addr: SocketAddr },
    /// Daemon API server only.
    Daemon { addr: SocketAddr },
    /// Both servers in the same process.
    All {
        web_addr: SocketAddr,
        daemon_addr: SocketAddr,
    },
    /// No HTTP server (TUI or headless mode).
    None,
}

impl ServerMode {
    /// Derive a [`ServerMode`] from the optional `--listen` value and legacy
    /// CLI flags.
    ///
    /// # Priority
    ///
    /// 1. If `listen` is `Some`, it is mapped directly (ignoring all flags).
    /// 2. Otherwise, `--web` / `--daemon` flags are used with their default
    ///    ports.
    /// 3. If neither flag is set, `None` is returned.
    pub fn from_listen_and_fallback(
        listen: Option<ListenUrl>,
        web_flag: bool,
        daemon_flag: bool,
        web_port: u16,
        daemon_port: u16,
    ) -> anyhow::Result<Self> {
        let mode = match listen {
            Some(ListenUrl::Web(addr)) => Self::Web { addr },
            Some(ListenUrl::Daemon(addr)) => Self::Daemon { addr },
            Some(ListenUrl::All {
                web_addr,
                daemon_addr,
            }) => Self::All {
                web_addr,
                daemon_addr,
            },
            Some(ListenUrl::Stdio) | Some(ListenUrl::Off) => Self::None,
            None => match (web_flag, daemon_flag) {
                (true, false) => Self::Web {
                    addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), web_port),
                },
                (false, true) => Self::Daemon {
                    addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), daemon_port),
                },
                (true, true) => Self::All {
                    web_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), web_port),
                    daemon_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), daemon_port),
                },
                (false, false) => Self::None,
            },
        };
        mode.validate()?;
        Ok(mode)
    }

    /// Returns `true` when at least one HTTP server is active.
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Validate non-syntax invariants for direct crate users.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn localhost(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    #[test]
    fn listen_web_overrides_all_flags() {
        let listen = Some(ListenUrl::Web(localhost(9999)));
        let mode = ServerMode::from_listen_and_fallback(listen, true, true, 17322, 19836).unwrap();
        assert_eq!(
            mode,
            ServerMode::Web {
                addr: localhost(9999)
            }
        );
    }

    #[test]
    fn listen_daemon_overrides_all_flags() {
        let listen = Some(ListenUrl::Daemon(localhost(9999)));
        let mode = ServerMode::from_listen_and_fallback(listen, true, true, 17322, 19836).unwrap();
        assert_eq!(
            mode,
            ServerMode::Daemon {
                addr: localhost(9999)
            }
        );
    }

    #[test]
    fn listen_all_overrides_all_flags() {
        let listen = Some(ListenUrl::All {
            web_addr: localhost(2000),
            daemon_addr: localhost(3000),
        });
        let mode = ServerMode::from_listen_and_fallback(listen, true, true, 17322, 19836).unwrap();
        assert_eq!(
            mode,
            ServerMode::All {
                web_addr: localhost(2000),
                daemon_addr: localhost(3000),
            }
        );
    }

    #[test]
    fn listen_stdio_yields_none() {
        let listen = Some(ListenUrl::Stdio);
        let mode = ServerMode::from_listen_and_fallback(listen, true, true, 17322, 19836).unwrap();
        assert_eq!(mode, ServerMode::None);
    }

    #[test]
    fn listen_off_yields_none() {
        let listen = Some(ListenUrl::Off);
        let mode = ServerMode::from_listen_and_fallback(listen, true, true, 17322, 19836).unwrap();
        assert_eq!(mode, ServerMode::None);
    }

    #[test]
    fn web_flag_fallback() {
        let mode = ServerMode::from_listen_and_fallback(None, true, false, 17322, 19836).unwrap();
        assert_eq!(
            mode,
            ServerMode::Web {
                addr: localhost(17322)
            }
        );
    }

    #[test]
    fn daemon_flag_fallback() {
        let mode = ServerMode::from_listen_and_fallback(None, false, true, 17322, 19836).unwrap();
        assert_eq!(
            mode,
            ServerMode::Daemon {
                addr: localhost(19836)
            }
        );
    }

    #[test]
    fn both_flags_fallback() {
        let mode = ServerMode::from_listen_and_fallback(None, true, true, 17322, 19836).unwrap();
        assert_eq!(
            mode,
            ServerMode::All {
                web_addr: localhost(17322),
                daemon_addr: localhost(19836),
            }
        );
    }

    #[test]
    fn neither_flag_yields_none() {
        let mode = ServerMode::from_listen_and_fallback(None, false, false, 17322, 19836).unwrap();
        assert_eq!(mode, ServerMode::None);
    }

    #[test]
    fn all_mode_rejects_duplicate_non_zero_addr() {
        let result = ServerMode::from_listen_and_fallback(
            Some(ListenUrl::All {
                web_addr: localhost(17322),
                daemon_addr: localhost(17322),
            }),
            false,
            false,
            17322,
            19836,
        );
        assert!(result.is_err());
    }

    #[test]
    fn fallback_all_rejects_duplicate_non_zero_addr() {
        let result = ServerMode::from_listen_and_fallback(None, true, true, 17322, 17322);
        assert!(result.is_err());
    }

    #[test]
    fn all_mode_allows_duplicate_zero_addr() {
        let mode = ServerMode::from_listen_and_fallback(
            Some(ListenUrl::All {
                web_addr: localhost(0),
                daemon_addr: localhost(0),
            }),
            false,
            false,
            17322,
            19836,
        )
        .unwrap();
        assert_eq!(
            mode,
            ServerMode::All {
                web_addr: localhost(0),
                daemon_addr: localhost(0),
            }
        );
    }

    #[test]
    fn is_active_web() {
        assert!(ServerMode::Web {
            addr: localhost(8080)
        }
        .is_active());
    }

    #[test]
    fn is_active_daemon() {
        assert!(ServerMode::Daemon {
            addr: localhost(8080)
        }
        .is_active());
    }

    #[test]
    fn is_active_all() {
        assert!(ServerMode::All {
            web_addr: localhost(8080),
            daemon_addr: localhost(8081),
        }
        .is_active());
    }

    #[test]
    fn is_active_none() {
        assert!(!ServerMode::None.is_active());
    }
}
