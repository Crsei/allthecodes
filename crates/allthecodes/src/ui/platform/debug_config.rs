// test infrastructure — debug-config rendering not wired to production TUI
//! Debug-config rendering helpers.

use ratatui::text::{Line, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugConfigEntry {
    pub key: String,
    pub value: String,
    pub source: Option<String>,
}

impl DebugConfigEntry {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

pub fn new_debug_config_output(entries: &[DebugConfigEntry]) -> String {
    if entries.is_empty() {
        return "No debug configuration entries.".to_string();
    }
    let width = entries
        .iter()
        .map(|entry| entry.key.len())
        .max()
        .unwrap_or(0);
    entries
        .iter()
        .map(|entry| {
            let source = entry
                .source
                .as_ref()
                .map(|source| format!(" ({source})"))
                .unwrap_or_default();
            format!(
                "{key:width$} = {value}{source}",
                key = entry.key,
                value = entry.value,
                width = width
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_debug_config_lines(entries: &[DebugConfigEntry]) -> Vec<Line<'static>> {
    if entries.is_empty() {
        return vec![Line::from("No debug configuration entries.")];
    }
    entries
        .iter()
        .map(|entry| {
            let mut spans = vec![
                Span::raw(entry.key.clone()),
                Span::raw(" = "),
                Span::raw(entry.value.clone()),
            ];
            if let Some(source) = &entry.source {
                spans.push(Span::raw(format!(" ({source})")));
            }
            Line::from(spans)
        })
        .collect()
}

pub fn flatten_json_key_values(value: &serde_json::Value) -> Vec<DebugConfigEntry> {
    fn walk(prefix: String, value: &serde_json::Value, out: &mut Vec<DebugConfigEntry>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    let next = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    walk(next, child, out);
                }
            }
            other => out.push(DebugConfigEntry::new(prefix, other.to_string())),
        }
    }

    let mut out = Vec::new();
    walk(String::new(), value, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_nested_json() {
        let entries = flatten_json_key_values(&serde_json::json!({"a": {"b": true}}));
        assert_eq!(entries[0], DebugConfigEntry::new("a.b", "true"));
    }

    #[test]
    fn renders_entries_with_sources() {
        let entries = vec![DebugConfigEntry::new("mode", "debug").with_source("env")];
        let output = new_debug_config_output(&entries);
        assert_eq!(output, "mode = debug (env)");

        let lines = render_debug_config_lines(&entries);
        let rendered = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "mode = debug (env)");
    }
}
