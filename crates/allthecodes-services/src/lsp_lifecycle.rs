//! LSP server lifecycle management — start, stop, and track LSP server
//! processes for code intelligence.
//!
//! This module manages the process lifecycle only. JSON-RPC protocol
//! communication will be added in a future phase.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A running or configured LSP server instance.
#[derive(Debug)]
pub struct LspServerInstance {
    /// Language identifier (e.g. "rust", "python").
    pub language_id: String,
    /// Command to start the server.
    pub command: String,
    /// Arguments to pass to the server command.
    pub args: Vec<String>,
    /// The child process handle, if the server is running.
    pub process: Option<Child>,
    /// Workspace root path for this server.
    pub root_path: PathBuf,
    /// Whether the server has completed LSP initialization.
    pub initialized: bool,
}

/// Manages multiple LSP server instances.
pub struct LspServerManager {
    /// Active server instances, keyed by language_id.
    servers: HashMap<String, LspServerInstance>,
    /// Maps file extensions to language identifiers.
    extension_map: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl LspServerManager {
    /// Create a new manager with default file-extension-to-language mappings.
    pub fn new() -> Self {
        let mut extension_map = HashMap::new();
        let mappings: &[(&str, &str)] = &[
            ("rs", "rust"),
            ("py", "python"),
            ("js", "javascript"),
            ("ts", "typescript"),
            ("tsx", "typescript"),
            ("jsx", "javascript"),
            ("go", "go"),
            ("java", "java"),
            ("c", "c"),
            ("h", "c"),
            ("cpp", "cpp"),
            ("cxx", "cpp"),
            ("cc", "cpp"),
            ("hpp", "cpp"),
            ("rb", "ruby"),
        ];
        for (ext, lang) in mappings {
            extension_map.insert(ext.to_string(), lang.to_string());
        }

        LspServerManager {
            servers: HashMap::new(),
            extension_map,
        }
    }

    /// Map a file path's extension to a language identifier.
    pub fn get_language_for_file(&self, path: &str) -> Option<String> {
        let ext = path.rsplit('.').next()?;
        self.extension_map.get(ext).cloned()
    }

    /// Return the (command, args) for a known LSP server.
    pub fn get_server_config(language_id: &str) -> Option<(String, Vec<String>)> {
        match language_id {
            "rust" => Some(("rust-analyzer".to_string(), vec![])),
            "python" => Some((
                "pyright-langserver".to_string(),
                vec!["--stdio".to_string()],
            )),
            "typescript" | "javascript" => Some((
                "typescript-language-server".to_string(),
                vec!["--stdio".to_string()],
            )),
            "go" => Some(("gopls".to_string(), vec!["serve".to_string()])),
            "java" => Some(("jdtls".to_string(), vec![])),
            "c" | "cpp" => Some(("clangd".to_string(), vec![])),
            "ruby" => Some(("solargraph".to_string(), vec!["stdio".to_string()])),
            _ => None,
        }
    }

    /// Start an LSP server for the given language in the specified workspace.
    pub fn start_server(&mut self, language_id: &str, root_path: PathBuf) -> Result<()> {
        // If already running, stop first
        if self.is_server_running(language_id) {
            self.stop_server(language_id)?;
        }

        let (command, args) = Self::get_server_config(language_id)
            .with_context(|| format!("No known LSP server for language: {}", language_id))?;

        let child = Command::new(&command)
            .args(&args)
            .current_dir(&root_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!("Failed to start LSP server: {} {}", command, args.join(" "))
            })?;

        let instance = LspServerInstance {
            language_id: language_id.to_string(),
            command,
            args,
            process: Some(child),
            root_path,
            initialized: false,
        };

        self.servers.insert(language_id.to_string(), instance);
        Ok(())
    }

    /// Stop the LSP server for the given language.
    pub fn stop_server(&mut self, language_id: &str) -> Result<()> {
        if let Some(instance) = self.servers.get_mut(language_id) {
            if let Some(ref mut child) = instance.process {
                let _ = child.kill();
                let _ = child.wait();
            }
            instance.process = None;
            instance.initialized = false;
        }
        Ok(())
    }

