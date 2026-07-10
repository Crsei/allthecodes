use axum::http::{header, HeaderMap, HeaderName, StatusCode, Uri};
use sha2::{Digest, Sha256};

pub const CONTROL_TOKEN_HEADER: HeaderName = HeaderName::from_static("x-allthecodes-control-token");

pub fn authorize(
    headers: &HeaderMap,
    expected_token: Option<&str>,
    token_required: bool,
) -> Result<Option<String>, StatusCode> {
    let origin = validate_same_origin(headers)?;
    if !token_required {
        return Ok(origin);
    }

    let supplied = headers
        .get(&CONTROL_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });
    match (supplied, expected_token) {
        (Some(supplied), Some(expected)) if constant_time_eq(supplied, expected) => Ok(origin),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

fn validate_same_origin(headers: &HeaderMap) -> Result<Option<String>, StatusCode> {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(None);
    };
    let origin = origin.to_str().map_err(|_| StatusCode::FORBIDDEN)?;
    let uri: Uri = origin.parse().map_err(|_| StatusCode::FORBIDDEN)?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(StatusCode::FORBIDDEN);
    }
    let origin_authority = uri.authority().ok_or(StatusCode::FORBIDDEN)?.as_str();
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;
    if !constant_time_eq(origin_authority, host) {
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

        assert_eq!(authorize(&headers, Some("secret"), true), Ok(None));
    }
}
