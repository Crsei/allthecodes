// test infrastructure — file search not wired to production TUI yet
//! File picker/search helpers for the Rust TUI boundary.

use std::io;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchMatch {
    pub path: PathBuf,
    pub display_path: String,
}

#[derive(Debug, Clone)]
pub struct FileSearchManager {
    root: PathBuf,
    max_results: usize,
}

impl FileSearchManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_results: 200,
        }
    }

    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results.max(1);
        self
    }

    pub fn search(&self, query: &str) -> io::Result<Vec<FileSearchMatch>> {
        search_files(&self.root, query, self.max_results)
    }
}

pub fn search_files(
    root: impl AsRef<Path>,
    query: &str,
    max_results: usize,
) -> io::Result<Vec<FileSearchMatch>> {
    let root = root.as_ref();
    let needle = normalize(query);
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for result in WalkBuilder::new(root).hidden(false).build() {
        let entry = result.map_err(io::Error::other)?;
        if !entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let display_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if normalize(&display_path).contains(&needle) {
            matches.push(FileSearchMatch {
                path: path.to_path_buf(),
                display_path,
            });
            if matches.len() >= max_results.max(1) {
                break;
            }
        }
    }
    Ok(matches)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn empty_query_returns_no_matches() {
        let matches = search_files(".", " ", 10).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn manager_searches_with_configured_limit() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
        std::fs::write(dir.path().join("alphabet.txt"), "").unwrap();

        let matches = FileSearchManager::new(dir.path())
            .with_max_results(1)
            .search("alp")
            .unwrap();

        assert_eq!(matches.len(), 1);
        assert!(matches[0].display_path.contains("alpha"));
    }
}
