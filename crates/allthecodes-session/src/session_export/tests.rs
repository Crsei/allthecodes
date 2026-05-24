use super::builders;
use super::compression;
use super::compression::detect_microcompact;
use super::*;
use allthecodes_types::message::*;
use std::path::Path;
use uuid::Uuid;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn make_user_msg(text: &str) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1700000000000,
        role: "user".into(),
        content: MessageContent::Text(text.into()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn make_assistant_msg(text: &str) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1700000001000,
        role: "assistant".into(),
        content: vec![ContentBlock::Text { text: text.into() }],
        usage: Some(Usage {
            input_tokens: 100,
            output_tokens: 50,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 0,
        }),
        stop_reason: Some("end_turn".into()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.001,
    })
}

fn make_assistant_with_tool_use(tool_use_id: &str, tool_name: &str) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1700000002000,
        role: "assistant".into(),
        content: vec![ContentBlock::ToolUse {
            id: tool_use_id.into(),
            name: tool_name.into(),
            input: serde_json::json!({"command": "ls"}),
        }],
        usage: Some(Usage {
            input_tokens: 200,
            output_tokens: 30,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }),
        stop_reason: Some("tool_use".into()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.002,
    })
}

fn make_tool_result_msg(tool_use_id: &str, result: &str, is_error: bool) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1700000003000,
        role: "user".into(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Text(result.into()),
            is_error,
        }]),
        is_meta: true,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn make_tool_result_image_msg(tool_use_id: &str) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1700000003000,
        role: "user".into(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Blocks(vec![ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".into(),
                    media_type: "image/png".into(),
                    data: "abcdef".into(),
                },
            }]),
            is_error: false,
        }]),
        is_meta: true,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn make_compact_boundary(pre: u64, post: u64) -> Message {
    make_compact_boundary_with_segment(pre, post, None)
}

fn make_compact_boundary_with_segment(
    pre: u64,
    post: u64,
    preserved_segment: Option<PreservedSegment>,
) -> Message {
    Message::System(SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1700000004000,
        subtype: SystemSubtype::CompactBoundary {
            compact_metadata: Some(CompactMetadata {
                pre_compact_token_count: pre,
                post_compact_token_count: post,
                preserved_segment,
            }),
        },
        content: format!("[Compacted: {} \u{2192} {} tokens]", pre, post),
    })
}

#[test]
fn test_reconstruct_tool_timeline_basic() {
    let messages = vec![
        make_user_msg("hello"),
        make_assistant_with_tool_use("tu_1", "Bash"),
        make_tool_result_msg("tu_1", "file1.rs\nfile2.rs", false),
        make_assistant_msg("Here are the files."),
    ];

    let timeline = reconstruct_tool_timeline(&messages);
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].tool_name, "Bash");
    assert_eq!(timeline[0].tool_use_id, "tu_1");
    assert!(timeline[0].result.is_some());
    assert!(!timeline[0].is_error);
    assert!(!timeline[0].was_content_replaced);
}

#[test]
fn test_reconstruct_multiple_tool_calls() {
    let messages = vec![
        make_user_msg("read two files"),
        make_assistant_with_tool_use("tu_1", "Read"),
        make_tool_result_msg("tu_1", "contents of file1", false),
        make_assistant_with_tool_use("tu_2", "Read"),
        make_tool_result_msg("tu_2", "contents of file2", false),
        make_assistant_msg("Done."),
    ];

    let timeline = reconstruct_tool_timeline(&messages);
    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0].sequence, 0);
    assert_eq!(timeline[1].sequence, 1);
}

#[test]
fn test_reconstruct_unmatched_tool_use() {
    let messages = vec![
        make_user_msg("do something"),
        make_assistant_with_tool_use("tu_orphan", "Bash"),
        // No tool_result — user interrupted
        make_user_msg("never mind"),
    ];

    let timeline = reconstruct_tool_timeline(&messages);
    assert_eq!(timeline.len(), 1);
    assert!(timeline[0].result.is_none());
    assert_eq!(timeline[0].tool_use_id, "tu_orphan");
}

