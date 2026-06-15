use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};

use crate::{GatewayDiagnostic, GatewayError};

pub const DAEMON_TOKEN_HEADER: &str = "x-allthecodes-daemon-token";
pub const REMOTE_TOKEN_HEADER: &str = "x-allthecodes-remote-token";
const LEGACY_DAEMON_TOKEN_HEADER: &str = "x-cc-rust-daemon-token";
const LEGACY_REMOTE_TOKEN_HEADER: &str = "x-cc-rust-remote-token";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAuthMode {
    LoopbackDaemonToken,
    RemoteToken,
}

pub trait GatewayAuthVerifier: Clone + Send + Sync + 'static {
    fn mode(&self) -> GatewayAuthMode;
    fn verify(&self, candidate: Option<&str>) -> Result<(), GatewayError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllGatewayAuth;

impl GatewayAuthVerifier for AllowAllGatewayAuth {
    fn mode(&self) -> GatewayAuthMode {
        GatewayAuthMode::LoopbackDaemonToken
    }

    fn verify(&self, _candidate: Option<&str>) -> Result<(), GatewayError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGatewayAuth {
    expected_token: String,
}

impl RemoteGatewayAuth {
    pub fn new(expected_token: impl Into<String>) -> Result<Self, GatewayError> {
        let expected_token = expected_token.into();
        if expected_token.trim().is_empty() {
            return Err(remote_token_required_error());
        }
        Ok(Self { expected_token })
    }
}

impl GatewayAuthVerifier for RemoteGatewayAuth {
    fn mode(&self) -> GatewayAuthMode {
        GatewayAuthMode::RemoteToken
    }

    fn verify(&self, candidate: Option<&str>) -> Result<(), GatewayError> {
        let Some(candidate) = candidate.filter(|value| !value.trim().is_empty()) else {
            return Err(missing_remote_token_error());
        };
        if constant_time_eq_digest(candidate, &self.expected_token) {
            Ok(())
        } else {
            Err(invalid_remote_token_error())
        }
    }
}

pub fn extract_gateway_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(REMOTE_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(LEGACY_REMOTE_TOKEN_HEADER)
                .and_then(|value| value.to_str().ok())
        })
        .or_else(|| {
            headers
                .get(DAEMON_TOKEN_HEADER)
                .and_then(|value| value.to_str().ok())
        })
        .or_else(|| {
            headers
                .get(LEGACY_DAEMON_TOKEN_HEADER)
                .and_then(|value| value.to_str().ok())
        })
        .or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
}

pub fn extract_origin(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
}

pub fn validate_origin(
    headers: &HeaderMap,
    allowed_origins: &[String],
) -> Result<(), GatewayError> {
    let Some(origin) = extract_origin(headers) else {
        return Ok(());
    };
    if allowed_origins.iter().any(|allowed| allowed == origin) {
        Ok(())
    } else {
        Err(GatewayError::new(GatewayDiagnostic::new(
            "bad_origin",
            "The remote-control gateway request origin is not allowed.",
            "Use an allowed browser origin or remove browser-origin credentials from the request.",
        )))
    }
}

pub fn validate_bind_requires_remote_token(
    bind_addr: SocketAddr,
    remote_token: Option<&str>,
) -> Result<(), GatewayError> {
    if is_loopback_ip(bind_addr.ip()) {
        return Ok(());
    }
    if remote_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        Ok(())
    } else {
        Err(remote_token_required_error())
    }
}

pub fn missing_token_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "missing_control_token",
        "The remote-control gateway request is missing a daemon control token.",
        "Send x-allthecodes-daemon-token or Authorization: Bearer with a valid local daemon token.",
    ))
}

pub fn invalid_token_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "invalid_control_token",
        "The remote-control gateway request has an invalid daemon control token.",
        "Refresh the daemon status and retry with the current local control token.",
    ))
}

pub fn missing_remote_token_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "missing_remote_token",
        "The remote-control gateway request is missing a remote token.",
        "Send x-allthecodes-remote-token or Authorization: Bearer with the configured remote token.",
    ))
}

pub fn invalid_remote_token_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "invalid_remote_token",
        "The remote-control gateway request has an invalid remote token.",
        "Rotate or refresh the configured remote token and retry.",
    ))
}

pub fn remote_token_required_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "remote_token_required",
        "Non-loopback remote-control gateway bindings require a remote token.",
        "Configure a remote token before binding the gateway to a non-loopback address.",
    ))
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}

fn constant_time_eq_digest(left: &str, right: &str) -> bool {
    let left_hash = Sha256::digest(left.as_bytes());
    let right_hash = Sha256::digest(right.as_bytes());
    left_hash
        .iter()
        .zip(right_hash.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn extracts_remote_header_before_daemon_and_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(REMOTE_TOKEN_HEADER, "remote-token".parse().unwrap());
        headers.insert(DAEMON_TOKEN_HEADER, "daemon-token".parse().unwrap());
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer bearer-token".parse().unwrap(),
        );

        assert_eq!(extract_gateway_token(&headers), Some("remote-token"));
    }

    #[test]
    fn extracts_daemon_header_before_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(DAEMON_TOKEN_HEADER, "daemon-token".parse().unwrap());
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer bearer-token".parse().unwrap(),
        );

        assert_eq!(extract_gateway_token(&headers), Some("daemon-token"));
    }

    #[test]
    fn extracts_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer bearer-token".parse().unwrap(),
        );

        assert_eq!(extract_gateway_token(&headers), Some("bearer-token"));
    }

    #[test]
    fn auth_errors_have_stable_codes() {
        assert_eq!(
            missing_token_error().diagnostic().code,
            "missing_control_token"
        );
        assert_eq!(
            invalid_token_error().diagnostic().code,
            "invalid_control_token"
        );
    }

    #[test]
    fn remote_auth_requires_and_validates_token_without_echoing_it() {
        let auth = RemoteGatewayAuth::new("raw-remote-token").unwrap();
        assert!(auth.verify(Some("raw-remote-token")).is_ok());
        let err = auth.verify(Some("wrong-token")).unwrap_err();

        assert_eq!(err.diagnostic().code, "invalid_remote_token");
        assert!(!serde_json::to_string(err.diagnostic())
            .unwrap()
            .contains("wrong-token"));
    }

    #[test]
    fn non_loopback_bind_requires_remote_token() {
        let err = validate_bind_requires_remote_token("0.0.0.0:21990".parse().unwrap(), None)
            .unwrap_err();

        assert_eq!(err.diagnostic().code, "remote_token_required");
        assert!(
            validate_bind_requires_remote_token("127.0.0.1:21990".parse().unwrap(), None,).is_ok()
        );
        assert!(validate_bind_requires_remote_token(
            "0.0.0.0:21990".parse().unwrap(),
            Some("configured-token"),
        )
        .is_ok());
    }

    #[test]
    fn origin_policy_rejects_unlisted_browser_origins() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ORIGIN,
            "https://evil.example".parse().unwrap(),
        );

        let err = validate_origin(&headers, &["https://trusted.example".to_string()]).unwrap_err();

        assert_eq!(err.diagnostic().code, "bad_origin");
    }
}
