//! Backward-compatible `/api/tui/ws` shim.

pub use crate::ws::terminal::{
    legacy_tui_ws_handler as tui_ws_handler, PtyDiagnostics, PtyDiagnosticsSnapshot, TuiWsParams,
};
