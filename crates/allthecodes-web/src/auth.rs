use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use sha2::{Digest, Sha256};

pub const CONTROL_TOKEN_HEADER: HeaderName = HeaderName::from_static("x-allthecodes-control-token");
pub const PRIVILEGED_TOKEN_HEADER: HeaderName =
    HeaderName::from_static("x-allthecodes-web-privileged-token");

pub fn authorize(
    headers: &HeaderMap,
    expected_control_token: Option<&str>,
    expected_privileged_token: Option<&str>,
    listener_authority: Option<&str>,
    privileged: bool,
) -> Result<Option<String>, StatusCode> {
    let supplied = headers
        .get(&CONTROL_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });
    match (supplied, expected_control_token) {
        (Some(supplied), Some(expected)) if constant_time_eq(supplied, expected) => {}
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    let origin = validate_listener(headers, listener_authority, false)?;
    if privileged {
        let supplied = headers
            .get(&PRIVILEGED_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok());
        match (supplied, expected_privileged_token) {
            (Some(supplied), Some(expected)) if constant_time_eq(supplied, expected) => {}
            _ => return Err(StatusCode::FORBIDDEN),
        }
    }

    Ok(origin)
}

pub fn authorize_preflight(
    headers: &HeaderMap,
    listener_authority: Option<&str>,
) -> Result<String, StatusCode> {
    validate_listener(headers, listener_authority, true)?.ok_or(StatusCode::FORBIDDEN)
}

fn validate_listener(
    headers: &HeaderMap,
    listener_authority: Option<&str>,
    origin_required: bool,
) -> Result<Option<String>, StatusCode> {
    let listener_authority = listener_authority.ok_or(StatusCode::FORBIDDEN)?;
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;
    if !constant_time_eq(host, listener_authority) {
        return Err(StatusCode::FORBIDDEN);
    }

    let Some(origin) = headers.get(header::ORIGIN) else {
        return if origin_required {
            Err(StatusCode::FORBIDDEN)
        } else {
            Ok(None)
        };
    };
    let origin = origin.to_str().map_err(|_| StatusCode::FORBIDDEN)?;
    let expected_origin = format!("http://{listener_authority}");
    if !constant_time_eq(origin, &expected_origin) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(origin.to_owned()))
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = Sha256::digest(left.as_bytes());
    let right = Sha256::digest(right.as_bytes());
    let mut different = 0_u8;
    for (left, right) in left.iter().zip(right.iter()) {
        different |= left ^ right;
    }
    different == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_handles_equal_different_and_different_length_values() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "secreu"));
        assert!(!constant_time_eq("secret", "secret-longer"));
    }

    #[test]
    fn explicit_control_header_is_accepted() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:17322".parse().unwrap());
        headers.insert(CONTROL_TOKEN_HEADER, "secret".parse().unwrap());

        assert_eq!(
            authorize(
                &headers,
                Some("secret"),
                None,
                Some("127.0.0.1:17322"),
                false,
            ),
            Ok(None)
        );
    }
}
