//! MCPB (Marketplace Plugin Binary) format.
//!
//! MCPB is a custom binary format for distributing plugins. The format is:
//! - 4-byte magic header: "MCPB"
//! - 2-byte format version (u16 LE)
//! - 4-byte manifest JSON length (u32 LE)
//! - Manifest JSON bytes
//! - 32-byte SHA-256 content hash (over the manifest + payload)
//! - Payload bytes (typically a ZIP archive)

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Magic bytes identifying an MCPB file.
const MCPB_MAGIC: &[u8; 4] = b"MCPB";

/// Current MCPB format version.
const MCPB_VERSION: u16 = 1;

/// Length of the SHA-256 hash in bytes.
const HASH_LENGTH: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct McpbLimits {
    pub max_bundle_bytes: usize,
    pub max_manifest_bytes: usize,
    pub max_payload_bytes: usize,
}

impl Default for McpbLimits {
    fn default() -> Self {
        Self {
            max_bundle_bytes: 128 * 1024 * 1024,
            max_manifest_bytes: 1024 * 1024,
            max_payload_bytes: 128 * 1024 * 1024,
        }
    }
}

/// Parsed MCPB bundle.
#[derive(Debug, Clone)]
pub struct McpbBundle {
    /// Format version.
    pub version: u16,
    /// Plugin manifest parsed from the bundle header.
    pub manifest: serde_json::Value,
    /// Raw manifest bytes as stored in the bundle (for hash verification).
    pub raw_manifest: Vec<u8>,
    /// Raw payload bytes (usually a ZIP archive).
    pub payload: Vec<u8>,
    /// Expected content hash (hex-encoded).
    pub expected_hash: String,
}

/// Parse an MCPB binary file from a path.
pub fn parse_mcpb(path: &Path) -> Result<McpbBundle> {
    let limits = McpbLimits::default();
    let data = read_mcpb_file_with_limits(path, limits)?;
    parse_mcpb_from_bytes_with_limits(&data, limits)
}

fn read_mcpb_file_with_limits(path: &Path, limits: McpbLimits) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open MCPB file: {}", path.display()))?;
    let size = file
        .metadata()
        .with_context(|| format!("Failed to inspect MCPB file: {}", path.display()))?
        .len();
    if size > limits.max_bundle_bytes as u64 {
        anyhow::bail!("MCPB bundle exceeds size limit {}", limits.max_bundle_bytes);
    }
    let mut data = Vec::with_capacity(size as usize);
    std::io::Read::by_ref(&mut file)
        .take(limits.max_bundle_bytes as u64 + 1)
        .read_to_end(&mut data)
        .with_context(|| format!("Failed to read MCPB file: {}", path.display()))?;
    if data.len() > limits.max_bundle_bytes {
        anyhow::bail!("MCPB bundle exceeds size limit {}", limits.max_bundle_bytes);
    }
    Ok(data)
}

/// Parse an MCPB bundle from raw bytes.
pub fn parse_mcpb_from_bytes(data: &[u8]) -> Result<McpbBundle> {
    parse_mcpb_from_bytes_with_limits(data, McpbLimits::default())
}

pub fn parse_mcpb_from_bytes_with_limits(data: &[u8], limits: McpbLimits) -> Result<McpbBundle> {
    if data.len() > limits.max_bundle_bytes {
        anyhow::bail!("MCPB bundle exceeds size limit {}", limits.max_bundle_bytes);
    }
    let mut cursor = std::io::Cursor::new(data);

    // Read magic
    let mut magic = [0u8; 4];
    cursor
        .read_exact(&mut magic)
        .context("Failed to read MCPB magic bytes")?;
    if &magic != MCPB_MAGIC {
        anyhow::bail!("Invalid MCPB magic: expected 'MCPB', got {:?}", magic);
    }

    // Read version (u16 LE)
    let mut version_buf = [0u8; 2];
    cursor
        .read_exact(&mut version_buf)
        .context("Failed to read MCPB version")?;
    let version = u16::from_le_bytes(version_buf);

    if version != MCPB_VERSION {
        anyhow::bail!(
            "Unsupported MCPB version: got {}, expected {}",
            version,
            MCPB_VERSION
        );
    }

    // Read manifest length (u32 LE)
    let mut manifest_len_buf = [0u8; 4];
    cursor
        .read_exact(&mut manifest_len_buf)
        .context("Failed to read MCPB manifest length")?;
    let manifest_len = u32::from_le_bytes(manifest_len_buf) as usize;
    if manifest_len > limits.max_manifest_bytes {
        anyhow::bail!(
            "MCPB manifest length {} exceeds limit {}",
            manifest_len,
            limits.max_manifest_bytes
        );
    }
    let minimum = 4usize + 2 + 4 + manifest_len + HASH_LENGTH;
    if minimum > data.len() {
        anyhow::bail!("MCPB manifest length exceeds available bundle data");
    }
    let payload_len = data.len() - minimum;
    if payload_len > limits.max_payload_bytes {
        anyhow::bail!(
            "MCPB payload exceeds size limit {}",
            limits.max_payload_bytes
        );
    }

    // Read manifest JSON
    let mut manifest_bytes = vec![0u8; manifest_len];
    cursor
        .read_exact(&mut manifest_bytes)
        .context("Failed to read MCPB manifest")?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).context("Failed to parse MCPB manifest JSON")?;

    // Read expected hash (32 bytes)
    let mut expected_hash_bytes = [0u8; HASH_LENGTH];
    cursor
        .read_exact(&mut expected_hash_bytes)
        .context("Failed to read MCPB content hash")?;
    let expected_hash = hex::encode(expected_hash_bytes);

    // Read remaining bytes as payload
    let mut payload = Vec::new();
    cursor
        .read_to_end(&mut payload)
        .context("Failed to read MCPB payload")?;

    Ok(McpbBundle {
        version,
        manifest,
        raw_manifest: manifest_bytes,
        payload,
        expected_hash,
    })
}

