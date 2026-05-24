//! AWS Signature Version 4 (SigV4) request signing 鈥?minimal implementation
//! for Bedrock `/invoke` requests.
//!
//! Only implements what's needed to sign a single POST request with a JSON
//! body. Does not cover query strings, multi-part, chunked streaming, or
//! presigned URLs.
//!
//! Reference: https://docs.aws.amazon.com/general/latest/gr/sigv4_signing.html

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// AWS credentials used for SigV4 signing.
#[derive(Debug, Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl AwsCredentials {
    /// Build credentials from standard environment variables:
    /// `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`.
    pub fn from_env() -> Option<Self> {
        let access_key_id = std::env::var("AWS_ACCESS_KEY_ID").ok()?;
        let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok()?;
        if access_key_id.is_empty() || secret_access_key.is_empty() {
            return None;
        }
        let session_token = std::env::var("AWS_SESSION_TOKEN")
            .ok()
            .filter(|v| !v.is_empty());
        Some(Self {
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

/// Inputs to a SigV4 POST request signature.
pub struct SignRequest<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub body: &'a [u8],
    pub content_type: &'a str,
    /// `YYYYMMDD'T'HHMMSS'Z'` timestamp, e.g. `20240115T143000Z`.
    pub amz_date: String,
    /// `YYYYMMDD`, e.g. `20240115`.
    pub date_stamp: String,
}

/// Headers computed by `sign()` that the caller must set on the outgoing request.
#[derive(Debug, Clone)]
pub struct SignedHeaders {
    pub authorization: String,
    pub x_amz_date: String,
    pub x_amz_content_sha256: String,
    pub x_amz_security_token: Option<String>,
}

/// Compute SigV4 signature for a POST JSON request.
pub fn sign(req: &SignRequest, creds: &AwsCredentials) -> Result<SignedHeaders> {
    let payload_hash = hex::encode(Sha256::digest(req.body));

    // Canonical headers 鈥?must be sorted by lowercased name.
    // We include: content-type, host, x-amz-content-sha256, x-amz-date,
    // and optionally x-amz-security-token.
    let mut canonical_headers = format!(
        "content-type:{}\nhost:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
        req.content_type, req.host, payload_hash, req.amz_date,
    );
    let mut signed_header_names = String::from("content-type;host;x-amz-content-sha256;x-amz-date");
    if let Some(tok) = &creds.session_token {
        canonical_headers.push_str(&format!("x-amz-security-token:{}\n", tok));
        signed_header_names.push_str(";x-amz-security-token");
    }

    let canonical_request = format!(
        "{method}\n{path}\n{query}\n{headers}\n{signed}\n{payload}",
        method = req.method,
        path = req.path,
        query = "", // no query string
        headers = canonical_headers,
        signed = signed_header_names,
        payload = payload_hash,
    );

    let canonical_request_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    let credential_scope = format!(
        "{date}/{region}/{service}/aws4_request",
        date = req.date_stamp,
        region = req.region,
        service = req.service,
    );

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{hash}",
        date = req.amz_date,
        scope = credential_scope,
        hash = canonical_request_hash,
    );

    let signing_key = derive_signing_key(
        &creds.secret_access_key,
        &req.date_stamp,
        req.region,
        req.service,
    )
    .context("failed to derive SigV4 signing key")?;

    let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes())?);

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={ak}/{scope}, SignedHeaders={signed}, Signature={sig}",
        ak = creds.access_key_id,
        scope = credential_scope,
        signed = signed_header_names,
        sig = signature,
    );

    Ok(SignedHeaders {
        authorization,
        x_amz_date: req.amz_date.clone(),
        x_amz_content_sha256: payload_hash,
        x_amz_security_token: creds.session_token.clone(),
    })
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac =
        HmacSha256::new_from_slice(key).context("invalid HMAC-SHA256 key (wrong length)")?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn derive_signing_key(
    secret: &str,
    date_stamp: &str,
    region: &str,
    service: &str,
) -> Result<Vec<u8>> {
    let k_date = hmac_sha256(format!("AWS4{}", secret).as_bytes(), date_stamp.as_bytes())?;
    let k_region = hmac_sha256(&k_date, region.as_bytes())?;
    let k_service = hmac_sha256(&k_region, service.as_bytes())?;
    hmac_sha256(&k_service, b"aws4_request")
}

/// Format the current UTC time as SigV4 `amz_date` (`YYYYMMDD'T'HHMMSS'Z'`)
/// and `date_stamp` (`YYYYMMDD`).
pub fn current_timestamps() -> (String, String) {
    let now = chrono::Utc::now();
    (
        now.format("%Y%m%dT%H%M%SZ").to_string(),
        now.format("%Y%m%d").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference: AWS SigV4 test suite "get-vanilla"
    // Simplified here 鈥?we only validate our implementation is consistent.
    #[test]
    fn signing_key_derivation_is_stable() {
        let k1 = derive_signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830",
            "us-east-1",
            "iam",
        )
        .unwrap();
        let k2 = derive_signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830",
            "us-east-1",
            "iam",
        )
        .unwrap();
        assert_eq!(k1, k2, "signing key derivation must be deterministic");
        // Match AWS docs expected value for these inputs.
        assert_eq!(
            hex::encode(&k1),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }

    #[test]
    fn sign_produces_deterministic_authorization() {
        let creds = AwsCredentials {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            session_token: None,
        };
        let req = SignRequest {
            method: "POST",
            host: "bedrock-runtime.us-east-1.amazonaws.com",
            path: "/model/foo/invoke",
            region: "us-east-1",
            service: "bedrock",
            body: b"{}",
            content_type: "application/json",
            amz_date: "20240115T143000Z".to_string(),
            date_stamp: "20240115".to_string(),
        };
        let signed1 = sign(&req, &creds).unwrap();
        let signed2 = sign(&req, &creds).unwrap();
        assert_eq!(signed1.authorization, signed2.authorization);
        assert!(signed1
            .authorization
            .starts_with("AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/"));
        assert!(signed1
            .authorization
            .contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn sign_with_session_token_adds_security_token() {
        let creds = AwsCredentials {
            access_key_id: "AKIA".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: Some("tok".to_string()),
        };
        let req = SignRequest {
            method: "POST",
            host: "h",
            path: "/p",
            region: "r",
            service: "s",
            body: b"",
            content_type: "application/json",
            amz_date: "20240115T000000Z".to_string(),
            date_stamp: "20240115".to_string(),
        };
        let signed = sign(&req, &creds).unwrap();
        assert!(signed.authorization.contains(
            "SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token"
        ));
        assert_eq!(signed.x_amz_security_token.as_deref(), Some("tok"));
    }

    #[test]
    fn credentials_from_env_missing_returns_none() {
        // Make sure helper returns None when no creds set.
        // Save and clear relevant env vars for this test.
        let saved_ak = std::env::var("AWS_ACCESS_KEY_ID").ok();
        let saved_sk = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");

        assert!(AwsCredentials::from_env().is_none());

        if let Some(v) = saved_ak {
            std::env::set_var("AWS_ACCESS_KEY_ID", v);
        }
        if let Some(v) = saved_sk {
            std::env::set_var("AWS_SECRET_ACCESS_KEY", v);
        }
    }
}
