use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::common::{current_dir, string_param, validate_enum};
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_types::message::{AssistantMessage, ContentBlock, ImageSource, ToolResultContent};

pub fn tools() -> Tools {
    vec![Arc::new(ViewImageTool), Arc::new(ViewImageAliasTool)]
}

pub struct ViewImageTool;
pub struct ViewImageAliasTool;

fn model_capability<'a>(
    settings: &'a allthecodes_config::runtime_settings::SettingsJson,
    model: &str,
) -> Option<&'a allthecodes_config::settings::ModelCapabilitySettings> {
    settings.model_capabilities.get(model).or_else(|| {
        settings
            .model_capabilities
            .iter()
            .find_map(|(name, capability)| name.eq_ignore_ascii_case(model).then_some(capability))
    })
}

pub fn model_supports_image_input(
    settings: &allthecodes_config::runtime_settings::SettingsJson,
    model: &str,
) -> bool {
    model_capability(settings, model)
        .map(|capability| {
            capability
                .input_modalities
                .iter()
                .any(|modality| modality.eq_ignore_ascii_case("image"))
        })
        .unwrap_or(true)
}

pub fn model_supports_original_image_detail(
    settings: &allthecodes_config::runtime_settings::SettingsJson,
    model: &str,
) -> bool {
    model_capability(settings, model)
        .map(|capability| capability.supports_image_detail_original)
        .unwrap_or(false)
}

pub fn filter_tools_for_model_capabilities(
    tools: Tools,
    settings: &allthecodes_config::runtime_settings::SettingsJson,
    model: &str,
) -> Tools {
    let image_supported = model_supports_image_input(settings, model);
    tools
        .into_iter()
        .filter(|tool| image_supported || !matches!(tool.name(), "ViewImage" | "view_image"))
        .collect()
}

fn validate_view_image_model_capability(input: &Value, ctx: &ToolUseContext) -> ValidationResult {
    let app_state = (ctx.get_app_state)();
    let model = app_state.main_loop_model.as_str();
    if !model_supports_image_input(&app_state.settings, model) {
        return ValidationResult::Error {
            message: format!("model {model} does not support image input"),
            error_code: 400,
        };
    }
    if string_param(input, "detail") == Some("original")
        && !model_supports_original_image_detail(&app_state.settings, model)
    {
        return ValidationResult::Error {
            message: format!("model {model} does not support original image detail"),
            error_code: 400,
        };
    }
    ValidationResult::Ok
}

fn ensure_view_image_model_capability(input: &Value, ctx: &ToolUseContext) -> Result<()> {
    match validate_view_image_model_capability(input, ctx) {
        ValidationResult::Ok => Ok(()),
        ValidationResult::Error { message, .. } => bail!("{message}"),
    }
}

fn image_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "png" => Some("image/png"),
        Some(ext) if ext == "jpg" || ext == "jpeg" => Some("image/jpeg"),
        Some(ext) if ext == "webp" => Some("image/webp"),
        Some(ext) if ext == "gif" => Some("image/gif"),
        _ => None,
    }
}

fn validate_read_path(path: &str, ctx: &ToolUseContext) -> Result<PathBuf> {
    let validated = allthecodes_permissions::path_validation::validate_file_path(path)?;
    let cwd = current_dir();
    let app_state = (ctx.get_app_state)();
    if !allthecodes_permissions::path_validation::is_path_within_allowed_directories(
        &validated,
        &cwd,
        &app_state.tool_permission_context,
    ) {
        bail!(
            "path is outside the current working directory and configured additional directories: {}",
            validated.display()
        );
    }
    Ok(validated)
}

#[async_trait]
impl Tool for ViewImageTool {
    fn name(&self) -> &str {
        "ViewImage"
    }

    async fn description(&self, _input: &Value) -> String {
        "Read a local image file and return it as an image content block.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to a png, jpeg, webp, or gif image."},
                "detail": {"type": "string", "enum": ["auto", "original"], "description": "Image detail hint for downstream rendering."}
            },
            "required": ["path"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn get_path(&self, input: &Value) -> Option<String> {
        string_param(input, "path").map(ToOwned::to_owned)
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        let Some(path) = string_param(input, "path") else {
            return ValidationResult::Error {
                message: "path is required".into(),
                error_code: 400,
            };
        };
        if image_mime(Path::new(path)).is_none() {
            return ValidationResult::Error {
                message: "path must point to a png, jpeg, webp, or gif image".into(),
                error_code: 400,
            };
        }
        if let Some(result) = validate_enum(input, "detail", &["auto", "original"]) {
            return result;
        }
        validate_view_image_model_capability(input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let path = string_param(&input, "path").ok_or_else(|| anyhow!("path is required"))?;
        ensure_view_image_model_capability(&input, ctx)?;
        let validated = validate_read_path(path, ctx)?;
        let mime = image_mime(&validated).ok_or_else(|| anyhow!("unsupported image type"))?;
        let bytes = tokio::fs::read(&validated).await?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let detail = string_param(&input, "detail").unwrap_or("auto");

        Ok(ToolResult {
            data: json!({
                "path": validated.display().to_string(),
                "mime": mime,
                "size": bytes.len(),
                "sha256": sha256,
                "detail": detail,
            }),
            model_content: Some(ToolResultContent::Blocks(vec![ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".into(),
                    media_type: mime.into(),
                    data: encoded,
                },
            }])),
            display_preview: Some(format!("Viewed image {}", validated.display())),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Use ViewImage to inspect local png, jpeg, webp, or gif files without placing binary data in ordinary text output.".into()
    }
}

crate::common::tool_alias!(ViewImageAliasTool, "view_image", ViewImageTool);
