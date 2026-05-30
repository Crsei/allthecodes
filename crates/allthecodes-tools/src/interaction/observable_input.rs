use std::path::PathBuf;

use serde_json::{Map, Value};

pub(crate) fn backfill_file_path(input: &mut Map<String, Value>) {
    let Some(file_path) = input.get("file_path").and_then(Value::as_str) else {
        return;
    };
    input.insert(
        "file_path".to_string(),
        Value::String(expand_path(file_path)),
    );
}

fn expand_path(path: &str) -> String {
    let trimmed = path.trim();
    let expanded = if trimmed == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(trimmed))
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(trimmed))
    } else {
        PathBuf::from(trimmed)
    };

    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        allthecodes_utils::cwd::get_cwd().join(expanded)
    };

    absolute.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CwdGuard;

    impl CwdGuard {
        fn set(path: &str) -> Self {
            allthecodes_utils::cwd::set_cwd(path);
            Self
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            allthecodes_utils::cwd::reset_cwd();
        }
    }

    #[test]
    fn file_path_backfill_resolves_relative_paths() {
        let cwd = std::env::current_dir().expect("current dir");
        let _guard = CwdGuard::set(&cwd.to_string_lossy());
        let mut input = Map::new();
        input.insert(
            "file_path".to_string(),
            Value::String("src/main.rs".to_string()),
        );

        backfill_file_path(&mut input);

        let expected = cwd.join("src/main.rs").to_string_lossy().to_string();
        assert_eq!(
            input.get("file_path").and_then(Value::as_str),
            Some(expected.as_str())
        );
    }

    #[test]
    fn file_path_backfill_expands_home() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let mut input = Map::new();
        input.insert(
            "file_path".to_string(),
            Value::String("~/notes.md".to_string()),
        );

        backfill_file_path(&mut input);

        let expected = home.join("notes.md").to_string_lossy().to_string();
        assert_eq!(
            input.get("file_path").and_then(Value::as_str),
            Some(expected.as_str())
        );
    }
}
