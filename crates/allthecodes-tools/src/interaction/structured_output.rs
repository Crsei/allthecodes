//! StructuredOutput tool -- format data into structured output formats.
//!
//! Supports JSON pretty-printing, CSV conversion, and aligned text tables.
//! Pure data transformation with no I/O.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::AssistantMessage;

/// StructuredOutputTool -- format data into JSON, CSV, or table format.
pub struct StructuredOutputTool;

impl StructuredOutputTool {
    /// Pretty-print JSON data.
    fn format_json(data: &Value) -> Result<String, String> {
        serde_json::to_string_pretty(data).map_err(|e| format!("JSON serialization error: {}", e))
    }

    /// Convert an array of objects to CSV format.
    fn format_csv(data: &Value) -> Result<String, String> {
        let arr = data
            .as_array()
            .ok_or_else(|| "CSV format requires an array of objects".to_string())?;

        if arr.is_empty() {
            return Ok(String::new());
        }

        // Collect all unique keys from all objects (preserving first-seen order)
        let mut headers: Vec<String> = Vec::new();
        for item in arr {
            if let Some(obj) = item.as_object() {
                for key in obj.keys() {
                    if !headers.contains(key) {
                        headers.push(key.clone());
                    }
                }
            }
        }

        if headers.is_empty() {
            return Err("No object keys found in the array".to_string());
        }

        let mut lines = Vec::new();

        // Header row
        lines.push(
            headers
                .iter()
                .map(|h| csv_escape(h))
                .collect::<Vec<_>>()
                .join(","),
        );

        // Data rows
        for item in arr {
            let row: Vec<String> = headers
                .iter()
                .map(|h| {
                    let val = item.get(h).unwrap_or(&Value::Null);
                    csv_escape(&value_to_cell(val))
                })
                .collect();
            lines.push(row.join(","));
        }

        Ok(lines.join("\n"))
    }

    /// Format data as an aligned text table.
    fn format_table(data: &Value) -> Result<String, String> {
        let arr = data
            .as_array()
            .ok_or_else(|| "Table format requires an array of objects".to_string())?;

        if arr.is_empty() {
            return Ok("(empty)".to_string());
        }

        // Collect headers
        let mut headers: Vec<String> = Vec::new();
        for item in arr {
            if let Some(obj) = item.as_object() {
                for key in obj.keys() {
                    if !headers.contains(key) {
                        headers.push(key.clone());
                    }
                }
            }
        }

        if headers.is_empty() {
            return Err("No object keys found in the array".to_string());
        }

        // Build cell matrix
        let mut rows: Vec<Vec<String>> = Vec::new();
        for item in arr {
            let row: Vec<String> = headers
                .iter()
                .map(|h| {
                    let val = item.get(h).unwrap_or(&Value::Null);
                    value_to_cell(val)
                })
                .collect();
            rows.push(row);
        }

        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() && cell.len() > widths[i] {
                    widths[i] = cell.len();
                }
            }
        }

        let mut lines = Vec::new();

        // Header line
        let header_line: String = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:<width$}", h, width = widths[i]))
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(header_line);

        // Separator
        let sep: String = widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("-+-");
        lines.push(sep);

        // Data rows
        for row in &rows {
            let line: String = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let w = if i < widths.len() {
                        widths[i]
                    } else {
                        cell.len()
                    };
                    format!("{:<width$}", cell, width = w)
                })
                .collect::<Vec<_>>()
                .join(" | ");
            lines.push(line);
        }

        Ok(lines.join("\n"))
    }
}

/// Escape a string for CSV output.
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Convert a JSON value to a plain string for table/CSV cells.
fn value_to_cell(val: &Value) -> String {
    match val {
        Value::Null => "".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(val).unwrap_or_else(|_| val.to_string())
        }
    }
}

#[async_trait]
impl Tool for StructuredOutputTool {
    fn name(&self) -> &str {
        "StructuredOutput"
    }

