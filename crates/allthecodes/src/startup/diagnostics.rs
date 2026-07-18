use std::collections::HashSet;

/// The startup subsystem that produced a user-actionable diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupDiagnosticSource {
    Model,
    Auth,
    Plugin,
    Mcp,
    Dashboard,
    History,
    Settings,
    Other,
}

impl StartupDiagnosticSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Auth => "auth",
            Self::Plugin => "plugin",
            Self::Mcp => "mcp",
            Self::Dashboard => "dashboard",
            Self::History => "history",
            Self::Settings => "settings",
            Self::Other => "startup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupDiagnosticSeverity {
    Warning,
    Error,
}

/// Structured startup feedback. Logs remain the diagnostic source of truth;
/// this DTO is the explicitly user-visible subset allowed into the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupDiagnostic {
    pub(crate) stable_id: &'static str,
    pub(crate) severity: StartupDiagnosticSeverity,
    pub(crate) source: StartupDiagnosticSource,
    pub(crate) summary: String,
    pub(crate) detail: Option<String>,
    pub(crate) user_action: Option<String>,
}

impl StartupDiagnostic {
    pub(crate) fn warning(
        stable_id: &'static str,
        source: StartupDiagnosticSource,
        summary: impl Into<String>,
        detail: Option<String>,
        user_action: Option<String>,
    ) -> Self {
        Self::new(
            stable_id,
            StartupDiagnosticSeverity::Warning,
            source,
            summary,
            detail,
            user_action,
        )
    }

    pub(crate) fn error(
        stable_id: &'static str,
        source: StartupDiagnosticSource,
        summary: impl Into<String>,
        detail: Option<String>,
        user_action: Option<String>,
    ) -> Self {
        Self::new(
            stable_id,
            StartupDiagnosticSeverity::Error,
            source,
            summary,
            detail,
            user_action,
        )
    }

    fn new(
        stable_id: &'static str,
        severity: StartupDiagnosticSeverity,
        source: StartupDiagnosticSource,
        summary: impl Into<String>,
        detail: Option<String>,
        user_action: Option<String>,
    ) -> Self {
        Self {
            stable_id,
            severity,
            source,
            summary: redact_secrets(summary.into()),
            detail: detail.map(redact_secrets),
            user_action: user_action.map(redact_secrets),
        }
    }

    pub(crate) fn display_text(&self) -> String {
        let mut text = format!("[{}] {}", self.source.label(), self.summary);
        if let Some(detail) = self.detail.as_deref().filter(|detail| !detail.is_empty()) {
            text.push_str(": ");
            text.push_str(detail);
        }
        if let Some(action) = self
            .user_action
            .as_deref()
            .filter(|action| !action.is_empty())
        {
            text.push(' ');
            text.push_str(action);
        }
        text
    }
}

/// Deduplicate only identical stable diagnostics. Different MCP/plugin
/// failures retain their details, while repeated builder paths cannot spam a
/// single startup session.
pub(crate) fn deduplicate(
    diagnostics: impl IntoIterator<Item = StartupDiagnostic>,
) -> Vec<StartupDiagnostic> {
    let mut seen = HashSet::new();
    diagnostics
        .into_iter()
        .filter(|diagnostic| {
            let detail = diagnostic.detail.as_deref().unwrap_or_default();
            let key = format!("{}\u{1f}{}", diagnostic.stable_id, normalize(detail));
            seen.insert(key)
        })
        .collect()
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Redact credential-shaped values before a diagnostic can reach a session
/// transcript or exported history. This intentionally works without a global
/// tracing subscriber so debug logs and machine protocols remain unchanged.
pub(crate) fn redact_secrets(mut value: String) -> String {
    value = redact_url_userinfo(&value);
    value = redact_credential_paths(&value);
    value = redact_sensitive_assignments(&value);

    for marker in [
        "Authorization:",
        "authorization:",
        "Bearer ",
        "token=",
        "token:",
        "api_key=",
        "apiKey=",
        "api-key=",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "OPENAI_CODEX_AUTH_TOKEN=",
    ] {
        value = redact_after_marker(&value, marker);
    }

    for marker in ["api_key=", "apiKey=", "password=", "secret="] {
        value = redact_after_marker(&value, marker);
    }

    let mut redacted = String::with_capacity(value.len());
    let mut token = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
            token.push(character);
            continue;
        }
        append_redacted_token(&mut redacted, &mut token);
        redacted.push(character);
    }
    append_redacted_token(&mut redacted, &mut token);
    redacted
}

