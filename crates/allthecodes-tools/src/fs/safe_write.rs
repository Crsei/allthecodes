use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use allthecodes_config::constants::files::{
    has_binary_extension, is_binary_content, BINARY_CHECK_SIZE,
};

pub const DEFAULT_MAX_WRITE_BYTES: usize = 10 * 1024 * 1024;

const TEMP_MARKER: &str = ".ccwrite.";
const DEFAULT_SESSION_ID: &str = "unknown-session";
const MAX_RECOVERY_BACKUPS_PER_FILE: usize = 20;

#[derive(Debug, Clone)]
pub struct SafeWriteOptions {
    pub max_bytes: usize,
    pub session_id: Option<String>,
    pub recovery_root: Option<PathBuf>,
    #[cfg(test)]
    pub fail_after_temp_write: bool,
}

impl Default for SafeWriteOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_WRITE_BYTES,
            session_id: None,
            recovery_root: None,
            #[cfg(test)]
            fail_after_temp_write: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SafeWriteReport {
    pub requested_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub bytes_written: usize,
    pub line_count: usize,
    pub created: bool,
    pub symlink_resolved: bool,
    pub permissions_preserved: bool,
}

pub fn safe_write_text(
    path: impl AsRef<Path>,
    content: &str,
    options: &SafeWriteOptions,
) -> Result<SafeWriteReport> {
    let requested_path = path.as_ref().to_path_buf();
    validate_write_request(&requested_path, content.as_bytes(), options.max_bytes)?;

    let (target_path, symlink_resolved) = resolve_symlink_target(&requested_path)
        .with_context(|| format!("Failed to resolve {}", requested_path.display()))?;
    let parent = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .context("File path must include a parent directory")?;

    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create directories for {}", parent.display()))?;

    let target_metadata = existing_file_metadata(&target_path)?;
    if let Some(metadata) = &target_metadata {
        if metadata.is_dir() {
            bail!(
                "Path is a directory, not a file: {}",
                requested_path.display()
            );
        }
        if metadata.permissions().readonly() {
            bail!(
                "Refusing to overwrite readonly file: {}",
                requested_path.display()
            );
        }
        if metadata.len() > options.max_bytes as u64 {
            bail!(
                "Refusing to overwrite file larger than safe write limit: {} bytes > {} bytes",
                metadata.len(),
                options.max_bytes
            );
        }
        reject_existing_binary_file(&target_path)?;
    }

    let created = target_metadata.is_none();
    let backup_path = if created {
        None
    } else {
        Some(create_recovery_backup(
            &target_path,
            &target_metadata,
            options,
        )?)
    };

    let temp_path = create_unique_temp_path(parent, &target_path)?;
    let write_result = write_temp_and_replace(
        &temp_path,
        &target_path,
        content.as_bytes(),
        target_metadata.as_ref(),
        options,
    );

    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        if let (Some(backup), Some(metadata)) = (&backup_path, target_metadata.as_ref()) {
            let _ = restore_backup_if_target_missing(&target_path, backup, metadata);
        }
        return Err(err);
    }

    Ok(SafeWriteReport {
        requested_path,
        target_path,
        backup_path,
        bytes_written: content.len(),
        line_count: content.lines().count(),
        created,
        symlink_resolved,
        permissions_preserved: !created,
    })
}

pub fn validate_write_request(path: &Path, content: &[u8], max_bytes: usize) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("file_path is required");
    }
    if content.len() > max_bytes {
        bail!(
            "Refusing to write oversized content: {} bytes > {} bytes",
            content.len(),
            max_bytes
        );
    }
    if has_binary_extension(&path.to_string_lossy()) {
        bail!(
            "Refusing to write text content to known binary file type: {}",
            path.display()
        );
    }
    if is_binary_content(content) {
        bail!("Refusing to write binary-looking content with the text Write tool");
    }
    Ok(())
}

fn existing_file_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Failed to stat {}", path.display())),
    }
}

fn reject_existing_binary_file(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to inspect {}", path.display()))?;
    let mut sample = vec![0_u8; BINARY_CHECK_SIZE];
    let n = std::io::Read::read(&mut file, &mut sample)
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    sample.truncate(n);
    if is_binary_content(&sample) {
        bail!(
            "Refusing to overwrite binary-looking file: {}",
            path.display()
        );
    }
    Ok(())
}

fn resolve_symlink_target(path: &Path) -> Result<(PathBuf, bool)> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let link_target = fs::read_link(path)
                .with_context(|| format!("Failed to read symlink {}", path.display()))?;
            let resolved = if link_target.is_absolute() {
                link_target
            } else {
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(link_target)
            };
            Ok((resolved, true))
        }
        Ok(_) => Ok((path.to_path_buf(), false)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok((path.to_path_buf(), false)),
        Err(err) => Err(err).with_context(|| format!("Failed to stat {}", path.display())),
    }
}

