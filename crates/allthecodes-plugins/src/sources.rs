//! Plugin source resolver — download plugins from various sources.
//!
//! Supports URL, GitHub, npm, and local file sources.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Plugin source type for downloading/resolving.
#[derive(Debug, Clone)]
pub enum ResolveSource {
    /// HTTP(S) URL.
    Url(String),
    /// GitHub repository (owner/repo format).
    GitHub { repo: String, tag: Option<String> },
    /// npm package.
    Npm {
        package: String,
        version: Option<String>,
    },
    /// Local file path.
    File(PathBuf),
}

/// Result of resolving a plugin source.
#[derive(Debug, Clone)]
pub struct ResolvedPlugin {
    /// Path to the downloaded/extracted plugin directory or file.
    pub path: PathBuf,
    /// Content hash (SHA-256) of the downloaded content.
    pub checksum: String,
    /// Original source URL (for display/record-keeping).
    pub source_url: String,
    /// File extension hint (e.g., "zip", "tar.gz", "mcpb").
    pub extension: String,
}

/// Resolve a plugin from its source and download it to a cache location.
pub async fn resolve_source(source: &ResolveSource, dest_dir: &Path) -> Result<ResolvedPlugin> {
    match source {
        ResolveSource::Url(url) => resolve_url_source(url, dest_dir).await,
        ResolveSource::GitHub { repo, tag } => {
            resolve_github_source(repo, tag.as_deref(), dest_dir).await
        }
        ResolveSource::Npm { package, version } => {
            resolve_npm_source(package, version.as_deref(), dest_dir).await
        }
        ResolveSource::File(path) => resolve_file_source(path, dest_dir),
    }
}

/// Resolve an HTTP(S) URL source.
async fn resolve_url_source(url: &str, dest_dir: &Path) -> Result<ResolvedPlugin> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to fetch URL: {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {} when fetching URL: {}", response.status(), url);
    }

    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    let checksum = hash_bytes(&bytes);
    let extension = infer_extension(url, &bytes);
    let filename = format!("plugin.{}", extension);
    let dest_path = dest_dir.join(&filename);

    tokio::fs::create_dir_all(dest_dir)
        .await
        .context("Failed to create destination directory")?;
    tokio::fs::write(&dest_path, &bytes)
        .await
        .with_context(|| format!("Failed to write to {}", dest_path.display()))?;

    Ok(ResolvedPlugin {
        path: dest_path,
        checksum,
        source_url: url.to_string(),
        extension,
    })
}

