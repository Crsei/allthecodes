// test infrastructure — upstream UI capability contract not wired to event dispatch yet
//! Rust-side UI capability contract.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BackendEventKind {
    AssistantDelta,
    ToolProgress,
    PermissionRequest,
    SubsystemStatus,
    AgentUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendCommandKind {
    SubmitPrompt,
    Abort,
    ApprovalResponse,
    OpenPanel,
    SelectItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSurface {
    Transcript,
    BottomPane,
    ApprovalOverlay,
    StatusWidget,
    FeaturePanel,
    SelectionSurface,
    ToolActivity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityMapping {
    pub event: BackendEventKind,
    pub command: Option<FrontendCommandKind>,
    pub surface: UiSurface,
    pub guarantee: &'static str,
}

pub fn default_contract() -> Vec<CapabilityMapping> {
    vec![
        CapabilityMapping {
            event: BackendEventKind::AssistantDelta,
            command: Some(FrontendCommandKind::SubmitPrompt),
            surface: UiSurface::Transcript,
            guarantee: "streamed text is visible and replayable",
        },
        CapabilityMapping {
            event: BackendEventKind::ToolProgress,
            command: Some(FrontendCommandKind::Abort),
            surface: UiSurface::ToolActivity,
            guarantee: "running tools expose compact and detailed status",
        },
        CapabilityMapping {
            event: BackendEventKind::PermissionRequest,
            command: Some(FrontendCommandKind::ApprovalResponse),
            surface: UiSurface::ApprovalOverlay,
            guarantee: "missing response defaults to deny",
        },
        CapabilityMapping {
            event: BackendEventKind::SubsystemStatus,
            command: Some(FrontendCommandKind::OpenPanel),
            surface: UiSurface::FeaturePanel,
            guarantee: "subsystem health has a visible entry point",
        },
        CapabilityMapping {
            event: BackendEventKind::AgentUpdate,
            command: Some(FrontendCommandKind::SelectItem),
            surface: UiSurface::SelectionSurface,
            guarantee: "agent choices remain keyboard reachable",
        },
    ]
}

pub fn surface_for_event(event: BackendEventKind) -> UiSurface {
    default_contract()
        .into_iter()
        .find(|mapping| mapping.event == event)
        .map(|mapping| mapping.surface)
        .unwrap_or(UiSurface::StatusWidget)
}

pub fn render_contract_table() -> String {
    let mut rows = vec!["event | command | surface | guarantee".to_string()];
    for mapping in default_contract() {
        rows.push(format!(
            "{:?} | {} | {:?} | {}",
            mapping.event,
            mapping
                .command
                .map(|command| format!("{command:?}"))
                .unwrap_or_else(|| "-".to_string()),
            mapping.surface,
            mapping.guarantee
        ));
    }
    rows.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_can_reference_bottom_pane_surface() {
        let mapping = CapabilityMapping {
            event: BackendEventKind::SubsystemStatus,
            command: None,
            surface: UiSurface::BottomPane,
            guarantee: "status summary is visible",
        };
        assert_eq!(mapping.surface, UiSurface::BottomPane);
    }
}
