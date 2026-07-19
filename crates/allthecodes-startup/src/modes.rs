//! Non-interactive output modes — `--output-format json` (JSONL on stdout)
//! and `-p` plain print mode. Both consume the same SDK message stream and
//! map `SdkMessage::Result::is_error` onto the process exit code.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Context;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QuerySource;
use allthecodes_types::sdk::{SdkMessage, SdkResult};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct PrintMessageEffect {
    wrote_stdout: bool,
    is_error: bool,
}

fn write_json_message<W: Write>(writer: &mut W, message: &SdkMessage) -> anyhow::Result<()> {
    let json = serde_json::to_string(message).context("failed to serialize SdkMessage to JSON")?;
    writeln!(writer, "{json}").context("failed to write JSONL SDK message")
}

fn write_print_message<WOut: Write, WErr: Write>(
    stdout: &mut WOut,
    stderr: &mut WErr,
    message: &SdkMessage,
) -> io::Result<PrintMessageEffect> {
    let mut effect = PrintMessageEffect::default();
    match message {
        SdkMessage::Assistant(assistant_msg) if !assistant_msg.message.is_api_error_message => {
            for block in &assistant_msg.message.content {
                if let allthecodes_types::message::ContentBlock::Text { text } = block {
                    write!(stdout, "{text}")?;
                    effect.wrote_stdout |= !text.is_empty();
                }
            }
        }
        SdkMessage::Result(result) if result.is_error => {
            if !result.result.trim().is_empty() {
                writeln!(stderr, "{}", result.result)?;
            }
            effect.is_error = true;
        }
        _ => {}
    }
    Ok(effect)
}

fn write_startup_failure<WOut: Write, WErr: Write>(
    stdout: &mut WOut,
    stderr: &mut WErr,
    json_mode: bool,
    result: &SdkResult,
) -> anyhow::Result<()> {
    if json_mode {
        write_json_message(stdout, &SdkMessage::Result(result.clone()))
    } else {
        writeln!(stderr, "{}", result.result).context("failed to write startup error")
    }
}

/// Emit a pre-submit failure using the same terminal `SdkResult` contract as
/// failures produced by the query engine.
pub fn emit_startup_failure(json_mode: bool, result: SdkResult) -> anyhow::Result<ExitCode> {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();
    write_startup_failure(&mut stdout, &mut stderr, json_mode, &result)?;
    stdout.flush().context("failed to flush stdout")?;
    stderr.flush().context("failed to flush stderr")?;
    Ok(ExitCode::FAILURE)
}

/// JSON output mode (for SDK consumers — JSONL on stdout).
pub async fn run_json_mode(engine: &QueryEngine, prompt: &str) -> anyhow::Result<ExitCode> {
    use futures::StreamExt;

    let stream = engine.submit_message(prompt, QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);
    let mut exit_code = ExitCode::SUCCESS;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    while let Some(msg) = stream.next().await {
        write_json_message(&mut stdout, &msg)?;

        if let SdkMessage::Result(ref result) = msg {
            if result.is_error {
                exit_code = ExitCode::FAILURE;
            }
        }
    }

    stdout.flush().context("failed to flush JSONL stdout")?;
    Ok(exit_code)
}

