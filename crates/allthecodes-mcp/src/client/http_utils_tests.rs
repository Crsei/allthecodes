use super::*;
use reqwest::StatusCode;
use serial_test::serial;

fn test_http_config() -> McpServerConfig {
    McpServerConfig {
        name: "test".to_string(),
        transport: "streamable-http".to_string(),
        command: None,
        args: None,
        url: Some("https://example.com/mcp".to_string()),
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
        bearer_token_env_var: None,
        env_http_headers: None,
        auth: None,
    }
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[test]
fn parses_bearer_insufficient_scope() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="insufficient_scope", scope="tools:read tools:write""#
            .parse()
            .unwrap(),
    );
    let challenge = www_authenticate_challenge(&map, StatusCode::FORBIDDEN).unwrap();
    assert!(challenge.insufficient_scope);
    assert_eq!(
        challenge.required_scope.as_deref(),
        Some("tools:read tools:write")
    );
}

#[test]
fn ignores_non_insufficient_scope_bearer_error() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="invalid_token", scope="tools:read""#
            .parse()
            .unwrap(),
    );
    let challenge = www_authenticate_challenge(&map, StatusCode::UNAUTHORIZED).unwrap();
    assert!(!challenge.insufficient_scope);
    assert_eq!(challenge.required_scope, None);
}

#[test]
fn not_401_403_returns_none() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="insufficient_scope""#.parse().unwrap(),
    );
    assert!(www_authenticate_challenge(&map, StatusCode::OK).is_none());
    assert!(www_authenticate_challenge(&map, StatusCode::NOT_FOUND).is_none());
    assert!(www_authenticate_challenge(&map, StatusCode::INTERNAL_SERVER_ERROR).is_none());
}

#[test]
fn parse_bearer_insufficient_scope_no_scope_param() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="insufficient_scope""#.parse().unwrap(),
    );
    let c = www_authenticate_challenge(&map, StatusCode::FORBIDDEN).unwrap();
    assert!(c.insufficient_scope);
    assert_eq!(c.required_scope, None);
}

#[test]
fn parse_bearer_invalid_token() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="invalid_token", error_description="token expired""#
            .parse()
            .unwrap(),
    );
    let c = www_authenticate_challenge(&map, StatusCode::UNAUTHORIZED).unwrap();
    assert!(!c.insufficient_scope);
    assert_eq!(c.required_scope, None);
}

#[test]
fn parse_not_bearer() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Basic realm="test""#.parse().unwrap(),
    );
    let c = www_authenticate_challenge(&map, StatusCode::UNAUTHORIZED).unwrap();
    assert!(!c.insufficient_scope);
    assert_eq!(c.header, r#"Basic realm="test""#);
}

#[test]
fn parse_multi_challenge() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="insufficient_scope", Basic realm="fallback""#
            .parse()
            .unwrap(),
    );
    let c = www_authenticate_challenge(&map, StatusCode::FORBIDDEN).unwrap();
    assert!(c.insufficient_scope);
    assert_eq!(c.required_scope, None);
}

#[test]
fn parse_bearer_first_then_other() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Basic realm="test", Bearer error="insufficient_scope", scope="read""#
            .parse()
            .unwrap(),
    );
    let c = www_authenticate_challenge(&map, StatusCode::FORBIDDEN).unwrap();
    assert!(c.insufficient_scope);
    assert_eq!(c.required_scope.as_deref(), Some("read"));
}

#[test]
fn extracts_from_headermap() {
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::WWW_AUTHENTICATE,
        r#"Bearer error="insufficient_scope""#.parse().unwrap(),
    );
    let c = www_authenticate_challenge(&map, StatusCode::FORBIDDEN).unwrap();
    assert!(c.insufficient_scope);
    assert_eq!(c.required_scope, None);
}

#[test]
fn missing_header_returns_none() {
    let map = HeaderMap::new();
    assert!(www_authenticate_challenge(&map, StatusCode::UNAUTHORIZED).is_none());
}

