use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{GatewayDiagnostic, GatewayError};

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn verify_hex_hmac(
    secret: &[u8],
    body: &[u8],
    hex_signature: &str,
) -> Result<(), GatewayError> {
    let expected = hmac_sha256(secret, body)?;
    let provided = hex::decode(hex_signature).map_err(|_| bad_hmac_error())?;
    if constant_time_eq(&provided, &expected) {
        Ok(())
    } else {
        Err(bad_hmac_error())
    }
}

fn hmac_sha256(secret: &[u8], body: &[u8]) -> Result<Vec<u8>, GatewayError> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| bad_hmac_error())?;
    mac.update(body);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

pub(crate) fn bad_hmac_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "bad_hmac",
        "Webhook signature could not be verified.",
        "Check the webhook secret and signature header.",
    ))
}