    async fn description(&self, input: &Value) -> String {
        let fmt = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("data");
        format!("Format data as {}.", fmt)
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["json", "csv", "table"],
                    "description": "Output format: json, csv, or table"
                },
                "data": {
                    "description": "The data to format. For csv/table, should be an array of objects."
                }
            },
            "required": ["format", "data"]
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true // Pure data transformation
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let format = input.get("format").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(format, "json" | "csv" | "table") {
            return ValidationResult::Error {
                message: format!(
                    "Unknown format \"{}\". Must be json, csv, or table.",
                    format
                ),
                error_code: 1,
            };
        }
        if input.get("data").is_none() {
            return ValidationResult::Error {
                message: "\"data\" is required".to_string(),
                error_code: 1,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("json");
        let data = input.get("data").cloned().unwrap_or(Value::Null);

        let result = match format {
            "json" => Self::format_json(&data),
            "csv" => Self::format_csv(&data),
            "table" => Self::format_table(&data),
            _ => Err(format!("Unknown format: {}", format)),
        };

        match result {
            Ok(formatted) => Ok(ToolResult {
                data: json!({
                    "format": format,
                    "output": formatted,
                }),
                new_messages: vec![],
                ..Default::default()
            }),
            Err(e) => Ok(ToolResult {
                data: json!({ "error": e }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        "Use StructuredOutput to format data into structured formats.\n\n\
Formats:\n\
- \"json\": Pretty-print any JSON value.\n\
- \"csv\": Convert an array of objects to CSV (header row + data rows).\n\
- \"table\": Format an array of objects as an aligned text table.\n\n\
This is a pure data transformation tool with no side effects."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "StructuredOutput".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structured_output_tool_name() {
        let tool = StructuredOutputTool;
        assert_eq!(tool.name(), "StructuredOutput");
    }

    #[test]
    fn test_structured_output_schema() {
        let tool = StructuredOutputTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("format"));
        assert!(props.contains_key("data"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("format")));
        assert!(required.contains(&json!("data")));
    }

    #[test]
    fn test_structured_output_is_read_only() {
        let tool = StructuredOutputTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_structured_output_is_concurrency_safe() {
        let tool = StructuredOutputTool;
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_format_json() {
        let data = json!({"key": "value", "num": 42});
        let result = StructuredOutputTool::format_json(&data).unwrap();
        assert!(result.contains("\"key\""));
        assert!(result.contains("\"value\""));
        assert!(result.contains("42"));
    }

    #[test]
    fn test_format_csv_basic() {
        let data = json!([
            {"name": "Alice", "age": 30},
            {"name": "Bob", "age": 25},
        ]);
        let csv = StructuredOutputTool::format_csv(&data).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 rows
        assert!(lines[0].contains("name"));
        assert!(lines[0].contains("age"));
        assert!(lines[1].contains("Alice"));
        assert!(lines[2].contains("Bob"));
    }

    #[test]
    fn test_format_csv_empty_array() {
        let data = json!([]);
        let csv = StructuredOutputTool::format_csv(&data).unwrap();
        assert!(csv.is_empty());
    }

    #[test]
    fn test_format_csv_not_array() {
        let data = json!({"not": "an array"});
        let result = StructuredOutputTool::format_csv(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_table_basic() {
        let data = json!([
            {"name": "Alice", "score": 95},
            {"name": "Bob", "score": 87},
        ]);
        let table = StructuredOutputTool::format_table(&data).unwrap();
        assert!(table.contains("name"));
        assert!(table.contains("score"));
        assert!(table.contains("Alice"));
        assert!(table.contains("Bob"));
        // Check separator line
        assert!(table.contains("---"));
    }

    #[test]
    fn test_format_table_empty() {
        let data = json!([]);
        let table = StructuredOutputTool::format_table(&data).unwrap();
        assert_eq!(table, "(empty)");
    }

    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("simple"), "simple");
        assert_eq!(csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(csv_escape("has\"quote"), "\"has\"\"quote\"");
    }

    #[test]
    fn test_value_to_cell() {
        assert_eq!(value_to_cell(&Value::Null), "");
        assert_eq!(value_to_cell(&json!(true)), "true");
        assert_eq!(value_to_cell(&json!(42)), "42");
        assert_eq!(value_to_cell(&json!("hello")), "hello");
    }
}