#[test]
fn test_extract_compact_boundaries() {
    let messages = vec![
        make_user_msg("hello"),
        make_assistant_msg("hi"),
        make_compact_boundary(150000, 50000),
        make_user_msg("after compact"),
    ];

    let compression = extract_compression_events(&messages);
    assert_eq!(compression.compact_boundaries.len(), 1);
    assert_eq!(compression.total_compactions, 1);
    assert_eq!(
        compression.compact_boundaries[0].pre_compact_tokens,
        Some(150000)
    );
    assert_eq!(
        compression.compact_boundaries[0].post_compact_tokens,
        Some(50000)
    );
}

#[test]
fn test_extract_compact_boundaries_includes_preserved_segment() {
    let summary_uuid = Uuid::new_v4().to_string();
    let preserved_uuid = Uuid::new_v4().to_string();
    let messages = vec![make_compact_boundary_with_segment(
        150000,
        50000,
        Some(PreservedSegment {
            summary_message_uuid: Some(summary_uuid.clone()),
            preserved_message_uuids: vec![preserved_uuid.clone()],
        }),
    )];

    let compression = extract_compression_events(&messages);
    let segment = compression.compact_boundaries[0]
        .preserved_segment
        .as_ref()
        .expect("preserved segment");

    assert_eq!(
        segment.summary_message_uuid.as_deref(),
        Some(summary_uuid.as_str())
    );
    assert_eq!(segment.preserved_message_uuids, vec![preserved_uuid]);
}

#[test]
fn test_detect_content_replacement() {
    let marker = "head text\n\n[... 50000 characters omitted. Full output saved to: /tmp/tool-results/tu_big.txt ...]\n\ntail text";
    let content = ToolResultContent::Text(marker.into());
    let record = detect_content_replacement("tu_big", &content);
    assert!(record.is_some());
    let r = record.unwrap();
    assert_eq!(r.tool_use_id, "tu_big");
    assert_eq!(r.original_size_hint, Some(50000));
    assert_eq!(r.file_path.as_deref(), Some("/tmp/tool-results/tu_big.txt"));
}

#[test]
fn test_detect_no_replacement() {
    let content = ToolResultContent::Text("normal output".into());
    assert!(detect_content_replacement("tu_1", &content).is_none());
}

#[test]
fn test_detect_microcompact() {
    let marker = "head text\n\n[... 1500 characters omitted (microcompacted) ...]\n\ntail text";
    let content = ToolResultContent::Text(marker.into());
    let record = detect_microcompact("tu_mc", &content);
    assert!(record.is_some());
    let r = record.unwrap();
    assert_eq!(r.tool_use_id, "tu_mc");
    assert_eq!(r.omitted_chars, 1500);
}

#[test]
fn test_detect_microcompact_not_present() {
    let content = ToolResultContent::Text("normal output".into());
    assert!(detect_microcompact("tu_1", &content).is_none());
}

#[test]
fn test_detect_content_replacement_in_result_microcompact() {
    let content = ToolResultContent::Text(
        "head\n\n[... 800 characters omitted (microcompacted) ...]\n\ntail".into(),
    );
    assert!(compression::detect_content_replacement_in_result(&content));
}

#[test]
fn test_build_context_snapshot() {
    let messages = vec![
        make_user_msg("hello"),
        make_assistant_with_tool_use("tu_1", "Bash"),
        make_tool_result_msg("tu_1", "output", false),
        make_assistant_msg("done"),
    ];

    let ctx = build_context_snapshot(&messages);
    assert!(ctx.estimated_total_tokens > 0);
    assert_eq!(ctx.context_window_size, 200_000);
    assert!(ctx.utilization_pct >= 0.0);
    assert_eq!(ctx.tool_use_count, 1);
    assert!(ctx.unique_tools_used.contains(&"Bash".to_string()));
    assert!(ctx.total_cost_usd > 0.0);
    assert_eq!(ctx.api_call_count, 2); // assistant with tool_use + assistant with text
}

