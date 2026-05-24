// test infrastructure — event router not wired to TUI event dispatch yet
//! Small event router used to separate UI events from app actions.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Key(String),
    Paste(String),
    Resize { width: u16, height: u16 },
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    AssistantDelta(String),
    ToolStarted(String),
    PermissionRequested(String),
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    Terminal(TerminalEvent),
    Engine(EngineEvent),
    CommandResult(String),
    OverlayResponse(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAction {
    SubmitInput,
    QueueInput,
    AppendTranscript,
    UpdateToolActivity,
    ShowApproval,
    CloseOverlay,
    Redraw,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RouteContext {
    pub busy: bool,
    pub overlay_active: bool,
}

pub fn route_event(event: &UiEvent, ctx: RouteContext) -> Vec<RouteAction> {
    match event {
        UiEvent::Terminal(TerminalEvent::Key(key)) if key == "Enter" && ctx.busy => {
            vec![RouteAction::QueueInput, RouteAction::Redraw]
        }
        UiEvent::Terminal(TerminalEvent::Key(key)) if key == "Enter" => {
            vec![RouteAction::SubmitInput, RouteAction::Redraw]
        }
        UiEvent::Terminal(TerminalEvent::Key(_)) => vec![RouteAction::Redraw],
        UiEvent::Terminal(TerminalEvent::Resize { .. })
        | UiEvent::Terminal(TerminalEvent::Paste(_))
        | UiEvent::Terminal(TerminalEvent::Tick) => vec![RouteAction::Redraw],
        UiEvent::Engine(EngineEvent::AssistantDelta(_)) => {
            vec![RouteAction::AppendTranscript, RouteAction::Redraw]
        }
        UiEvent::Engine(EngineEvent::ToolStarted(_)) => {
            vec![RouteAction::UpdateToolActivity, RouteAction::Redraw]
        }
        UiEvent::Engine(EngineEvent::PermissionRequested(_)) => {
            vec![RouteAction::ShowApproval, RouteAction::Redraw]
        }
        UiEvent::Engine(EngineEvent::Completed) => vec![RouteAction::Redraw],
        UiEvent::CommandResult(_) => vec![RouteAction::AppendTranscript, RouteAction::Redraw],
        UiEvent::OverlayResponse(_) if ctx.overlay_active => {
            vec![RouteAction::CloseOverlay, RouteAction::Redraw]
        }
        UiEvent::OverlayResponse(_) => vec![RouteAction::Noop],
    }
}

pub fn render_route_trace(events: &[UiEvent], ctx: RouteContext) -> String {
    events
        .iter()
        .map(|event| format!("{event:?} -> {:?}", route_event(event, ctx)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_terminal_variants_to_redraw() {
        for event in [
            UiEvent::Terminal(TerminalEvent::Paste("text".to_string())),
            UiEvent::Terminal(TerminalEvent::Resize {
                width: 80,
                height: 24,
            }),
            UiEvent::Terminal(TerminalEvent::Tick),
        ] {
            assert_eq!(
                route_event(&event, RouteContext::default()),
                vec![RouteAction::Redraw]
            );
        }
    }

    #[test]
    fn routes_engine_and_command_variants() {
        assert_eq!(
            route_event(
                &UiEvent::Engine(EngineEvent::AssistantDelta("hi".to_string())),
                RouteContext::default(),
            ),
            vec![RouteAction::AppendTranscript, RouteAction::Redraw]
        );
        assert_eq!(
            route_event(
                &UiEvent::Engine(EngineEvent::ToolStarted("bash".to_string())),
                RouteContext::default(),
            ),
            vec![RouteAction::UpdateToolActivity, RouteAction::Redraw]
        );
        assert_eq!(
            route_event(
                &UiEvent::Engine(EngineEvent::Completed),
                RouteContext::default(),
            ),
            vec![RouteAction::Redraw]
        );
        assert_eq!(
            route_event(
                &UiEvent::CommandResult("done".to_string()),
                RouteContext::default(),
            ),
            vec![RouteAction::AppendTranscript, RouteAction::Redraw]
        );
    }
}
