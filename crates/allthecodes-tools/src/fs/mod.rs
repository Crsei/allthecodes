//! Filesystem tools sub-domain.
//!
//! Tools in this module read, write, search, or otherwise manipulate files and
//! directories on the local filesystem. They share concurrency-safety traits
//! (most are read-only and parallelizable) and live here so filesystem concerns
//! stay isolated from execution, code intelligence, or networking tools.
//!
//! See `src/tools/ARCHITECTURE.md` for placement rules.

use std::sync::Arc;

use crate::tool::Tools;
use allthecodes_types::message::{Attachment, AttachmentMessage, Message};

pub mod apply_patch;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob_tool;
pub mod grep;
pub mod hash_edit;
pub(crate) mod hashline;
pub mod notebook_edit;
pub mod safe_write;

/// Returns every tool owned by the filesystem sub-domain.
///
/// The registry aggregates each sub-domain's `tools()` instead of hard-coding
/// the full list, so adding a new fs tool only requires touching this file.
pub fn tools() -> Tools {
    vec![
        Arc::new(file_read::FileReadTool::new()),
        Arc::new(file_write::FileWriteTool::new()),
        Arc::new(file_edit::FileEditTool::new()),
        Arc::new(hash_edit::HashEditTool::new()),
        Arc::new(notebook_edit::NotebookEditTool::new()),
        Arc::new(glob_tool::GlobTool::new()),
        Arc::new(grep::GrepTool),
        Arc::new(apply_patch::ApplyPatchTool),
        Arc::new(apply_patch::ApplyPatchFreeformTool),
    ]
}

pub(crate) fn edited_text_file_message(path: impl Into<String>) -> Message {
    Message::Attachment(AttachmentMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        attachment: Attachment::EditedTextFile { path: path.into() },
    })
}
