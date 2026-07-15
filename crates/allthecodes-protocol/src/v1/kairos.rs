//! KAIROS configuration and lifecycle API contracts.

use allthecodes_types::kairos::{
    KairosConfigScope, KairosControlResult, KairosFeatureProfilePatch, KairosRuntimeSnapshot,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KairosApplyMode {
    #[default]
    None,
    Reconcile,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosConfigUpdateRequest {
    pub profile: KairosFeatureProfilePatch,
    pub scope: KairosConfigScope,
    pub apply: KairosApplyMode,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosControlParameters {
    pub cwd: Option<String>,
    pub port: Option<u16>,
    pub readiness_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosResponse {
    pub snapshot: KairosRuntimeSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<KairosControlResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_patch_roundtrip_preserves_explicit_false() {
        let request = KairosConfigUpdateRequest {
            profile: KairosFeatureProfilePatch {
                brief: Some(false),
                ..Default::default()
            },
            scope: KairosConfigScope::Project,
            apply: KairosApplyMode::Reconcile,
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["profile"]["brief"], false);
        assert_eq!(
            serde_json::from_value::<KairosConfigUpdateRequest>(json).unwrap(),
            request
        );
    }

    #[test]
    fn control_parameters_allow_forward_compatible_unknown_fields() {
        let value = serde_json::json!({
            "port": 21001,
            "future_option": true
        });
        let request: KairosControlParameters = serde_json::from_value(value).unwrap();
        assert_eq!(request.port, Some(21001));
    }
}