fn write_temp_and_replace(
    temp_path: &Path,
    target_path: &Path,
    content: &[u8],
    target_metadata: Option<&fs::Metadata>,
    options: &SafeWriteOptions,
) -> Result<()> {
    #[cfg(not(test))]
    let _ = options;

    let mut open_options = OpenOptions::new();
    open_options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if target_metadata.is_none() {
            open_options.mode(0o644);
        }
    }

    let mut temp_file = open_options
        .open(temp_path)
        .with_context(|| format!("Failed to create temp file {}", temp_path.display()))?;

    temp_file
        .write_all(content)
        .with_context(|| format!("Failed to write temp file {}", temp_path.display()))?;
    temp_file
        .flush()
        .with_context(|| format!("Failed to flush temp file {}", temp_path.display()))?;

    if let Some(metadata) = target_metadata {
        fs::set_permissions(temp_path, metadata.permissions()).with_context(|| {
            format!(
                "Failed to preserve permissions on temp file {}",
                temp_path.display()
            )
        })?;
    }

    temp_file
        .sync_all()
        .with_context(|| format!("Failed to sync temp file {}", temp_path.display()))?;
    drop(temp_file);

    #[cfg(test)]
    if options.fail_after_temp_write {
        bail!("Injected safe write failure after temp file sync");
    }

    atomic_replace(temp_path, target_path).with_context(|| {
        format!(
            "Failed to atomically replace {} with {}",
            target_path.display(),
            temp_path.display()
        )
    })?;
    sync_parent_dir(target_path.parent()).ok();

    Ok(())
}

fn create_unique_temp_path(parent: &Path, target_path: &Path) -> Result<PathBuf> {
    let file_name = target_path
        .file_name()
        .context("File path must include a file name")?
        .to_string_lossy();
    let pid = std::process::id();
    for attempt in 0..100_u32 {
        let nanos = now_nanos();
        let temp_name = format!(
            ".{}{}{}.{}.tmp",
            file_name,
            TEMP_MARKER,
            pid,
            nanos + attempt as u128
        );
        let candidate = parent.join(temp_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "Failed to allocate a unique temp path beside {}",
        target_path.display()
    );
}

fn create_recovery_backup(
    target_path: &Path,
    target_metadata: &Option<fs::Metadata>,
    options: &SafeWriteOptions,
) -> Result<PathBuf> {
    let session_id = options
        .session_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_SESSION_ID);
    let root = options
        .recovery_root
        .clone()
        .unwrap_or_else(|| allthecodes_config::paths::data_root().join("file-write-history"));
    let backup_dir = root.join(sanitize_path_segment(session_id));
    fs::create_dir_all(&backup_dir).with_context(|| {
        format!(
            "Failed to create recovery directory {}",
            backup_dir.display()
        )
    })?;

    let mut hasher = Sha256::new();
    hasher.update(target_path.to_string_lossy().as_bytes());
    let digest = hex::encode(hasher.finalize());
    let backup_prefix = &digest[..16];
    let backup_path = backup_dir.join(format!("{}-{}.bak", backup_prefix, now_nanos()));

    fs::copy(target_path, &backup_path).with_context(|| {
        format!(
            "Failed to create recovery backup {} for {}",
            backup_path.display(),
            target_path.display()
        )
    })?;
    if let Some(metadata) = target_metadata {
        fs::set_permissions(&backup_path, metadata.permissions()).with_context(|| {
            format!(
                "Failed to preserve permissions on recovery backup {}",
                backup_path.display()
            )
        })?;
    }
    let _ = prune_recovery_backups(&backup_dir, backup_prefix, MAX_RECOVERY_BACKUPS_PER_FILE);
    Ok(backup_path)
}