fn redact_url_userinfo(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative_scheme_end) = value[cursor..].find("://") {
        let scheme_end = cursor + relative_scheme_end + 3;
        let authority_end = value[scheme_end..]
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '/' | ',' | ';' | ')' | ']')
            })
            .map_or(value.len(), |offset| scheme_end + offset);
        let authority = &value[scheme_end..authority_end];
        let Some(at) = authority.rfind('@') else {
            output.push_str(&value[cursor..scheme_end]);
            cursor = scheme_end;
            continue;
        };

        output.push_str(&value[cursor..scheme_end]);
        output.push_str("<redacted>@");
        output.push_str(&authority[at + 1..]);
        cursor = authority_end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn redact_credential_paths(value: &str) -> String {
    let needles = [
        "credentials.json",
        "auth.json",
        "github_token.txt",
        ".aws/credentials",
        "application_default_credentials.json",
        "accessTokens.json",
        ".npmrc",
        ".pypirc",
        ".netrc",
    ];
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some((index, needle)) = needles
        .iter()
        .filter_map(|needle| {
            value[cursor..]
                .find(needle)
                .map(|offset| (cursor + offset, *needle))
        })
        .min_by_key(|(index, _)| *index)
    {
        let mut start = index;
        while start > cursor {
            let previous = value[..start].chars().next_back().unwrap_or_default();
            if previous.is_whitespace()
                || matches!(previous, '"' | '\'' | '(' | '[' | '{' | '<' | '=' | ':')
            {
                break;
            }
            start -= previous.len_utf8();
        }
        output.push_str(&value[cursor..start]);
        output.push_str("<redacted>");
        cursor = index + needle.len();
    }
    output.push_str(&value[cursor..]);
    output
}

fn redact_sensitive_assignments(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut copied_until = 0;
    let mut index = 0;

    while index < bytes.len() {
        if !is_secret_key_byte(bytes[index])
            || index
                .checked_sub(1)
                .is_some_and(|previous| is_secret_key_byte(bytes[previous]))
        {
            index += 1;
            continue;
        }

        let key_start = index;
        while index < bytes.len() && is_secret_key_byte(bytes[index]) {
            index += 1;
        }
        let key_end = index;
        if !is_sensitive_key(&value[key_start..key_end]) {
            continue;
        }

        let mut delimiter = key_end;
        if delimiter < bytes.len() && matches!(bytes[delimiter], b'\'' | b'"') {
            delimiter += 1;
        }
        while delimiter < bytes.len() && bytes[delimiter].is_ascii_whitespace() {
            delimiter += 1;
        }
        if delimiter >= bytes.len() || !matches!(bytes[delimiter], b'=' | b':') {
            continue;
        }

        let mut secret_start = delimiter + 1;
        while secret_start < bytes.len() && bytes[secret_start].is_ascii_whitespace() {
            secret_start += 1;
        }
        let quote = bytes
            .get(secret_start)
            .copied()
            .filter(|byte| matches!(byte, b'\'' | b'"'));
        if quote.is_some() {
            secret_start += 1;
        }
        if secret_start >= bytes.len() {
            break;
        }

        let authorization = value[key_start..key_end].eq_ignore_ascii_case("authorization");
        let mut secret_end = secret_start;
        while secret_end < bytes.len() {
            let byte = bytes[secret_end];
            if quote.is_some_and(|quote| byte == quote)
                || matches!(byte, b',' | b';' | b'\n' | b'\r')
                || (!authorization && byte.is_ascii_whitespace())
            {
                break;
            }
            secret_end += 1;
        }
        if secret_start == secret_end {
            continue;
        }

        output.push_str(&value[copied_until..secret_start]);
        output.push_str("<redacted>");
        copied_until = secret_end;
        index = secret_end;
    }

    output.push_str(&value[copied_until..]);
    output
}

fn is_secret_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    key == "authorization"
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("passwd")
        || key.contains("private_key")
        || key.contains("credential")
}

