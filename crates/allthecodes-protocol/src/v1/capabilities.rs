use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response returned for capability discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityDiscoveryResponse {
    pub capabilities: HashMap<String, bool>,
}
