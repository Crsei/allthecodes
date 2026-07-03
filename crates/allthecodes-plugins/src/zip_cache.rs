//! ZIP file caching and extraction utilities.
//!
//! Provides an LRU cache of downloaded plugin ZIP files and safe extraction
//! with path traversal protection.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use lru::LruCache;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::num::NonZeroUsize;
use std::sync::LazyLock;

/// Default cache capacity (number of ZIP files).
const DEFAULT_CACHE_CAPACITY: usize = 100;

/// Strict extraction limits for official plugin ZIP artifacts.
#[derive(Debug, Clone, Copy)]
pub struct StrictZipLimits {
    pub max_entries: usize,
    pub max_total_uncompressed_bytes: u64,
    pub max_file_uncompressed_bytes: u64,
}

impl Default for StrictZipLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_total_uncompressed_bytes: 512 * 1024 * 1024,
            max_file_uncompressed_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Thread-safe LRU cache for downloaded plugin ZIP data.
pub struct ZipCache {
    inner: Mutex<LruCache<String, Vec<u8>>>,
}

impl ZipCache {
    /// Create a new ZIP cache with the given capacity.
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(capacity)),
        }
    }

    /// Get cached entry by key.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.lock().get(key).cloned()
    }

    /// Insert entry into cache.
    pub fn put(&self, key: String, data: Vec<u8>) {
        self.inner.lock().put(key, data);
    }

    /// Check if key exists in cache.
    pub fn contains(&self, key: &str) -> bool {
        self.inner.lock().contains(key)
    }

    /// Clear all cached entries.
    pub fn clear(&self) {
        self.inner.lock().clear();
    }

    /// Current number of cached entries.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ZipCache {
    fn default() -> Self {
        Self::new(NonZeroUsize::new(DEFAULT_CACHE_CAPACITY).unwrap_or(NonZeroUsize::MIN))
    }
}

/// Global ZIP cache instance.
pub static GLOBAL_ZIP_CACHE: LazyLock<ZipCache> = LazyLock::new(ZipCache::default);

/// Compute a cache key from a source URL and optional checksum.
pub fn cache_key(url: &str, checksum: Option<&str>) -> String {
    match checksum {
        Some(cs) => format!("{}#{}", url, cs),
        None => {
            let mut hasher = Sha256::new();
            hasher.update(url.as_bytes());
            hex::encode(hasher.finalize())
        }
    }
}

/// Safe extraction of a ZIP archive to a destination directory.
///
/// This function protects against path traversal attacks by rejecting entries
/// that would escape the destination directory.
pub fn extract_zip_to(zip_data: &[u8], dest_dir: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open ZIP archive")?;

    std::fs::create_dir_all(dest_dir).with_context(|| {
        format!(
            "Failed to create destination directory: {}",
            dest_dir.display()
        )
    })?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Failed to read ZIP entry at index {}", i))?;

        let entry_name = match entry.enclosed_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => {
                anyhow::bail!(
                    "Path traversal detected: entry at index {} would escape destination '{}'",
                    i,
                    dest_dir.display()
                );
            }
        };

        let entry_path = dest_dir.join(&entry_name);

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .with_context(|| format!("Failed to create directory: {}", entry_path.display()))?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create parent directory: {}", parent.display())
                })?;
            }

            let mut output_file = std::fs::File::create(&entry_path)
                .with_context(|| format!("Failed to create file: {}", entry_path.display()))?;

            std::io::copy(&mut entry, &mut output_file)
                .with_context(|| format!("Failed to extract entry: {}", entry_name))?;
        }
    }

    Ok(())
}

/// Strict extraction for official plugin ZIP archives.
pub fn extract_zip_to_strict(
    zip_data: &[u8],
    dest_dir: &Path,
    limits: StrictZipLimits,
) -> Result<()> {
    let cursor = std::io::Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open ZIP archive")?;

    if archive.len() > limits.max_entries {
        anyhow::bail!(
            "ZIP archive has {} entries, exceeding limit {}",
            archive.len(),
            limits.max_entries
        );
    }

    std::fs::create_dir_all(dest_dir).with_context(|| {
        format!(
            "Failed to create destination directory: {}",
            dest_dir.display()
        )
    })?;
    let dest_root = dest_dir
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize {}", dest_dir.display()))?;

    let mut total_uncompressed = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Failed to read ZIP entry at index {}", i))?;
        let raw_name = entry.name().to_string();
        let relative = strict_zip_relative_path(&raw_name)
            .with_context(|| format!("Invalid ZIP entry path '{}'", raw_name))?;

        if is_zip_symlink(&entry) {
            anyhow::bail!("Refusing to extract symlink ZIP entry '{}'", raw_name);
        }

        let size = entry.size();
        if !entry.is_dir() && size > limits.max_file_uncompressed_bytes {
            anyhow::bail!(
                "ZIP entry '{}' expands to {} bytes, exceeding single-file limit {}",
                raw_name,
                size,
                limits.max_file_uncompressed_bytes
            );
        }
        total_uncompressed = total_uncompressed
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("ZIP archive expanded size overflow"))?;
        if total_uncompressed > limits.max_total_uncompressed_bytes {
            anyhow::bail!(
                "ZIP archive expands to {} bytes, exceeding total limit {}",
                total_uncompressed,
                limits.max_total_uncompressed_bytes
            );
        }

        let entry_path = dest_root.join(&relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .with_context(|| format!("Failed to create directory: {}", entry_path.display()))?;
            ensure_within_root(&dest_root, &entry_path)?;
            continue;
        }

        if let Some(parent) = entry_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }
        ensure_within_root(&dest_root, &entry_path)?;

        let mut output_file = std::fs::File::create(&entry_path)
            .with_context(|| format!("Failed to create file: {}", entry_path.display()))?;
        std::io::copy(&mut entry, &mut output_file)
            .with_context(|| format!("Failed to extract entry: {}", raw_name))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let mode = mode & 0o777;
            std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(mode))
                .with_context(|| {
                    format!("Failed to set permissions on {}", entry_path.display())
                })?;
        }
    }

    Ok(())
}