fn prune_recovery_backups(backup_dir: &Path, backup_prefix: &str, keep: usize) -> io::Result<()> {
    if keep == 0 {
        return Ok(());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(backup_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(backup_prefix) && name.ends_with(".bak") {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            candidates.push((modified, entry.path()));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in candidates.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn restore_backup_if_target_missing(
    target_path: &Path,
    backup_path: &Path,
    metadata: &fs::Metadata,
) -> Result<()> {
    if target_path.exists() {
        return Ok(());
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(backup_path, target_path)?;
    fs::set_permissions(target_path, metadata.permissions())?;
    Ok(())
}

fn sanitize_path_segment(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        DEFAULT_SESSION_ID.to_string()
    } else {
        sanitized
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[cfg(unix)]
fn sync_parent_dir(parent: Option<&Path>) -> io::Result<()> {
    if let Some(parent) = parent {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_parent: Option<&Path>) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let from_wide = wide(from);
    let to_wide = wide(to);
    let ok = unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_options(recovery_root: PathBuf) -> SafeWriteOptions {
        SafeWriteOptions {
            recovery_root: Some(recovery_root),
            session_id: Some("safe/write:test".to_string()),
            ..Default::default()
        }
    }

    fn assert_no_temp_files(dir: &Path) {
        let leftovers: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(TEMP_MARKER))
            .collect();
        assert!(leftovers.is_empty(), "unexpected temp files: {leftovers:?}");
    }

    #[test]
    fn writes_new_file_through_temp_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("new.txt");
        let report = safe_write_text(
            &file,
            "hello\nworld",
            &test_options(dir.path().join("history")),
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "hello\nworld");
        assert!(report.created);
        assert_eq!(report.bytes_written, "hello\nworld".len());
        assert_eq!(report.line_count, 2);
        assert!(report.backup_path.is_none());
        assert_no_temp_files(dir.path());
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a").join("b").join("new.txt");

        safe_write_text(&file, "hello", &test_options(dir.path().join("history"))).unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "hello");
    }

    #[test]
    fn overwrite_creates_recovery_backup() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        fs::write(&file, "old").unwrap();

        let report =
            safe_write_text(&file, "new", &test_options(dir.path().join("history"))).unwrap();

        assert!(!report.created);
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        let backup = report.backup_path.expect("backup path");
        assert_eq!(fs::read_to_string(backup).unwrap(), "old");
    }

    #[test]
    fn prunes_old_recovery_backups_for_same_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        let history = dir.path().join("history");
        fs::write(&file, "v0").unwrap();

        for i in 1..=MAX_RECOVERY_BACKUPS_PER_FILE + 2 {
            safe_write_text(&file, &format!("v{i}"), &test_options(history.clone())).unwrap();
        }

        let backup_dir = history.join("safe_write_test");
        let backups = fs::read_dir(backup_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("bak"))
            .count();
        assert_eq!(backups, MAX_RECOVERY_BACKUPS_PER_FILE);
    }

    #[test]
    fn stale_temp_file_does_not_affect_target() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        let stale = dir
            .path()
            .join(format!(".target.txt{}stale.tmp", TEMP_MARKER));
        fs::write(&file, "old").unwrap();
        fs::write(&stale, "partial").unwrap();

        safe_write_text(&file, "new", &test_options(dir.path().join("history"))).unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(fs::read_to_string(&stale).unwrap(), "partial");
    }

    #[test]
    fn injected_temp_failure_preserves_original_and_cleans_temp() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        fs::write(&file, "old").unwrap();
        let mut options = test_options(dir.path().join("history"));
        options.fail_after_temp_write = true;

        let err = safe_write_text(&file, "new", &options).unwrap_err();

        assert!(err.to_string().contains("Injected safe write failure"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "old");
        assert_no_temp_files(dir.path());
    }

    #[test]
    fn rejects_binary_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("binary.txt");

        let err = safe_write_text(&file, "abc\0def", &test_options(dir.path().join("history")))
            .unwrap_err();

        assert!(err.to_string().contains("binary-looking content"));
        assert!(!file.exists());
    }

    #[test]
    fn rejects_known_binary_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("image.png");

        let err = safe_write_text(
            &file,
            "not really an image",
            &test_options(dir.path().join("history")),
        )
        .unwrap_err();

        assert!(err.to_string().contains("known binary file type"));
        assert!(!file.exists());
    }

    #[test]
    fn rejects_oversized_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("large.txt");
        let options = SafeWriteOptions {
            max_bytes: 3,
            recovery_root: Some(dir.path().join("history")),
            ..Default::default()
        };

        let err = safe_write_text(&file, "four", &options).unwrap_err();

        assert!(err.to_string().contains("oversized content"));
        assert!(!file.exists());
    }

    #[test]
    fn rejects_readonly_target_without_corruption() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("readonly.txt");
        fs::write(&file, "old").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();

        let err =
            safe_write_text(&file, "new", &test_options(dir.path().join("history"))).unwrap_err();

        assert!(err.to_string().contains("readonly"));
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = permissions.mode();
            permissions.set_mode(mode | 0o200);
        }
        #[cfg(windows)]
        {
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
        }
        fs::set_permissions(&file, permissions).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "old");
    }

    #[cfg(unix)]
    #[test]
    fn preserves_unix_permissions_on_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("mode.txt");
        fs::write(&file, "old").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();

        let report =
            safe_write_text(&file, "new", &test_options(dir.path().join("history"))).unwrap();

        assert!(report.permissions_preserved);
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlink_without_replacing_link() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, "old").unwrap();
        symlink(&target, &link).unwrap();

        let report =
            safe_write_text(&link, "new", &test_options(dir.path().join("history"))).unwrap();

        assert!(report.symlink_resolved);
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
