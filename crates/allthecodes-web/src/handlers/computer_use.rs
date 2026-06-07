//! Computer Use status and explicit action boundaries.

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use allthecodes_computer_use::host_adapter::{
    probe_capabilities, CapabilityLevel, OsType, PermissionState,
};

use crate::handlers::{setting_bool, ApiError};
use crate::state::WebState;

#[derive(Serialize)]
pub struct ComputerUseStatusResponse {
    pub enabled: bool,
    pub available: bool,
    pub status: String,
    pub diagnostics: Vec<String>,
}

/// GET /api/computer-use/status
pub async fn computer_use_status_handler(State(state): State<WebState>) -> impl IntoResponse {
    let caps = probe_capabilities().await;
    let enabled = setting_bool(&state, "computerUse.enabled")
        .or_else(|| setting_bool(&state, "computer_use.enabled"))
        .unwrap_or_else(|| {
            state
                .engine()
                .tool_names()
                .iter()
                .any(|name| name.starts_with("mcp__computer-use__"))
        });
    let available = caps.supported
        && (caps.screenshot != CapabilityLevel::None || caps.input != CapabilityLevel::None);
    let mut diagnostics = vec![
        format!("os={}", os_label(caps.os)),
        format!("os_version={}", caps.os_version),
        format!("screenshot={}", capability_label(caps.screenshot)),
        format!("input={}", capability_label(caps.input)),
        format!("app_launch={}", capability_label(caps.app_launch)),
        format!(
            "window_management={}",
            capability_label(caps.window_management)
        ),
        format!(
            "accessibility={}",
            permission_label(caps.accessibility_granted)
        ),
        format!(
            "screen_recording={}",
            permission_label(caps.screen_recording_granted)
        ),
        format!("display_count={}", caps.display_count),
        format!("dpi_scale={}", caps.dpi_scale),
    ];
    if let Some(desktop) = caps.desktop_environment {
        diagnostics.push(format!("desktop_environment={desktop}"));
    }
    if caps.is_wayland {
        diagnostics.push("wayland=true".to_string());
    }

    Json(ComputerUseStatusResponse {
        enabled,
        available,
        status: if available {
            "available".to_string()
        } else {
            "unsupported".to_string()
        },
        diagnostics,
    })
}

/// POST /api/computer-use/permissions/{permission}/request
pub async fn computer_use_permission_request_handler(
    AxumPath(permission): AxumPath<String>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiError {
            error: format!(
                "Computer Use permission request is not implemented by this backend: {}",
                permission
            ),
            code: "computer_use_permission_request_not_implemented".into(),

            details: serde_json::json!({}),
        }),
    )
}

/// POST /api/computer-use/test
pub async fn computer_use_test_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiError {
            error: "Computer Use web test is not implemented by this backend".into(),
            code: "computer_use_test_not_implemented".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn os_label(os: OsType) -> &'static str {
    match os {
        OsType::MacOS => "macos",
        OsType::Windows => "windows",
        OsType::Linux => "linux",
        OsType::Unknown => "unknown",
    }
}

fn capability_label(level: CapabilityLevel) -> &'static str {
    match level {
        CapabilityLevel::Full => "full",
        CapabilityLevel::Partial => "partial",
        CapabilityLevel::None => "none",
    }
}

fn permission_label(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::NotApplicable => "not_applicable",
        PermissionState::Unknown => "unknown",
    }
}