fn strict_zip_relative_path(raw_name: &str) -> Result<PathBuf> {
    if raw_name.trim().is_empty() {
        anyhow::bail!("empty ZIP entry path");
    }
    if raw_name.starts_with('/') || raw_name.starts_with('\\') {
        anyhow::bail!("absolute ZIP entry path");
    }
    if raw_name.starts_with("//") || raw_name.starts_with("\\\\") {
        anyhow::bail!("UNC ZIP entry path");
    }
    let bytes = raw_name.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        anyhow::bail!("Windows drive-prefixed ZIP entry path");
    }

    let mut out = PathBuf::new();
    for part in raw_name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => anyhow::bail!("ZIP entry contains parent directory component"),
            component => out.push(component),
        }
    }
    if out.as_os_str().is_empty() {
        anyhow::bail!("empty normalized ZIP entry path");
    }
    Ok(out)
}

fn ensure_within_root(root: &Path, target: &Path) -> Result<()> {
    let parent = if target.extension().is_some() || !target.exists() {
        target.parent().unwrap_or(root)
    } else {
        target
    };
    let canonical_parent = parent
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize {}", parent.display()))?;
    if !canonical_parent.starts_with(root) {
        anyhow::bail!(
            "ZIP entry would escape destination directory: {}",
            target.display()
        );
    }
    Ok(())
}

fn is_zip_symlink(entry: &zip::read::ZipFile<'_>) -> bool {
    entry.is_symlink()
}

/// Safe extraction of a gzip-compressed tar archive to a destination directory.
///
/// This intentionally implements only the regular-file/directory subset used
/// by npm plugin packages and rejects absolute paths, `..`, symlinks, hard
/// links, and special entries.
pub fn extract_tgz_to(tgz_data: &[u8], dest_dir: &Path) -> Result<()> {
    let mut decoder = GzDecoder::new(tgz_data);
    let mut tar_data = Vec::new();
    decoder
        .read_to_end(&mut tar_data)
        .context("Failed to decompress gzip archive")?;

    std::fs::create_dir_all(dest_dir).with_context(|| {
        format!(
            "Failed to create destination directory: {}",
            dest_dir.display()
        )
    })?;

    let mut offset = 0usize;
    while offset + 512 <= tar_data.len() {
        let header = &tar_data[offset..offset + 512];
        offset += 512;

        if header.iter().all(|b| *b == 0) {
            break;
        }

        let path = tar_header_path(header)?;
        let size = tar_octal(&header[124..136])?;
        let typeflag = header[156];
        let padded = size.div_ceil(512) * 512;
        if offset + padded > tar_data.len() {
            anyhow::bail!("Invalid tar archive: entry '{}' exceeds archive size", path);
        }
        let payload = &tar_data[offset..offset + size];
        offset += padded;

        let relative = safe_tar_path(&path)?;
        let target = dest_dir.join(relative);
        match typeflag {
            b'0' | 0 => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create parent directory: {}", parent.display())
                    })?;
                }
                std::fs::write(&target, payload)
                    .with_context(|| format!("Failed to write tar entry: {}", target.display()))?;
            }
            b'5' => {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("Failed to create directory: {}", target.display()))?;
            }
            b'1' | b'2' => {
                anyhow::bail!("Refusing to extract link entry from tar archive: {}", path);
            }
            _ => {
                if size != 0 {
                    anyhow::bail!(
                        "Unsupported tar entry type '{}' for {}",
                        typeflag as char,
                        path
                    );
                }
            }
        }
    }

    Ok(())
}

fn tar_header_path(header: &[u8]) -> Result<String> {
    let name = tar_string(&header[0..100]);
    let prefix = tar_string(&header[345..500]);
    let path = if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    };
    if path.trim().is_empty() {
        anyhow::bail!("Invalid tar archive: empty entry path");
    }
    Ok(path)
}

