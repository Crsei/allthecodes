#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use allthecodes_types::kairos::{KairosLifecycleTransition, KairosRuntimeSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KairosLifecycleChangedPayload {
    pub operation_id: String,
    pub transition: KairosLifecycleTransition,
    pub snapshot_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<KairosRuntimeSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", content = "payload")]
pub enum ServerNotification {
    KairosLifecycleChanged(KairosLifecycleChangedPayload),
}
