//! ACP private extension capabilities.

use agent_client_protocol_schema::v2;

pub const STRUCTURED_BRIEF_CAPABILITY: &str = "allthecodes.structuredBrief";
pub const STRUCTURED_BRIEF_UPDATE: &str = "_allthecodes_structured_brief";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcpClientCapabilities {
    pub structured_brief: bool,
}

impl AcpClientCapabilities {
    pub fn from_initialize_request(req: &v2::InitializeRequest) -> Self {
        Self {
            structured_brief: has_true_flag(
                req.capabilities.meta.as_ref(),
                STRUCTURED_BRIEF_CAPABILITY,
            ),
        }
    }
}

pub fn agent_extension_meta() -> v2::Meta {
    let mut meta = v2::Meta::new();
    meta.insert(
        STRUCTURED_BRIEF_CAPABILITY.into(),
        serde_json::Value::Bool(true),
    );
    meta
}

fn has_true_flag(meta: Option<&v2::Meta>, key: &str) -> bool {
    meta.and_then(|meta| meta.get(key))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
