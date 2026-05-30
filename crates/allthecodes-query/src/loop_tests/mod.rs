use super::*;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use allthecodes_types::hooks::{
    HookEventConfig, HookOutput, HookRunner, HooksMap, PostToolHookResult, PreToolHookResult,
};
use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;

use crate::deps::{
    CompactionResult, ModelCallParams, ModelResponse, QueryDeps, ToolExecRequest, ToolExecResult,
};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::config::{QueryGates, QuerySource, TaskBudget};
use allthecodes_engine::types::message::{
    AssistantMessage, CompactMetadata, ContentBlock, ImageSource, MessageContent, StreamEvent,
    SystemMessage, SystemSubtype, ToolResultContent, Usage, UserMessage,
};
use allthecodes_engine::types::state::AutoCompactTracking;
use allthecodes_engine::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools};

mod mocks;

mod continuation_tests;
mod misc_tests;
mod recovery_tests;
mod tool_execution_tests;
mod tool_setup_tests;

use mocks::*;
