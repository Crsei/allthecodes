//! Sidebar pin / reorder handlers.
//!
//! Manages pinned session order and custom session ordering, persisted via
//! the WebUiStore preferences.

use async_trait::async_trait;
use axum::extract::State;
use axum::response::Response;
use axum::routing::post;
use axum::Json;

use allthecodes_protocol::v1::sidebar::{
    SidebarPinRequest, SidebarPinResponse, SidebarReorderRequest, SidebarReorderResponse,
    SidebarUnpinRequest, SidebarUnpinResponse,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;
use allthecodes_web_state::normalize_owner;

const PREF_KEY_PINNED_IDS: &str = "sidebarPinnedSessionIds";
const PREF_KEY_SESSION_ORDER: &str = "sidebarSessionOrder";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn owner(profile_id: Option<&str>) -> String {
    normalize_owner(profile_id.unwrap_or_default())
}

async fn read_pinned_ids(
    store: &allthecodes_web_state::WebUiStore,
    owner_profile_id: &str,
) -> Vec<String> {
    let prefs = store
        .get_preferences(owner_profile_id)
        .await
        .unwrap_or_default();
    prefs
        .get(PREF_KEY_PINNED_IDS)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

async fn write_pinned_ids(
    store: &allthecodes_web_state::WebUiStore,
    owner_profile_id: &str,
    ids: &[String],
) -> Result<(), String> {
    let patch = serde_json::json!({ PREF_KEY_PINNED_IDS: ids });
    store
        .update_preferences(owner_profile_id, patch)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn write_session_order(
    store: &allthecodes_web_state::WebUiStore,
    owner_profile_id: &str,
    ids: &[String],
) -> Result<(), String> {
    let patch = serde_json::json!({ PREF_KEY_SESSION_ORDER: ids });
    store
        .update_preferences(owner_profile_id, patch)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SidebarPinProcessor {
    state: WebState,
}

impl From<WebState> for SidebarPinProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SidebarPinProcessor {
    type Request = SidebarPinRequest;
    type Response = SidebarPinResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "sidebar.pin"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, req: Self::Request) -> Result<Self::Response, Self::Error> {
        let owner = owner(req.profile_id.as_deref());
        let mut pinned = read_pinned_ids(&self.state.web_ui_store, &owner).await;
        if !pinned.contains(&req.session_id) {
            if let Some(pos) = req.position {
                let insert_at = pos.min(pinned.len());
                pinned.insert(insert_at, req.session_id);
            } else {
                pinned.push(req.session_id);
            }
            write_pinned_ids(&self.state.web_ui_store, &owner, &pinned)
                .await
                .map_err(|e| ProtocolApiError::Internal { message: e })?;
        }
        Ok(SidebarPinResponse {
            pinned_ids: pinned,
            profile_id: Some(owner),
        })
    }
}

#[derive(Clone)]
pub struct SidebarUnpinProcessor {
    state: WebState,
}

impl From<WebState> for SidebarUnpinProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SidebarUnpinProcessor {
    type Request = SidebarUnpinRequest;
    type Response = SidebarUnpinResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "sidebar.unpin"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, req: Self::Request) -> Result<Self::Response, Self::Error> {
        let owner = owner(req.profile_id.as_deref());
        let mut pinned = read_pinned_ids(&self.state.web_ui_store, &owner).await;
        pinned.retain(|id| id != &req.session_id);
        write_pinned_ids(&self.state.web_ui_store, &owner, &pinned)
            .await
            .map_err(|e| ProtocolApiError::Internal { message: e })?;
        Ok(SidebarUnpinResponse {
            pinned_ids: pinned,
            profile_id: Some(owner),
        })
    }
}

#[derive(Clone)]
pub struct SidebarReorderProcessor {
    state: WebState,
}

impl From<WebState> for SidebarReorderProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SidebarReorderProcessor {
    type Request = SidebarReorderRequest;
    type Response = SidebarReorderResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "sidebar.reorder"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, req: Self::Request) -> Result<Self::Response, Self::Error> {
        let owner = owner(req.profile_id.as_deref());
        write_pinned_ids(&self.state.web_ui_store, &owner, &req.pinned_ids)
            .await
            .map_err(|e| ProtocolApiError::Internal { message: e })?;
        write_session_order(&self.state.web_ui_store, &owner, &req.session_ids)
            .await
            .map_err(|e| ProtocolApiError::Internal { message: e })?;
        Ok(SidebarReorderResponse {
            pinned_ids: req.pinned_ids,
            session_ids: req.session_ids,
            profile_id: Some(owner),
        })
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge functions
// ---------------------------------------------------------------------------

async fn pin_handler(
    State(state): State<WebState>,
    Json(req): Json<SidebarPinRequest>,
) -> Response {
    rest_processor_response::<SidebarPinProcessor>(state, ApiMethod::SidebarPin, req).await
}

async fn unpin_handler(
    State(state): State<WebState>,
    Json(req): Json<SidebarUnpinRequest>,
) -> Response {
    rest_processor_response::<SidebarUnpinProcessor>(state, ApiMethod::SidebarUnpin, req).await
}

async fn reorder_handler(
    State(state): State<WebState>,
    Json(req): Json<SidebarReorderRequest>,
) -> Response {
    rest_processor_response::<SidebarReorderProcessor>(state, ApiMethod::SidebarReorder, req).await
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::SidebarPin, post(pin_handler))
        .handle(ApiMethod::SidebarUnpin, post(unpin_handler))
        .handle(ApiMethod::SidebarReorder, post(reorder_handler))
}