fn redact_after_marker(value: &str, marker: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative) = value[cursor..].find(marker) {
        let start = cursor + relative;
        output.push_str(&value[cursor..start + marker.len()]);
        let secret_start = start + marker.len();
        let secret_end = value[secret_start..]
            .find(|character: char| character.is_whitespace() || matches!(character, ',' | ';'))
            .map_or(value.len(), |offset| secret_start + offset);
        if secret_start < secret_end {
            output.push_str("<redacted>");
        }
        cursor = secret_end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn append_redacted_token(output: &mut String, token: &mut String) {
    if token.starts_with("sk-")
        || token.starts_with("ghp_")
        || token.starts_with("github_pat_")
        || token.starts_with("glpat-")
        || token.starts_with("xoxb-")
        || token.starts_with("xoxp-")
        || token.starts_with("xoxa-")
        || token.starts_with("xoxs-")
        || token.starts_with("AKIA")
        || token.starts_with("ASIA")
        || token.starts_with("AIza")
        || token.starts_with("ya29.")
        || looks_like_jwt(token)
        || token.contains("credentials.json")
        || token.contains("auth.json")
    {
        output.push_str("<redacted>");
    } else {
        output.push_str(token);
    }
    token.clear();
}

fn looks_like_jwt(token: &str) -> bool {
    token.starts_with("eyJ") && token.split('.').count() == 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_uses_stable_id_and_normalized_detail() {
        let first = StartupDiagnostic::warning(
            "mcp-connect",
            StartupDiagnosticSource::Mcp,
            "MCP connection failed",
            Some("server   unavailable".to_string()),
            None,
        );
        let second = StartupDiagnostic::warning(
            "mcp-connect",
            StartupDiagnosticSource::Mcp,
            "MCP connection failed",
            Some("server unavailable".to_string()),
            None,
        );
        assert_eq!(deduplicate([first, second]).len(), 1);
    }

    #[test]
    fn redaction_removes_headers_tokens_and_credential_paths() {
        let diagnostic = StartupDiagnostic::warning(
            "auth",
            StartupDiagnosticSource::Auth,
            "Authorization: Bearer sk-secret-value",
            Some(
                "https://user:password@example.test/login /home/user/.allthecodes/credentials.json token=abc123"
                    .to_string(),
            ),
            None,
        );
        let text = diagnostic.display_text();
        assert!(!text.contains("sk-secret-value"));
        assert!(!text.contains("abc123"));
        assert!(!text.contains("credentials.json"));
        assert!(!text.contains("/home/user"));
        assert!(!text.contains("user:password@"));
        assert!(text.contains("<redacted>@example.test"));
    }

    #[test]
    fn redaction_covers_cloud_keys_json_env_and_token_shapes() {
        for (input, secret) in [
            ("AZURE_API_KEY=azure-value", "azure-value"),
            ("GOOGLE_API_KEY: google-value", "google-value"),
            (r#"env={"API_TOKEN":"nested-value"}"#, "nested-value"),
            ("authorization: bearer lower-case-value", "lower-case-value"),
            ("AWS_SECRET_ACCESS_KEY=aws-value", "aws-value"),
            ("credential at /home/user/.aws/credentials", "/home/user"),
            ("token glpat-example-value", "glpat-example-value"),
            ("token AIzaExampleValue", "AIzaExampleValue"),
            (
                "token eyJheader.payload.signature",
                "eyJheader.payload.signature",
            ),
        ] {
            let redacted = redact_secrets(input.to_string());
            assert!(
                !redacted.contains(secret),
                "secret {secret:?} leaked from {input:?}: {redacted:?}"
            );
            assert!(redacted.contains("<redacted>"));
        }
    }

    #[test]
    fn display_text_keeps_actionable_source_and_action() {
        let diagnostic = StartupDiagnostic::warning(
            "model-fallback",
            StartupDiagnosticSource::Model,
            "Requested model unavailable",
            Some("using fallback".to_string()),
            Some("Choose another model with /model.".to_string()),
        );
        assert_eq!(
            diagnostic.display_text(),
            "[model] Requested model unavailable: using fallback Choose another model with /model."
        );
    }
}
