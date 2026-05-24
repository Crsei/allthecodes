//! /model-add command -- add a model with token pricing to .env.
//!
//! Usage:
//!   /model-add <name> <input_price> <output_price>
//!     Sets CLAUDE_MODEL, MODEL_INPUT_PRICE, MODEL_OUTPUT_PRICE in .env
//!
//!   /model-add <name>
//!     Sets CLAUDE_MODEL in .env; uses built-in pricing if known
//!
//! Prices are in USD per 1M tokens.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::Path;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_models::pricing;

pub struct ModelAddHandler;

#[async_trait]
impl CommandHandler for ModelAddHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        if parts.is_empty() {
            return Ok(CommandResult::Output(
                "Usage: /model-add <name> [input_price output_price]\n\
                 \n\
                 Examples:\n\
                 \x20 /model-add gpt-4o 2.50 10.00\n\
                 \x20 /model-add claude-sonnet-4    (uses built-in pricing)\n\
                 \n\
                 Prices are in USD per 1M tokens.\n\
                 Writes CLAUDE_MODEL, MODEL_INPUT_PRICE, MODEL_OUTPUT_PRICE to .env"
                    .into(),
            ));
        }

        let model_name = parts[0];

        let (input_price, output_price) = if parts.len() >= 3 {
            // User provided explicit prices
            let input: f64 = parts[1]
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid input price: {}", parts[1]))?;
            let output: f64 = parts[2]
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid output price: {}", parts[2]))?;
            (input, output)
        } else {
            // Try built-in pricing
            let p = pricing::get_pricing(model_name);
            if p.input_per_1m == 0.0 && p.output_per_1m == 0.0 {
                return Ok(CommandResult::Output(format!(
                    "Unknown model '{}' — no built-in pricing found.\n\
                     Please specify prices: /model-add {} <input_price> <output_price>",
                    model_name, model_name
                )));
            }
            (p.input_per_1m, p.output_per_1m)
        };

        // Write to .env in cwd
        let env_path = ctx.cwd.join(".env");
        let mut env_vars = load_env_file(&env_path)?;
        env_vars.insert("CLAUDE_MODEL".into(), model_name.into());
        env_vars.insert("MODEL_INPUT_PRICE".into(), format_price(input_price));
        env_vars.insert("MODEL_OUTPUT_PRICE".into(), format_price(output_price));
        write_env_file(&env_path, &env_vars)?;

        // Also update process env so pricing takes effect immediately
        set_env_var("CLAUDE_MODEL", model_name);
        set_env_var("MODEL_INPUT_PRICE", format_price(input_price));
        set_env_var("MODEL_OUTPUT_PRICE", format_price(output_price));

        // Switch the active model
        ctx.app_state.main_loop_model = model_name.to_string();

        Ok(CommandResult::Output(format!(
            "Model added: {}\n\
             \x20 Input:  ${}/1M tokens\n\
             \x20 Output: ${}/1M tokens\n\
             Updated .env at: {}",
            model_name,
            format_price(input_price),
            format_price(output_price),
            env_path.display()
        )))
    }
}

fn format_price(price: f64) -> String {
    if price == price.floor() {
        format!("{:.1}", price)
    } else {
        format!("{}", price)
    }
}

fn set_env_var<K, V>(key: K, value: V)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    unsafe {
        std::env::set_var(key, value);
    }
}

#[cfg(test)]
fn remove_env_var<K>(key: K)
where
    K: AsRef<std::ffi::OsStr>,
{
    unsafe {
        std::env::remove_var(key);
    }
}

/// Load existing .env file into an ordered map, preserving unknown keys.
fn load_env_file(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    if !path.exists() {
        return Ok(map);
    }
    for item in dotenvy::from_path_iter(path)
        .with_context(|| format!("Failed to load existing .env {}", path.display()))?
    {
        let (key, value) =
            item.with_context(|| format!("Failed to parse existing .env {}", path.display()))?;
        map.insert(key, value);
    }
    Ok(map)
}