fn tar_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn tar_octal(bytes: &[u8]) -> Result<usize> {
    let raw = tar_string(bytes);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    usize::from_str_radix(trimmed, 8).with_context(|| format!("Invalid tar octal field: {trimmed}"))
}

fn safe_tar_path(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        anyhow::bail!(
            "Path traversal detected: absolute tar path {}",
            path.display()
        );
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            _ => anyhow::bail!("Path traversal detected in tar path {}", path.display()),
        }
    }
    if out.as_os_str().is_empty() {
        anyhow::bail!("Invalid tar path {}", path.display());
    }
    Ok(out)
}

/// Compute SHA-256 hash of a file.
pub fn hash_zip(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read file for hashing: {}", path.display()))?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Get the default plugin cache directory path.
pub fn cache_dir() -> PathBuf {
    crate::cache_dir().join("downloads")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_zip_cache_basic() {
        let cache = ZipCache::new(NonZeroUsize::new(10).unwrap());
        assert!(cache.is_empty());

        cache.put("key1".to_string(), vec![1, 2, 3]);
        assert!(cache.contains("key1"));
        assert_eq!(cache.get("key1"), Some(vec![1, 2, 3]));
        assert!(!cache.contains("key2"));

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_key() {
        let key1 = cache_key("https://example.com/plugin.zip", Some("abc123"));
        let key2 = cache_key("https://example.com/plugin.zip", Some("abc123"));
        let key3 = cache_key("https://example.com/plugin.zip", Some("def456"));
        let key4 = cache_key("https://different.com/pkg.zip", None);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert!(key4.len() == 64 || key4.len() > 32); // SHA-256 hex
    }

    #[test]
    fn test_extract_zip_to_rejects_path_traversal() {
        // Create a malicious ZIP that attempts path traversal
        // We need to construct it programmatically

        let mut zip_buf = Vec::new();
        {
            let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let options = zip::write::FileOptions::<()>::default();
            zip_writer
                .start_file("../evil.sh", options)
                .expect("start file");
            zip_writer.write_all(b"malicious content").expect("write");
            zip_writer.finish().expect("finish");
        }

        let dest = tempfile::tempdir().unwrap();
        let result = extract_zip_to(&zip_buf, dest.path());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Path traversal"),
            "expected path traversal error"
        );
    }

    #[test]
    fn test_extract_zip_to_empty_archive() {
        let mut zip_buf = Vec::new();
        {
            let zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            zip_writer.finish().expect("finish");
        }

        let dest = tempfile::tempdir().unwrap();
        let result = extract_zip_to(&zip_buf, dest.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_zip_to_invalid_zip() {
        let invalid_data = b"this is not a zip file";
        let dest = tempfile::tempdir().unwrap();
        let result = extract_zip_to(invalid_data, dest.path());
        assert!(result.is_err());
    }

    #[test]
    fn strict_zip_rejects_windows_drive_prefix() {
        let mut zip_buf = Vec::new();
        {
            let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let options = zip::write::FileOptions::<()>::default();
            zip_writer.start_file("C:/evil.sh", options).unwrap();
            zip_writer.write_all(b"evil").unwrap();
            zip_writer.finish().unwrap();
        }

        let dest = tempfile::tempdir().unwrap();
        let result = extract_zip_to_strict(&zip_buf, dest.path(), StrictZipLimits::default());
        assert!(result.is_err());
    }

    #[test]
    fn strict_zip_rejects_symlink_entries() {
        let mut zip_buf = Vec::new();
        {
            let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let options = zip::write::FileOptions::<()>::default();
            zip_writer.add_symlink("link", "target", options).unwrap();
            zip_writer.finish().unwrap();
        }

        let dest = tempfile::tempdir().unwrap();
        let result = extract_zip_to_strict(&zip_buf, dest.path(), StrictZipLimits::default());
        assert!(result.is_err());
    }

    #[test]
    fn strict_zip_enforces_entry_count_limit() {
        let mut zip_buf = Vec::new();
        {
            let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let options = zip::write::FileOptions::<()>::default();
            zip_writer.start_file("a", options).unwrap();
            zip_writer.write_all(b"a").unwrap();
            zip_writer.start_file("b", options).unwrap();
            zip_writer.write_all(b"b").unwrap();
            zip_writer.finish().unwrap();
        }

        let dest = tempfile::tempdir().unwrap();
        let result = extract_zip_to_strict(
            &zip_buf,
            dest.path(),
            StrictZipLimits {
                max_entries: 1,
                ..StrictZipLimits::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_zip_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.zip");
        std::fs::write(&path, b"content").unwrap();

        let hash = hash_zip(&path).unwrap();
        assert_eq!(hash.len(), 64);

        let hash2 = hash_zip(&path).unwrap();
        assert_eq!(hash, hash2);
    }
}