#[test]
#[serial]
fn env_http_headers_override_static_case_insensitively() {
    let key = "ATC_TEST_MCP_HEADER";
    let old = std::env::var(key).ok();
    std::env::set_var(key, "env-value");
    let mut headers = std::collections::HashMap::new();
    headers.insert("X-Test".to_string(), "static-value".to_string());
    let mut env_headers = std::collections::HashMap::new();
    env_headers.insert("x-test".to_string(), key.to_string());
    let config = McpServerConfig {
        name: "test".to_string(),
        transport: "streamable-http".to_string(),
        command: None,
        args: None,
        url: Some("https://example.com/mcp".to_string()),
        headers: Some(headers),
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
        bearer_token_env_var: None,
        env_http_headers: Some(env_headers),
        auth: None,
    };

    let normalized = normalized_http_transport_headers(&config);
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].0, "X-Test");
    assert_eq!(normalized[0].1, "env-value");

    match old {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[test]
#[serial]
fn env_http_headers_skips_reserved_transport_headers() {
    let key = "ATC_TEST_MCP_RESERVED_HEADER";
    let old = std::env::var(key).ok();
    std::env::set_var(key, "should-not-appear");
    let mut env_headers = std::collections::HashMap::new();
    env_headers.insert("content-type".to_string(), key.to_string());
    let config = McpServerConfig {
        name: "test".to_string(),
        transport: "streamable-http".to_string(),
        command: None,
        args: None,
        url: Some("https://example.com/mcp".to_string()),
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
        bearer_token_env_var: None,
        env_http_headers: Some(env_headers),
        auth: None,
    };

    let normalized = normalized_http_transport_headers(&config);
    assert!(normalized.is_empty(), "reserved header should be skipped");

    match old {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[tokio::test]
#[serial]
async fn bearer_token_env_var_overrides_static_authorization_case_insensitively() {
    let key = "ATC_TEST_MCP_BEARER_TOKEN";
    let old = std::env::var(key).ok();
    std::env::set_var(key, "env-token");

    let mut headers = std::collections::HashMap::new();
    headers.insert(
        "authorization".to_string(),
        "Bearer static-token".to_string(),
    );
    let mut config = test_http_config();
    config.headers = Some(headers);
    config.bearer_token_env_var = Some(key.to_string());

    let normalized = normalized_http_transport_headers_with_auth(&config)
        .await
        .unwrap();
    assert_eq!(
        header_value(&normalized, "Authorization"),
        Some("Bearer env-token")
    );
    assert_eq!(
        normalized
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .count(),
        1
    );

    match old {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[tokio::test]
#[serial]
async fn bearer_token_env_var_missing_errors() {
    let key = "ATC_TEST_MCP_MISSING_BEARER_TOKEN";
    let old = std::env::var(key).ok();
    std::env::remove_var(key);

    let mut config = test_http_config();
    config.bearer_token_env_var = Some(key.to_string());

    let err = normalized_http_transport_headers_with_auth(&config)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("is not set"));

    match old {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[tokio::test]
#[serial]
async fn bearer_token_env_var_empty_errors() {
    let key = "ATC_TEST_MCP_EMPTY_BEARER_TOKEN";
    let old = std::env::var(key).ok();
    std::env::set_var(key, "   ");

    let mut config = test_http_config();
    config.bearer_token_env_var = Some(key.to_string());

    let err = normalized_http_transport_headers_with_auth(&config)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("is empty"));

    match old {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn bearer_token_env_var_non_unicode_errors() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let key = "ATC_TEST_MCP_NON_UNICODE_BEARER_TOKEN";
    let old = std::env::var_os(key);
    std::env::set_var(
        key,
        OsString::from_vec(vec![0xff, b't', b'o', b'k', b'e', b'n']),
    );

    let mut config = test_http_config();
    config.bearer_token_env_var = Some(key.to_string());

    let err = normalized_http_transport_headers_with_auth(&config)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("contains invalid Unicode"));

    match old {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}