/// Write env vars back to .env file.
fn write_env_file(path: &Path, vars: &BTreeMap<String, String>) -> Result<()> {
    // Read existing file to preserve comments and ordering
    let existing = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read existing .env {}", path.display()))?
    } else {
        String::new()
    };
    let mut lines: Vec<String> = Vec::new();
    let mut written_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in existing.lines() {
        let trimmed = line.trim();
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.starts_with('#') {
                if let Some(new_val) = vars.get(key) {
                    lines.push(format!("{}={}", key, new_val));
                    written_keys.insert(key.to_string());
                    continue;
                }
            }
        }
        lines.push(line.to_string());
    }

    // Append new keys not in original file
    for (key, val) in vars {
        if !written_keys.contains(key) {
            lines.push(format!("{}={}", key, val));
        }
    }

    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    /// Save and restore env vars to avoid contaminating parallel tests.
    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            let saved = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for &(key, ref val) in &self.saved {
                match val {
                    Some(v) => set_env_var(key, v),
                    None => remove_env_var(key),
                }
            }
        }
    }

    const ENV_KEYS: &[&str] = &["CLAUDE_MODEL", "MODEL_INPUT_PRICE", "MODEL_OUTPUT_PRICE"];

    #[tokio::test]
    async fn test_model_add_no_args_shows_help() {
        let tmp = TempDir::new().unwrap();
        let handler = ModelAddHandler;
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage:"));
                assert!(text.contains("model-add"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_model_add_with_explicit_prices() {
        let _guard = EnvGuard::new(ENV_KEYS);
        let tmp = TempDir::new().unwrap();
        let handler = ModelAddHandler;
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        let result = handler.execute("my-model 1.5 7.0", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("my-model"));
                assert!(text.contains("1.5"));
                assert!(text.contains("7.0"));
            }
            _ => panic!("Expected Output result"),
        }

        // Verify .env was written
        let env_content = std::fs::read_to_string(tmp.path().join(".env")).unwrap();
        assert!(env_content.contains("CLAUDE_MODEL=my-model"));
        assert!(env_content.contains("MODEL_INPUT_PRICE=1.5"));
        assert!(env_content.contains("MODEL_OUTPUT_PRICE=7.0"));

        // Verify model was switched
        assert_eq!(ctx.app_state.main_loop_model, "my-model");
    }

    #[tokio::test]
    async fn test_model_add_known_model_uses_builtin() {
        let _guard = EnvGuard::new(ENV_KEYS);
        // Remove pricing env override so built-in table is used
        remove_env_var("MODEL_INPUT_PRICE");
        remove_env_var("MODEL_OUTPUT_PRICE");
        let tmp = TempDir::new().unwrap();
        let handler = ModelAddHandler;
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        let result = handler.execute("gpt-4o", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("gpt-4o"));
                assert!(text.contains("2.5"));
                assert!(text.contains("10.0"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_model_add_unknown_model_no_price_errors() {
        let _guard = EnvGuard::new(ENV_KEYS);
        remove_env_var("MODEL_INPUT_PRICE");
        remove_env_var("MODEL_OUTPUT_PRICE");
        let tmp = TempDir::new().unwrap();
        let handler = ModelAddHandler;
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        let result = handler
            .execute("totally-unknown-model", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown model"));
                assert!(text.contains("specify prices"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_model_add_preserves_existing_env() {
        let _guard = EnvGuard::new(ENV_KEYS);
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".env"),
            "AZURE_API_KEY=secret123\nCLAUDE_MODEL=old-model\n",
        )
        .unwrap();

        let handler = ModelAddHandler;
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        handler
            .execute("new-model 1.0 2.0", &mut ctx)
            .await
            .unwrap();

        let env_content = std::fs::read_to_string(tmp.path().join(".env")).unwrap();
        assert!(env_content.contains("AZURE_API_KEY=secret123"));
        assert!(env_content.contains("CLAUDE_MODEL=new-model"));
        assert!(!env_content.contains("old-model"));
    }

    #[tokio::test]
    async fn test_model_add_invalid_existing_env_aborts_without_rewrite() {
        let _guard = EnvGuard::new(ENV_KEYS);
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        let original = "AZURE_API_KEY=secret123\nBROKEN=\"unterminated\n";
        std::fs::write(&env_path, original).unwrap();

        let handler = ModelAddHandler;
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        let err = match handler.execute("new-model 1.0 2.0", &mut ctx).await {
            Ok(_) => panic!("expected invalid .env to abort"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("Failed to parse existing .env")
                || format!("{err:#}").contains("Failed to parse existing .env"),
            "unexpected error: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&env_path).unwrap(), original);
        assert_ne!(ctx.app_state.main_loop_model, "new-model");
    }
}
