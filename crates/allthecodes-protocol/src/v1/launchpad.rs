use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LaunchpadSnapshotCreateRequest {
    pub snapshot: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LaunchpadSnapshotCreateResponse {
    pub path: String,
    pub file_name: String,
}