/// Create an MCPB bundle from a plugin directory.
pub fn create_mcpb(plugin_dir: &Path, output_path: &Path) -> Result<()> {
    // Read plugin.json as manifest
    let manifest_path = plugin_dir.join("plugin.json");
    let manifest_bytes = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read manifest: {}", manifest_path.display()))?;

    // Create payload: walk the plugin directory and create a ZIP
    let payload = create_directory_zip(plugin_dir)?;

    // Content hash covers manifest + payload
    let mut hasher = Sha256::new();
    hasher.update(manifest_bytes.as_bytes());
    hasher.update(&payload);
    let content_hash = hasher.finalize();

    // Write bundle
    let mut output = std::fs::File::create(output_path)
        .with_context(|| format!("Failed to create MCPB output: {}", output_path.display()))?;

    // Magic
    output.write_all(MCPB_MAGIC)?;
    // Version
    output.write_all(&MCPB_VERSION.to_le_bytes())?;
    // Manifest length
    let manifest_len = manifest_bytes.len() as u32;
    output.write_all(&manifest_len.to_le_bytes())?;
    // Manifest bytes
    output.write_all(manifest_bytes.as_bytes())?;
    // Content hash
    output.write_all(&content_hash)?;
    // Payload
    output.write_all(&payload)?;

    Ok(())
}

/// Verify the integrity of an MCPB file by recomputing the content hash.
pub fn verify_mcpb_integrity(path: &Path) -> Result<bool> {
    let limits = McpbLimits::default();
    let data = read_mcpb_file_with_limits(path, limits)?;

    // Parse to extract the expected hash and the manifest+payload portion
    let bundle = parse_mcpb_from_bytes_with_limits(&data, limits)?;

    // Reconstruct the data that was hashed: raw manifest bytes + payload
    let mut hasher = Sha256::new();
    hasher.update(&bundle.raw_manifest);
    hasher.update(&bundle.payload);
    let computed_hash = hex::encode(hasher.finalize());

    Ok(computed_hash == bundle.expected_hash)
}

/// Create a ZIP archive of a directory's contents (without the top-level dir).
fn create_directory_zip(dir: &Path) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));

    let options = zip::write::FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated);

    add_dir_to_zip(&mut zip_writer, dir, dir, &options)?;

    zip_writer
        .finish()
        .context("Failed to finalize ZIP archive")?;
    Ok(buf)
}

fn add_dir_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    dir: &Path,
    options: &zip::write::FileOptions<()>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if path.is_dir() {
            zip.add_directory(&relative, *options)
                .with_context(|| format!("Failed to add directory to ZIP: {}", relative))?;
            add_dir_to_zip(zip, base, &path, options)?;
        } else {
            let data = std::fs::read(&path)
                .with_context(|| format!("Failed to read file: {}", path.display()))?;
            zip.start_file(&relative, *options)
                .with_context(|| format!("Failed to add file to ZIP: {}", relative))?;
            std::io::copy(&mut std::io::Cursor::new(data), zip)
                .context("Failed to write file content to ZIP")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invalid_magic() {
        let data = b"NOTMCPB...";
        let result = parse_mcpb_from_bytes(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("MCPB magic"));
    }

    #[test]
    fn test_parse_truncated_data() {
        let data = b"MCPB";
        let result = parse_mcpb_from_bytes(data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_manifest_length_over_limit_before_allocation() {
        let mut data = b"MCPB".to_vec();
        data.extend_from_slice(&MCPB_VERSION.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        let error = parse_mcpb_from_bytes_with_limits(&data, McpbLimits::default()).unwrap_err();
        assert!(error.to_string().contains("manifest length"));
    }

    #[test]
    fn test_create_and_verify_mcpb() {
        let dir = tempfile::tempdir().unwrap();

        // Create a minimal plugin directory
        let manifest = serde_json::json!({
            "name": "test-plugin",
            "version": "1.0.0",
            "description": "Test plugin"
        });
        std::fs::write(
            dir.path().join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();
        std::fs::write(dir.path().join("skills/hello.md"), b"# Hello").unwrap();

        let output = dir.path().join("bundle.mcpb");
        create_mcpb(dir.path(), &output).unwrap();

        assert!(output.exists());

        // Verify integrity
        let valid = verify_mcpb_integrity(&output).unwrap();
        assert!(valid);

        // Parse and check manifest
        let bundle = parse_mcpb(&output).unwrap();
        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.manifest["name"], "test-plugin");
        assert!(!bundle.payload.is_empty());
    }

    #[test]
    fn test_verify_corrupted_mcpb_fails() {
        let dir = tempfile::tempdir().unwrap();

        let manifest = serde_json::json!({
            "name": "test-plugin",
            "version": "1.0.0"
        });
        std::fs::write(
            dir.path().join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let output = dir.path().join("bundle.mcpb");
        create_mcpb(dir.path(), &output).unwrap();

        // Corrupt the file
        let mut data = std::fs::read(&output).unwrap();
        let last_byte = data.len() - 1;
        data[last_byte] ^= 0xFF;
        std::fs::write(&output, &data).unwrap();

        let valid = verify_mcpb_integrity(&output).unwrap();
        assert!(!valid);
    }
}
