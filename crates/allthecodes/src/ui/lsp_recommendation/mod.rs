//! Rust-side LSP recommendation UI surfaces.
pub mod lsp_recommendation_menu;

#[cfg(test)]
mod tests {
    use super::lsp_recommendation_menu::{render_lsp_recommendation_menu, LspRecommendation};

    #[test]
    fn snapshot_lsp_recommendation_menu() {
        let mut rust = LspRecommendation::new("Rust", "rust-analyzer");
        rust.install_command = Some("rustup component add rust-analyzer".to_string());
        rust.reason = "Cargo.toml detected".to_string();
        rust.selected = true;
        let rendered = render_lsp_recommendation_menu(&[
            rust,
            LspRecommendation::new("TypeScript", "typescript-language-server"),
        ]);
        insta::assert_snapshot!("lsp_recommendation_menu", rendered);
    }
}