    /// Stop all running LSP servers.
    pub fn stop_all(&mut self) {
        let language_ids: Vec<String> = self.servers.keys().cloned().collect();
        for id in language_ids {
            let _ = self.stop_server(&id);
        }
    }

    /// Check if a server for the given language is currently running.
    pub fn is_server_running(&self, language_id: &str) -> bool {
        self.servers
            .get(language_id)
            .map(|s| s.process.is_some())
            .unwrap_or(false)
    }

    /// List all language IDs that have a running server.
    pub fn get_running_servers(&self) -> Vec<&str> {
        self.servers
            .iter()
            .filter(|(_, s)| s.process.is_some())
            .map(|(id, _)| id.as_str())
            .collect()
    }
}

impl Default for LspServerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LspServerManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_mapping_basic() {
        let mgr = LspServerManager::new();
        assert_eq!(
            mgr.get_language_for_file("main.rs"),
            Some("rust".to_string())
        );
        assert_eq!(
            mgr.get_language_for_file("app.py"),
            Some("python".to_string())
        );
        assert_eq!(
            mgr.get_language_for_file("index.js"),
            Some("javascript".to_string())
        );
        assert_eq!(
            mgr.get_language_for_file("index.ts"),
            Some("typescript".to_string())
        );
        assert_eq!(mgr.get_language_for_file("main.go"), Some("go".to_string()));
        assert_eq!(
            mgr.get_language_for_file("Main.java"),
            Some("java".to_string())
        );
        assert_eq!(mgr.get_language_for_file("util.c"), Some("c".to_string()));
        assert_eq!(
            mgr.get_language_for_file("util.cpp"),
            Some("cpp".to_string())
        );
        assert_eq!(
            mgr.get_language_for_file("app.rb"),
            Some("ruby".to_string())
        );
    }

    #[test]
    fn extension_mapping_unknown() {
        let mgr = LspServerManager::new();
        assert_eq!(mgr.get_language_for_file("data.csv"), None);
        assert_eq!(mgr.get_language_for_file("noext"), None);
    }

    #[test]
    fn extension_mapping_tsx_jsx() {
        let mgr = LspServerManager::new();
        assert_eq!(
            mgr.get_language_for_file("App.tsx"),
            Some("typescript".to_string())
        );
        assert_eq!(
            mgr.get_language_for_file("App.jsx"),
            Some("javascript".to_string())
        );
    }

    #[test]
    fn server_config_known_languages() {
        assert!(LspServerManager::get_server_config("rust").is_some());
        assert!(LspServerManager::get_server_config("python").is_some());
        assert!(LspServerManager::get_server_config("typescript").is_some());
        assert!(LspServerManager::get_server_config("javascript").is_some());
        assert!(LspServerManager::get_server_config("go").is_some());
        assert!(LspServerManager::get_server_config("c").is_some());
        assert!(LspServerManager::get_server_config("cpp").is_some());
        assert!(LspServerManager::get_server_config("ruby").is_some());
    }

    #[test]
    fn server_config_unknown_language() {
        assert!(LspServerManager::get_server_config("brainfuck").is_none());
        assert!(LspServerManager::get_server_config("").is_none());
    }

    #[test]
    fn manager_creation_no_running_servers() {
        let mgr = LspServerManager::new();
        assert!(mgr.get_running_servers().is_empty());
        assert!(!mgr.is_server_running("rust"));
    }

    #[test]
    fn server_config_rust_analyzer() {
        let (cmd, args) = LspServerManager::get_server_config("rust").unwrap();
        assert_eq!(cmd, "rust-analyzer");
        assert!(args.is_empty());
    }

    #[test]
    fn server_config_pyright() {
        let (cmd, args) = LspServerManager::get_server_config("python").unwrap();
        assert_eq!(cmd, "pyright-langserver");
        assert!(args.contains(&"--stdio".to_string()));
    }

    #[test]
    fn server_config_gopls() {
        let (cmd, args) = LspServerManager::get_server_config("go").unwrap();
        assert_eq!(cmd, "gopls");
        assert!(args.contains(&"serve".to_string()));
    }
}
