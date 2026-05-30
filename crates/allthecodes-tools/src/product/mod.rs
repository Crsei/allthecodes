//! Product-experience tools for context, artifacts, peer discovery, and remote runs.

#[cfg(feature = "full")]
mod capture;
#[cfg(feature = "full")]
mod common;
#[cfg(feature = "full")]
mod ctx_inspect;
#[cfg(feature = "full")]
mod peers;
#[cfg(feature = "full")]
mod review;
#[cfg(feature = "full")]
mod send_file;
#[cfg(feature = "full")]
mod snip;

use std::sync::Arc;

use crate::tool::Tools;

pub fn tools() -> Tools {
    vec![
        Arc::new(ctx_inspect::CtxInspectTool),
        Arc::new(capture::MonitorTool),
        Arc::new(capture::TerminalCaptureTool),
        Arc::new(send_file::SendUserFileTool),
        Arc::new(review::ReviewArtifactTool),
        Arc::new(snip::SnipTool),
        Arc::new(peers::ListPeersTool),
        Arc::new(peers::RemoteTriggerTool),
    ]
}