/// Plain-text print mode (non-interactive `-p`). Emits assistant text
/// blocks as they arrive and a trailing newline at the end.
pub async fn run_print_mode(engine: &QueryEngine, prompt: &str) -> anyhow::Result<ExitCode> {
    use futures::StreamExt;

    let stream = engine.submit_message(prompt, QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    let mut exit_code = ExitCode::SUCCESS;
    let mut wrote_stdout = false;
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();

    while let Some(msg) = stream.next().await {
        let effect = write_print_message(&mut stdout, &mut stderr, &msg)
            .context("failed to write plain SDK output")?;
        wrote_stdout |= effect.wrote_stdout;
        if effect.is_error {
            exit_code = ExitCode::FAILURE;
        }
    }

    if wrote_stdout {
        writeln!(stdout).context("failed to write trailing output newline")?;
    }
    stdout.flush().context("failed to flush stdout")?;
    stderr.flush().context("failed to flush stderr")?;
    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{AssistantMessage, ContentBlock};
    use allthecodes_types::sdk::{ResultSubtype, SdkAssistantMessage, UsageTracking};

    fn assistant(text: &str, is_api_error_message: bool) -> SdkMessage {
        SdkMessage::Assistant(SdkAssistantMessage {
            message: AssistantMessage {
                uuid: Default::default(),
                timestamp: 0,
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: text.to_string(),
                }],
                usage: None,
                stop_reason: Some("end_turn".to_string()),
                is_api_error_message,
                api_error: is_api_error_message.then(|| "provider rejected request".to_string()),
                cost_usd: 0.0,
            },
            session_id: "session-test".to_string(),
            parent_tool_use_id: None,
        })
    }

    fn result(text: &str, is_error: bool) -> SdkResult {
        SdkResult {
            subtype: if is_error {
                ResultSubtype::ErrorDuringExecution
            } else {
                ResultSubtype::Success
            },
            is_error,
            duration_ms: 1,
            duration_api_ms: 1,
            num_turns: 1,
            result: text.to_string(),
            stop_reason: Some("end_turn".to_string()),
            session_id: "session-test".to_string(),
            total_cost_usd: 0.0,
            usage: UsageTracking::default(),
            permission_denials: Vec::new(),
            structured_output: None,
            uuid: Default::default(),
            errors: is_error.then(|| vec![text.to_string()]).unwrap_or_default(),
        }
    }

    #[test]
    fn plain_mode_suppresses_api_error_assistant_and_writes_terminal_error_to_stderr() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let assistant_effect = write_print_message(
            &mut stdout,
            &mut stderr,
            &assistant("API error: unsafe provider detail", true),
        )
        .unwrap();
        let result_effect = write_print_message(
            &mut stdout,
            &mut stderr,
            &SdkMessage::Result(result("Request failed", true)),
        )
        .unwrap();

        assert!(stdout.is_empty());
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("Request failed"));
        assert!(!assistant_effect.wrote_stdout);
        assert!(result_effect.is_error);
    }

    #[test]
    fn plain_mode_keeps_success_text_on_stdout() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let effect =
            write_print_message(&mut stdout, &mut stderr, &assistant("completed", false)).unwrap();

        assert_eq!(String::from_utf8(stdout).unwrap(), "completed");
        assert!(stderr.is_empty());
        assert!(effect.wrote_stdout);
        assert!(!effect.is_error);
    }

    #[test]
    fn json_mode_preserves_api_error_assistant_and_error_result() {
        let mut stdout = Vec::new();
        let assistant = assistant("API error: provider rejected request", true);
        let result = SdkMessage::Result(result("Request failed", true));

        write_json_message(&mut stdout, &assistant).unwrap();
        write_json_message(&mut stdout, &result).unwrap();

        let lines = String::from_utf8(stdout).unwrap();
        assert_eq!(lines.lines().count(), 2);
        assert!(lines.contains("\"type\":\"assistant\""));
        assert!(lines.contains("\"is_api_error_message\":true"));
        assert!(lines.contains("\"type\":\"result\""));
        assert!(lines.contains("\"is_error\":true"));
    }

    #[test]
    fn startup_failure_is_plain_stderr_or_full_jsonl_result() {
        let failure = result("Unable to resume session", true);
        let mut plain_stdout = Vec::new();
        let mut plain_stderr = Vec::new();
        write_startup_failure(&mut plain_stdout, &mut plain_stderr, false, &failure).unwrap();
        assert!(plain_stdout.is_empty());
        assert!(String::from_utf8(plain_stderr)
            .unwrap()
            .contains("Unable to resume session"));

        let mut json_stdout = Vec::new();
        let mut json_stderr = Vec::new();
        write_startup_failure(&mut json_stdout, &mut json_stderr, true, &failure).unwrap();
        assert!(json_stderr.is_empty());
        let value: serde_json::Value =
            serde_json::from_slice(json_stdout.strip_suffix(b"\n").unwrap()).unwrap();
        assert_eq!(value["type"], "result");
        assert_eq!(value["subtype"], "error_during_execution");
        assert_eq!(value["is_error"], true);
    }
}