/// Resolve a GitHub release source.
async fn resolve_github_source(
    repo: &str,
    tag: Option<&str>,
    dest_dir: &Path,
) -> Result<ResolvedPlugin> {
    let release_tag = tag.unwrap_or("latest");
    // GitHub release archive URL
    let url = format!(
        "https://api.github.com/repos/{repo}/zipball/{tag}",
        repo = repo,
        tag = release_tag
    );

    let client = reqwest::Client::builder()
        .user_agent("allthecodes-plugin-manager/0.1")
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch GitHub release: {}", url))?;

    if !response.status().is_success() {
        // Try the simpler archive URL as fallback
        let fallback_url = format!(
            "https://github.com/{repo}/archive/refs/tags/{tag}.zip",
            repo = repo,
            tag = release_tag
        );
        let fallback_response = client
            .get(&fallback_url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch GitHub archive: {}", fallback_url))?;

        if !fallback_response.status().is_success() {
            anyhow::bail!(
                "GitHub source {}@{} returned HTTP {} (tried API and archive URLs)",
                repo,
                release_tag,
                fallback_response.status()
            );
        }

        let bytes = fallback_response
            .bytes()
            .await
            .context("Failed to read GitHub archive response")?;
        let checksum = hash_bytes(&bytes);
        let dest_path = dest_dir.join("plugin.zip");

        tokio::fs::create_dir_all(dest_dir)
            .await
            .context("Failed to create destination directory")?;
        tokio::fs::write(&dest_path, &bytes)
            .await
            .with_context(|| format!("Failed to write to {}", dest_path.display()))?;

        return Ok(ResolvedPlugin {
            path: dest_path,
            checksum,
            source_url: fallback_url,
            extension: "zip".to_string(),
        });
    }

    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    let checksum = hash_bytes(&bytes);
    let dest_path = dest_dir.join("plugin.zip");

    tokio::fs::create_dir_all(dest_dir)
        .await
        .context("Failed to create destination directory")?;
    tokio::fs::write(&dest_path, &bytes)
        .await
        .with_context(|| format!("Failed to write to {}", dest_path.display()))?;

    Ok(ResolvedPlugin {
        path: dest_path,
        checksum,
        source_url: url,
        extension: "zip".to_string(),
    })
}

/// Resolve an npm package source.
async fn resolve_npm_source(
    package: &str,
    version: Option<&str>,
    dest_dir: &Path,
) -> Result<ResolvedPlugin> {
    let version_str = version.unwrap_or("latest");
    let encoded_pkg = url::form_urlencoded::byte_serialize(package.as_bytes()).collect::<String>();
    let url = format!(
        "https://registry.npmjs.org/{package}/{version}",
        package = encoded_pkg,
        version = version_str
    );

    let response = reqwest::get(&url)
        .await
        .with_context(|| format!("Failed to fetch npm package: {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "npm registry returned HTTP {} for {}",
            response.status(),
            url
        );
    }

    let metadata_bytes = response
        .bytes()
        .await
        .context("Failed to read npm response body")?;

    let tarball_url = npm_tarball_url_from_metadata(&metadata_bytes)
        .context("Failed to resolve npm package tarball URL from registry metadata")?;

    let tarball_response = reqwest::get(&tarball_url)
        .await
        .with_context(|| format!("Failed to fetch npm tarball: {}", tarball_url))?;

    if !tarball_response.status().is_success() {
        anyhow::bail!(
            "npm tarball returned HTTP {} for {}",
            tarball_response.status(),
            tarball_url
        );
    }

    let bytes = tarball_response
        .bytes()
        .await
        .context("Failed to read npm tarball body")?;

    let checksum = hash_bytes(&bytes);
    let dest_path = dest_dir.join("package.tgz");

    tokio::fs::create_dir_all(dest_dir)
        .await
        .context("Failed to create destination directory")?;
    tokio::fs::write(&dest_path, &bytes)
        .await
        .with_context(|| format!("Failed to write to {}", dest_path.display()))?;

    Ok(ResolvedPlugin {
        path: dest_path,
        checksum,
        source_url: tarball_url,
        extension: "tgz".to_string(),
    })
}

#[derive(serde::Deserialize)]
struct NpmVersionMetadata {
    dist: NpmDistMetadata,
}

#[derive(serde::Deserialize)]
struct NpmDistMetadata {
    tarball: String,
}

fn npm_tarball_url_from_metadata(bytes: &[u8]) -> Result<String> {
    let metadata: NpmVersionMetadata =
        serde_json::from_slice(bytes).context("Failed to parse npm version metadata")?;
    if metadata.dist.tarball.trim().is_empty() {
        anyhow::bail!("npm metadata dist.tarball is empty");
    }
    Ok(metadata.dist.tarball)
}

/// Resolve a local file or directory source.
pub fn resolve_file_source(path: &Path, dest_dir: &Path) -> Result<ResolvedPlugin> {
    if !path.exists() {
        anyhow::bail!("Local plugin path does not exist: {}", path.display());
    }

    if path.is_dir() {
        let dest_path = dest_dir.join("extracted");
        if dest_path.exists() {
            std::fs::remove_dir_all(&dest_path)
                .with_context(|| format!("Failed to clear {}", dest_path.display()))?;
        }
        copy_dir_all(path, &dest_path)?;
        return Ok(ResolvedPlugin {
            path: dest_path,
            checksum: hash_directory(path)?,
            source_url: path.to_string_lossy().to_string(),
            extension: "dir".to_string(),
        });
    }

    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read local file: {}", path.display()))?;

    let checksum = hash_bytes(&bytes);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("zip")
        .to_string();
    let filename = format!("plugin.{}", extension);
    let dest_path = dest_dir.join(&filename);

    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;
    std::fs::copy(path, &dest_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            path.display(),
            dest_path.display()
        )
    })?;

    Ok(ResolvedPlugin {
        path: dest_path,
        checksum,
        source_url: path.to_string_lossy().to_string(),
        extension,
    })
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory: {}", dst.display()))?;
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn hash_directory(path: &Path) -> Result<String> {
    let mut entries = Vec::new();
    collect_dir_hash_entries(path, path, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (relative, bytes) in entries {
        hasher.update(relative.as_bytes());
        hasher.update(&bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_dir_hash_entries(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_dir_hash_entries(root, &path, out)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            out.push((relative, bytes));
        }
    }
    Ok(())
}

/// Compute SHA-256 hash of bytes.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Infer file extension from URL or content.
fn infer_extension(url: &str, _bytes: &[u8]) -> String {
    let url_lower = url.to_lowercase();
    if url_lower.ends_with(".zip") {
        "zip".to_string()
    } else if url_lower.ends_with(".mcpb") {
        "mcpb".to_string()
    } else if url_lower.ends_with(".tar.gz") || url_lower.ends_with(".tgz") {
        "tgz".to_string()
    } else {
        "zip".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_bytes() {
        let h1 = hash_bytes(b"hello");
        let h2 = hash_bytes(b"hello");
        let h3 = hash_bytes(b"world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_infer_extension() {
        assert_eq!(
            infer_extension("https://example.com/plugin.zip", b""),
            "zip"
        );
        assert_eq!(
            infer_extension("https://example.com/plugin.mcpb", b""),
            "mcpb"
        );
        assert_eq!(
            infer_extension("https://example.com/plugin.tar.gz", b""),
            "tgz"
        );
        assert_eq!(infer_extension("https://example.com/plugin", b""), "zip");
    }

    #[test]
    fn test_resolve_file_source_missing() {
        let result =
            resolve_file_source(Path::new("/nonexistent/path/plugin.zip"), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_file_source_success() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("test-plugin.zip");
        std::fs::write(&src, b"fake zip content").unwrap();

        let dest = dir.path().join("dest");
        let result = resolve_file_source(&src, &dest).unwrap();

        assert_eq!(result.extension, "zip");
        assert!(result.path.exists());
        let checksum = hash_bytes(b"fake zip content");
        assert_eq!(result.checksum, checksum);
    }

    #[test]
    fn test_resolve_local_directory_source_success() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("plugin-dir");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("plugin.json"),
            r#"{"name":"local-plugin","version":"1.0.0"}"#,
        )
        .unwrap();

        let dest = dir.path().join("dest");
        let result = resolve_file_source(&src, &dest).unwrap();

        assert_eq!(result.extension, "dir");
        assert!(result.path.join("plugin.json").exists());
    }

    #[test]
    fn npm_metadata_uses_dist_tarball() {
        let metadata = br#"{
            "name": "pkg",
            "version": "1.0.0",
            "dist": {
                "tarball": "https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz"
            }
        }"#;

        let url = npm_tarball_url_from_metadata(metadata).unwrap();
        assert_eq!(url, "https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz");
    }
}
