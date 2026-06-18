//! JSONL stdio transport and legacy frontend parser.

use std::io;

use allthecodes_ipc_protocol::{BackendMessage, FrontendMessage};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use super::{IpcFrame, IpcReader, IpcTransport, IpcWriter};

/// Parsed frontend line or an explicit diagnostic backend message.
#[derive(Debug)]
pub enum ParsedFrontendLine {
    Message(FrontendMessage),
    Diagnostic(Box<BackendMessage>),
}

/// Parse one newline-delimited frontend message.
pub fn parse_frontend_line(line: &str) -> ParsedFrontendLine {
    match serde_json::from_str::<FrontendMessage>(line) {
        Ok(msg) => ParsedFrontendLine::Message(msg),
        Err(err) => ParsedFrontendLine::Diagnostic(Box::new(BackendMessage::Error {
            message: format!("invalid FrontendMessage: {err}; source_phase=client_transport"),
            recoverable: true,
        })),
    }
}

pub struct JsonlStdioTransport;

impl JsonlStdioTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonlStdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

pub struct JsonlStdioReader {
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::io::Stdin>>,
}

pub struct JsonlStdioWriter {
    stdout: tokio::io::Stdout,
}

impl IpcTransport for JsonlStdioTransport {
    type Reader = JsonlStdioReader;
    type Writer = JsonlStdioWriter;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let stdin = tokio::io::BufReader::new(tokio::io::stdin());
        (
            JsonlStdioReader {
                lines: stdin.lines(),
            },
            JsonlStdioWriter {
                stdout: tokio::io::stdout(),
            },
        )
    }
}

#[async_trait]
impl IpcReader for JsonlStdioReader {
    async fn read_frame(&mut self) -> io::Result<Option<IpcFrame>> {
        self.lines
            .next_line()
            .await
            .map(|line| line.map(IpcFrame::new))
    }
}

#[async_trait]
impl IpcWriter for JsonlStdioWriter {
    async fn write_frame(&mut self, frame: IpcFrame) -> io::Result<()> {
        self.stdout.write_all(frame.line.as_bytes()).await?;
        self.stdout.write_all(b"\n").await?;
        self.stdout.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_frontend_message() {
        let parsed = parse_frontend_line(r#"{"type":"quit"}"#);
        assert!(matches!(
            parsed,
            ParsedFrontendLine::Message(FrontendMessage::Quit)
        ));
    }

    #[test]
    fn parses_legacy_submit_permission_and_question_messages() {
        let submit = parse_frontend_line(r#"{"type":"submit_prompt","text":"hello","id":"ui-1"}"#);
        assert!(matches!(
            submit,
            ParsedFrontendLine::Message(FrontendMessage::SubmitPrompt { text, id })
                if text == "hello" && id == "ui-1"
        ));

        let permission = parse_frontend_line(
            r#"{"type":"permission_response","tool_use_id":"tool-1","decision":"allow"}"#,
        );
        assert!(matches!(
            permission,
            ParsedFrontendLine::Message(FrontendMessage::PermissionResponse {
                tool_use_id,
                decision,
                ..
            }) if tool_use_id == "tool-1" && decision == "allow"
        ));

        let question =
            parse_frontend_line(r#"{"type":"question_response","id":"q-1","text":"yes"}"#);
        assert!(matches!(
            question,
            ParsedFrontendLine::Message(FrontendMessage::QuestionResponse { id, text, .. })
                if id == "q-1" && text == "yes"
        ));
    }

    #[test]
    fn parse_failure_is_debuggable_diagnostic() {
        let parsed = parse_frontend_line("{bad json");
        let ParsedFrontendLine::Diagnostic(diagnostic) = parsed else {
            panic!("expected diagnostic");
        };
        let BackendMessage::Error {
            message,
            recoverable,
        } = *diagnostic
        else {
            panic!("expected diagnostic");
        };

        assert!(recoverable);
        assert!(message.contains("invalid FrontendMessage"));
        assert!(message.contains("source_phase=client_transport"));
    }
}