#[test]
fn test_build_transcript_data() {
    let messages = vec![
        make_user_msg("hello"),
        make_assistant_msg("hi"),
        make_compact_boundary(100000, 30000),
    ];

    let transcript = builders::build_transcript_data(&messages);
    assert_eq!(transcript.message_count, 3);
    assert_eq!(transcript.user_message_count, 1);
    assert_eq!(transcript.assistant_message_count, 1);
    assert_eq!(transcript.system_message_count, 1);
    assert_eq!(transcript.messages.len(), 3);
}

#[test]
fn test_build_transcript_data_sanitizes_image_blocks_for_export() {
    let messages = vec![
        make_assistant_with_tool_use("tu_img", "ComputerUseScreenshot"),
        make_tool_result_image_msg("tu_img"),
    ];

    let transcript = builders::build_transcript_data(&messages);
    let source = &transcript.messages[1]["content"][0]["content"][0]["source"];

    assert_eq!(source["media_type"], "image/png");
    assert_eq!(source["metadata"]["base64_length"], 6);
    assert_ne!(source["data"], "abcdef");
    assert!(source["data"].as_str().unwrap().contains("image omitted"));
}

#[test]
fn test_build_session_export_schema_v2_includes_api_view_defaults() {
    let export = build_session_export("session-export-v2", &[make_user_msg("hello")], "/tmp");

    assert_eq!(export.schema_version, SESSION_EXPORT_SCHEMA_VERSION);
    assert_eq!(
        export.raw_transcript.message_count,
        export.transcript.message_count
    );
    assert_eq!(export.api_view.request_count, 0);
    assert!(export.api_requests.is_empty());
}

#[test]
#[serial_test::serial]
fn test_build_session_export_reports_bad_api_snapshot_log() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
    let path = crate::request_snapshot::snapshot_path("session-bad-snapshots");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "{not-json}\n").unwrap();

    let export = build_session_export("session-bad-snapshots", &[make_user_msg("hello")], "/tmp");

    assert_eq!(export.api_view.request_count, 0);
    assert!(export.api_requests.is_empty());
    assert!(
        export.api_view.diagnostics[0].contains("failed to load API request snapshots"),
        "{:?}",
        export.api_view.diagnostics
    );
}

#[test]
fn test_old_session_export_json_still_deserializes() {
    let old = serde_json::json!({
        "schema_version": 1,
        "exported_at": "2026-01-01T00:00:00Z",
        "session": {
            "session_id": "old",
            "project_path": null,
            "git_branch": null,
            "git_head_sha": null,
            "model": null,
            "started_at": null,
            "ended_at": null
        },
        "transcript": {
            "messages": [],
            "message_count": 0,
            "user_message_count": 0,
            "assistant_message_count": 0,
            "system_message_count": 0
        },
        "tool_calls": [],
        "compression": {
            "compact_boundaries": [],
            "content_replacements": [],
            "microcompact_replacements": [],
            "total_compactions": 0
        },
        "context": {
            "estimated_total_tokens": 0,
            "context_window_size": 200000,
            "utilization_pct": 0.0,
            "total_cost_usd": 0.0,
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "cache_read_tokens": 0,
            "api_call_count": 0,
            "tool_use_count": 0,
            "unique_tools_used": []
        }
    });

    let parsed: SessionExport = serde_json::from_value(old).expect("old schema readable");
    assert_eq!(parsed.schema_version, 1);
    assert!(parsed.api_requests.is_empty());
    assert_eq!(parsed.api_view.request_count, 0);
    assert!(parsed.api_view.diagnostics.is_empty());
}

#[test]
fn test_export_dir() {
    let dir = get_export_dir();
    assert!(dir.to_string_lossy().contains("exports"));
}

#[test]
fn test_format_ts_millis() {
    let ts = format_ts_millis(1700000000000);
    assert!(ts.contains("2023"));
}
