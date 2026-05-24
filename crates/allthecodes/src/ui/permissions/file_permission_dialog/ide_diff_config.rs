//! IDE diff configuration state for file permission dialogs.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeDiffConfig {
    pub enabled: bool,
    pub editor: String,
    pub supports_inline_diff: bool,
}

pub fn render_ide_diff_config(config: &IdeDiffConfig) -> String {
    format!(
        "IDE diff: {}\neditor: {}\ninline diff: {}",
        config.enabled, config.editor, config.supports_inline_diff
    )
}
