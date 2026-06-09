use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response returned by the web health check endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub db: String,
}
