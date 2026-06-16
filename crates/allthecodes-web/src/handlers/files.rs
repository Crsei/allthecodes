//! Files API handler — CRUD, tree listing, read/write, upload/download, and file
//! system operations scoped to the workspace root.
//!
//! All paths are resolved relative to the workspace root (engine.cwd()).
//! Every endpoint performs mandatory canonicalization and rejects paths that
//! escape the workspace root (path-traversal protection, including symlink attacks).

use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub use allthecodes_protocol::v1::files::{
    FileCopyRequest, FileDeleteRequest, FileDownloadQuery, FileEntry, FileMkdirRequest,
    FileMoveRequest, FileMutationResponse, FileReadQuery, FileReadResponse, FileRenameRequest,
    FileStat, FileStatQuery, FileTreeQuery, FileTreeResponse, FileUploadItem, FileUploadRequest,
    FileUploadResponse, FileUploadResult, FileWriteRequest,
};
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod, SerializationScope};
use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use sha2::{Digest, Sha256};

use crate::api_dispatcher::rest_processor_response;
use crate::processors::{protocol_error_response, Processor};
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default maximum bytes to read for the read endpoint.
const DEFAULT_READ_MAX_BYTES: u64 = 1_000_000; // 1 MiB
/// Absolute maximum bytes a client can request via max_bytes.
const MAX_READ_MAX_BYTES: u64 = 50_000_000; // 50 MiB
/// Maximum directory entries returned by tree listing.
const MAX_TREE_CHILDREN: usize = 10_000;

// ---------------------------------------------------------------------------
// Path resolution helpers
// ---------------------------------------------------------------------------

/// Canonicalize the workspace root so we can use it as an anchor.
fn workspace_root(state: &WebState) -> Result<PathBuf, ProtocolApiError> {
    let cwd = PathBuf::from(state.engine().cwd());
    cwd.canonicalize()
        .map_err(|err| internal_error(format!("Cannot resolve workspace root: {err}")))
}

/// Resolve a user-supplied path to an absolute path inside the workspace root.
///
/// * `raw_path` — the path string from the client (may be relative or absolute,
///   empty or "." for the workspace root itself).
/// * `must_exist` — if true, the resolved path must already exist on disk.
/// * `allow_missing_parent` — if true, allows the path to not exist even when
///   its parent does not exist (used by `mkdir -p`-style operations).
fn resolve_raw(
    root: &Path,
    raw_path: &str,
    must_exist: bool,
    allow_missing_parent: bool,
) -> Result<PathBuf, ProtocolApiError> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() || raw_path == "." || raw_path == "./" {
        return Ok(root.to_path_buf());
    }

    let user_path = Path::new(raw_path);
    let resolved = if user_path.is_absolute() {
        // Strip the Windows drive prefix or root component; re-attach below.
        user_path.to_path_buf()
    } else {
        root.join(user_path)
    };

    // ---- Normalize away `.` and `..` before any existence checks -----------
    let normalized = normalize_path(&resolved);

    // ---- Reject obvious path traversal on normalized path ------------------
    if !normalized.starts_with(root) {
        return Err(path_traversal());
    }

    // ---- Existence branches ------------------------------------------------
    if normalized.exists() {
        let canonical = normalized
            .canonicalize()
            .map_err(|err| bad_request(format!("Cannot access path: {err}")))?;
        if !canonical.starts_with(root) {
            return Err(path_traversal());
        }
        return Ok(canonical);
    }

    if must_exist {
        return Err(not_found(format!("Path does not exist: {raw_path}")));
    }

    // Path does not exist — verify that its parent (or the path itself when
    // it is the root) stays inside the workspace root so a future write or
    // mkdir cannot escape.
    if let Some(parent) = normalized.parent() {
        if parent != Path::new("") && parent.exists() {
            let pcanon = parent
                .canonicalize()
                .map_err(|err| bad_request(format!("Cannot access parent directory: {err}")))?;
            if !pcanon.starts_with(root) {
                return Err(path_traversal());
            }
            return Ok(normalized);
        }
        // Parent does not exist either.
        if !allow_missing_parent {
            return Err(not_found(format!(
                "Parent directory does not exist: {}",
                parent.display()
            )));
        }
        // Walk up until we find an existing ancestor, canonicalize that.
        let mut ancestor: Option<PathBuf> = None;
        for comp in parent.ancestors() {
            if comp.exists() {
                let pcanon = comp
                    .canonicalize()
                    .map_err(|err| bad_request(format!("Cannot access parent directory: {err}")))?;
                if !pcanon.starts_with(root) {
                    return Err(path_traversal());
                }
                ancestor = Some(pcanon);
                break;
            }
        }
        if ancestor.is_none() {
            // Nothing in the chain exists — at minimum root must exist.
            if !root.starts_with(root) || !root.exists() {
                return Err(internal_error("Workspace root is not accessible"));
            }
        }
        Ok(normalized)
    } else {
        // No parent (path is a root-like component)
        Ok(normalized)
    }
}

/// Resolve a path, canonicalize if it exists, and verify it is under `root`.
fn resolve_existing(root: &Path, raw_path: &str) -> Result<PathBuf, ProtocolApiError> {
    resolve_raw(root, raw_path, true, false)
}

/// Resolve a path that may or may not exist yet.
fn resolve_any(root: &Path, raw_path: &str) -> Result<PathBuf, ProtocolApiError> {
    resolve_raw(root, raw_path, false, false)
}

/// Resolve a path whose parent may also not exist (mkdir -p).
fn resolve_any_deep(root: &Path, raw_path: &str) -> Result<PathBuf, ProtocolApiError> {
    resolve_raw(root, raw_path, false, true)
}

/// Strip `.` and `..` components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(_) => components.push(component),
            Component::CurDir => { /* skip */ }
            Component::ParentDir => {
                if components.last().is_some() {
                    components.pop();
                }
            }
            other => components.push(other),
        }
    }
    components.iter().collect()
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of a file.
pub(crate) fn file_hash(path: &Path) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Detect whether a byte slice looks like binary content.
fn is_binary_content(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0x00)
}

/// Format a `SystemTime` to an RFC 3339 string.
fn format_time(time: std::time::SystemTime) -> String {
    // Use chrono when possible, fallback to debug display.
    let datetime: chrono::DateTime<chrono::Utc> = time.into();
    datetime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Atomic write: write content to a temporary file in the same directory, then
/// atomically rename it to the target path.
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), std::io::Error> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp_name = format!(
        ".allthecodes_tmp_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let tmp_path = dir.join(tmp_name);
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Count the number of lines in a string.
fn line_count(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    s.lines().count()
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn bad_request(msg: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::BadRequest {
        code: "bad_request",
        message: msg.into(),
    }
}

fn not_found(msg: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::NotFound {
        entity: "path",
        id: msg.into(),
    }
}

fn conflict(msg: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::Conflict { reason: msg.into() }
}

fn internal_error(msg: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::Internal {
        message: msg.into(),
    }
}

fn path_traversal() -> ProtocolApiError {
    ProtocolApiError::Forbidden {
        code: "path_traversal",
        message: "Path escapes workspace root".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FilesTreeProcessor {
    state: WebState,
}

impl From<WebState> for FilesTreeProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesTreeProcessor {
    type Request = FileTreeQuery;
    type Response = FileTreeResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.tree"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_tree(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesStatProcessor {
    state: WebState,
}

impl From<WebState> for FilesStatProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesStatProcessor {
    type Request = FileStatQuery;
    type Response = FileStat;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.stat"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_stat(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesReadProcessor {
    state: WebState,
}

impl From<WebState> for FilesReadProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesReadProcessor {
    type Request = FileReadQuery;
    type Response = FileReadResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.read"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_read(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesWriteProcessor {
    state: WebState,
}

impl From<WebState> for FilesWriteProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesWriteProcessor {
    type Request = FileWriteRequest;
    type Response = FileMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.write"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "path")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_write(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesUploadProcessor {
    state: WebState,
}

impl From<WebState> for FilesUploadProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesUploadProcessor {
    type Request = FileUploadRequest;
    type Response = FileUploadResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.upload"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "path")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_upload(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesMkdirProcessor {
    state: WebState,
}

impl From<WebState> for FilesMkdirProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesMkdirProcessor {
    type Request = FileMkdirRequest;
    type Response = FileMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.mkdir"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "path")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_mkdir(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesRenameProcessor {
    state: WebState,
}

impl From<WebState> for FilesRenameProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesRenameProcessor {
    type Request = FileRenameRequest;
    type Response = FileMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.rename"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "source")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_rename(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesCopyProcessor {
    state: WebState,
}

impl From<WebState> for FilesCopyProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesCopyProcessor {
    type Request = FileCopyRequest;
    type Response = FileMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.copy"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "destination")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_copy(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesMoveProcessor {
    state: WebState,
}

impl From<WebState> for FilesMoveProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesMoveProcessor {
    type Request = FileMoveRequest;
    type Response = FileMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.move"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "source")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_move(&self.state, params)
    }
}

#[derive(Clone)]
pub struct FilesDeleteProcessor {
    state: WebState,
}

impl From<WebState> for FilesDeleteProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for FilesDeleteProcessor {
    type Request = FileDeleteRequest;
    type Response = FileMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "files.delete"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "path")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        files_delete(&self.state, params)
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/files/tree — List directory entries.
pub async fn files_tree_handler(
    State(state): State<WebState>,
    Query(query): Query<FileTreeQuery>,
) -> Response {
    rest_processor_response::<FilesTreeProcessor>(state, ApiMethod::FilesTree, query).await
}

fn files_tree(
    state: &WebState,
    query: FileTreeQuery,
) -> Result<FileTreeResponse, ProtocolApiError> {
    let root = workspace_root(state)?;

    let dir_path = match query.path.as_deref() {
        Some(p) if !p.is_empty() => resolve_existing(&root, p)?,
        _ => root.clone(),
    };

    let profile_id = query.profile_id;

    // If the path points to a single file, return a single-entry listing.
    if dir_path.is_file() {
        let name = dir_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let meta = match std::fs::symlink_metadata(&dir_path) {
            Ok(m) => m,
            Err(err) => return Err(internal_error(format!("Cannot read metadata: {err}"))),
        };
        return Ok(FileTreeResponse {
            entries: vec![entry_from_path(&name, &dir_path, &meta)],
            path: dir_path.to_string_lossy().to_string(),
            profile_id,
            truncated: false,
        });
    }

    // Read directory contents.
    let mut read_dir = match std::fs::read_dir(&dir_path) {
        Ok(r) => r,
        Err(err) => return Err(not_found(format!("Cannot read directory: {err}"))),
    };

    let mut entries: Vec<FileEntry> = Vec::new();
    let mut truncated = false;
    for entry in read_dir.by_ref().take(MAX_TREE_CHILDREN + 1) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        entries.push(entry_from_path(&name, &entry.path(), &meta));
    }
    if read_dir.next().is_some() {
        truncated = true;
    }

    Ok(FileTreeResponse {
        entries,
        path: dir_path.to_string_lossy().to_string(),
        profile_id,
        truncated,
    })
}

/// GET /api/files/stat — File metadata + hash.
pub async fn files_stat_handler(
    State(state): State<WebState>,
    Query(query): Query<FileStatQuery>,
) -> Response {
    rest_processor_response::<FilesStatProcessor>(state, ApiMethod::FilesStat, query).await
}

fn files_stat(state: &WebState, query: FileStatQuery) -> Result<FileStat, ProtocolApiError> {
    let root = workspace_root(state)?;

    let path_str = query.path.as_deref().unwrap_or("");
    let profile_id = query.profile_id;

    if path_str.is_empty() {
        // Stat the workspace root itself.
        let meta = match std::fs::symlink_metadata(&root) {
            Ok(m) => m,
            Err(err) => {
                return Ok(FileStat {
                    exists: false,
                    path: root.to_string_lossy().to_string(),
                    is_dir: false,
                    is_file: false,
                    is_symlink: false,
                    size: None,
                    modified: None,
                    hash: None,
                    profile_id,
                    error: Some(err.to_string()),
                });
            }
        };
        let hash = file_hash(&root).ok();
        return Ok(FileStat {
            exists: true,
            path: root.to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            is_file: meta.is_file(),
            is_symlink: meta.file_type().is_symlink(),
            size: Some(meta.len()),
            modified: Some(meta.modified().map(format_time).unwrap_or_default()),
            hash,
            profile_id,
            error: None,
        });
    }

    let resolved = resolve_existing(&root, path_str)?;

    let meta = match std::fs::symlink_metadata(&resolved) {
        Ok(m) => m,
        Err(err) => {
            return Ok(FileStat {
                exists: false,
                path: resolved.to_string_lossy().to_string(),
                is_dir: false,
                is_file: false,
                is_symlink: false,
                size: None,
                modified: None,
                hash: None,
                profile_id,
                error: Some(err.to_string()),
            });
        }
    };

    let hash = file_hash(&resolved).ok();

    Ok(FileStat {
        exists: true,
        path: resolved.to_string_lossy().to_string(),
        is_dir: meta.is_dir(),
        is_file: meta.is_file(),
        is_symlink: meta.file_type().is_symlink(),
        size: Some(meta.len()),
        modified: Some(meta.modified().map(format_time).unwrap_or_default()),
        hash,
        profile_id,
        error: None,
    })
}

/// GET /api/files/read — Read text content from a file.
pub async fn files_read_handler(
    State(state): State<WebState>,
    Query(query): Query<FileReadQuery>,
) -> Response {
    rest_processor_response::<FilesReadProcessor>(state, ApiMethod::FilesRead, query).await
}

fn files_read(
    state: &WebState,
    query: FileReadQuery,
) -> Result<FileReadResponse, ProtocolApiError> {
    let root = workspace_root(state)?;

    let path_str = match query.path.as_deref() {
        Some(p) if !p.is_empty() => p,
        _ => return Err(bad_request("path query parameter is required")),
    };

    let profile_id = query.profile_id;
    let max_bytes = query
        .max_bytes
        .unwrap_or(DEFAULT_READ_MAX_BYTES)
        .min(MAX_READ_MAX_BYTES);

    let resolved = resolve_existing(&root, path_str)?;

    if resolved.is_dir() {
        return Err(bad_request("Cannot read a directory as text"));
    }

    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(err) => return Err(internal_error(format!("Cannot read metadata: {err}"))),
    };

    let hash = match file_hash(&resolved) {
        Ok(h) => h,
        Err(err) => return Err(internal_error(format!("Cannot compute hash: {err}"))),
    };

    let file_size = meta.len();

    // Read up to max_bytes.
    let mut data = Vec::with_capacity(max_bytes as usize);
    let file = match std::fs::File::open(&resolved) {
        Ok(f) => f,
        Err(err) => return Err(internal_error(format!("Cannot open file: {err}"))),
    };
    if let Err(err) = file.take(max_bytes).read_to_end(&mut data) {
        return Err(internal_error(format!("Cannot read file: {err}")));
    }

    let truncated = (data.len() as u64) < file_size;

    // Detect binary content.
    if is_binary_content(&data) {
        return Ok(FileReadResponse {
            content: String::new(),
            truncated: false,
            is_binary: true,
            hash,
            size: file_size,
            lines: 0,
            profile_id,
        });
    }

    let content = match String::from_utf8(data) {
        Ok(s) => s,
        Err(_) => {
            // If UTF-8 decoding fails, treat as binary.
            return Ok(FileReadResponse {
                content: String::new(),
                truncated: false,
                is_binary: true,
                hash,
                size: file_size,
                lines: 0,
                profile_id,
            });
        }
    };

    let lines = line_count(&content);

    Ok(FileReadResponse {
        content,
        truncated,
        is_binary: false,
        hash,
        size: file_size,
        lines,
        profile_id,
    })
}

/// PUT /api/files/write — Write content to a file with optional hash/revision
/// enforcement and atomic write semantics.
pub async fn files_write_handler(
    State(state): State<WebState>,
    Json(req): Json<FileWriteRequest>,
) -> Response {
    rest_processor_response::<FilesWriteProcessor>(state, ApiMethod::FilesWrite, req).await
}

fn files_write(
    state: &WebState,
    req: FileWriteRequest,
) -> Result<FileMutationResponse, ProtocolApiError> {
    let root = workspace_root(state)?;
    let resolved = resolve_any(&root, &req.path)?;

    if resolved.is_dir() {
        return Err(bad_request("Cannot write to a directory"));
    }

    // Overwrite protection.
    let overwrite = req.overwrite.unwrap_or(false);
    if resolved.exists() && !overwrite {
        return Err(conflict(format!(
            "File already exists: {}. Set overwrite=true to replace.",
            resolved.display()
        )));
    }

    // Hash/revision enforcement: if the client provides the expected hash,
    // compute the current file's hash (if it exists) and reject on mismatch.
    if let Some(expected_hash) = &req.hash {
        if resolved.exists() {
            let current_hash = match file_hash(&resolved) {
                Ok(h) => h,
                Err(err) => return Err(internal_error(format!("Cannot compute hash: {err}"))),
            };
            if &current_hash != expected_hash {
                return Err(conflict(format!(
                    "Hash mismatch: expected {}, got {}. File was modified since last read.",
                    expected_hash, current_hash
                )));
            }
        }
    }

    // Ensure parent directory exists.
    if let Some(parent) = resolved.parent() {
        if !parent.exists() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return Err(internal_error(format!(
                    "Cannot create parent directory: {err}"
                )));
            }
        }
    }

    // Atomic write.
    let content_bytes = req.content.as_bytes();
    if let Err(err) = atomic_write(&resolved, content_bytes) {
        return Err(internal_error(format!("Cannot write file: {err}")));
    }

    let hash = match file_hash(&resolved) {
        Ok(h) => Some(h),
        Err(_) => None,
    };

    Ok(FileMutationResponse {
        ok: true,
        path: resolved.to_string_lossy().to_string(),
        hash,
    })
}

/// POST /api/files/upload — Upload one or more files to a directory.
pub async fn files_upload_handler(
    State(state): State<WebState>,
    Json(req): Json<FileUploadRequest>,
) -> Response {
    rest_processor_response::<FilesUploadProcessor>(state, ApiMethod::FilesUpload, req).await
}

fn files_upload(
    state: &WebState,
    req: FileUploadRequest,
) -> Result<FileUploadResponse, ProtocolApiError> {
    let root = workspace_root(state)?;
    let target_dir = resolve_any(&root, &req.path)?;

    // If the target doesn't exist, try to create it.
    if !target_dir.exists() {
        if let Err(err) = std::fs::create_dir_all(&target_dir) {
            return Err(internal_error(format!(
                "Cannot create target directory: {err}"
            )));
        }
    }

    if !target_dir.is_dir() {
        return Err(bad_request("Upload path must be a directory"));
    }

    let mut results = Vec::new();
    for item in &req.files {
        // Prevent path traversal in individual file names.
        let file_name = Path::new(&item.name);
        if file_name.components().any(|c| {
            matches!(
                c,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        }) {
            return Err(bad_request(format!("Invalid file name: {}", item.name)));
        }

        let dest = target_dir.join(&item.name);

        // Decode content.
        let content_bytes = match item.encoding.as_deref() {
            Some("base64") | Some("b64") => match base64_decode(&item.content) {
                Ok(b) => b,
                Err(err) => {
                    return Err(bad_request(format!(
                        "Base64 decode error for '{}': {err}",
                        item.name
                    )));
                }
            },
            _ => item.content.as_bytes().to_vec(),
        };

        // Atomic write.
        if let Err(err) = atomic_write(&dest, &content_bytes) {
            return Err(internal_error(format!(
                "Cannot write '{}': {err}",
                item.name
            )));
        }

        let hash = file_hash(&dest).unwrap_or_default();
        results.push(FileUploadResult {
            name: item.name.clone(),
            path: dest.to_string_lossy().to_string(),
            size: content_bytes.len() as u64,
            hash,
        });
    }

    Ok(FileUploadResponse {
        ok: true,
        path: target_dir.to_string_lossy().to_string(),
        files: results,
    })
}

/// GET /api/files/download — Stream a file as a binary download.
pub async fn files_download_handler(
    State(state): State<WebState>,
    Query(query): Query<FileDownloadQuery>,
) -> Response {
    let root = match workspace_root(&state) {
        Ok(r) => r,
        Err(e) => return protocol_error_response(e),
    };

    let path_str = match query.path.as_deref() {
        Some(p) if !p.is_empty() => p,
        _ => return protocol_error_response(bad_request("path query parameter is required")),
    };

    let resolved = match resolve_existing(&root, path_str) {
        Ok(r) => r,
        Err(e) => return protocol_error_response(e),
    };

    if resolved.is_dir() {
        return protocol_error_response(bad_request("Cannot download a directory"));
    }

    let data = match std::fs::read(&resolved) {
        Ok(d) => d,
        Err(err) => {
            return protocol_error_response(internal_error(format!("Cannot read file: {err}")))
        }
    };

    let file_name = resolved
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());

    let content_type = guess_content_type(&resolved);

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", file_name),
            ),
            (header::CONTENT_LENGTH, &data.len().to_string()),
        ],
        data,
    )
        .into_response()
}

/// POST /api/files/mkdir — Create a directory (mkdir -p semantics).
pub async fn files_mkdir_handler(
    State(state): State<WebState>,
    Json(req): Json<FileMkdirRequest>,
) -> Response {
    rest_processor_response::<FilesMkdirProcessor>(state, ApiMethod::FilesMkdir, req).await
}

fn files_mkdir(
    state: &WebState,
    req: FileMkdirRequest,
) -> Result<FileMutationResponse, ProtocolApiError> {
    let root = workspace_root(state)?;
    let resolved = resolve_any_deep(&root, &req.path)?;

    if resolved.exists() {
        if resolved.is_dir() {
            // Idempotent: directory already exists.
            return Ok(FileMutationResponse {
                ok: true,
                path: resolved.to_string_lossy().to_string(),
                hash: None,
            });
        }
        return Err(bad_request(format!(
            "Path already exists and is not a directory: {}",
            resolved.display()
        )));
    }

    if let Err(err) = std::fs::create_dir_all(&resolved) {
        return Err(internal_error(format!("Cannot create directory: {err}")));
    }

    Ok(FileMutationResponse {
        ok: true,
        path: resolved.to_string_lossy().to_string(),
        hash: None,
    })
}

/// POST /api/files/rename — Rename a file or directory.
pub async fn files_rename_handler(
    State(state): State<WebState>,
    Json(req): Json<FileRenameRequest>,
) -> Response {
    rest_processor_response::<FilesRenameProcessor>(state, ApiMethod::FilesRename, req).await
}

fn files_rename(
    state: &WebState,
    req: FileRenameRequest,
) -> Result<FileMutationResponse, ProtocolApiError> {
    let root = workspace_root(state)?;
    let source = resolve_existing(&root, &req.source)?;
    let destination = resolve_any(&root, &req.destination)?;

    if destination.exists() {
        return Err(conflict(format!(
            "Destination already exists: {}",
            destination.display()
        )));
    }

    // Ensure parent of destination exists.
    if let Some(parent) = destination.parent() {
        if !parent.exists() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return Err(internal_error(format!(
                    "Cannot create parent directory: {err}"
                )));
            }
        }
    }

    if let Err(err) = std::fs::rename(&source, &destination) {
        return Err(internal_error(format!("Cannot rename: {err}")));
    }

    Ok(FileMutationResponse {
        ok: true,
        path: destination.to_string_lossy().to_string(),
        hash: None,
    })
}

/// POST /api/files/copy — Copy a file or directory.
pub async fn files_copy_handler(
    State(state): State<WebState>,
    Json(req): Json<FileCopyRequest>,
) -> Response {
    rest_processor_response::<FilesCopyProcessor>(state, ApiMethod::FilesCopy, req).await
}

fn files_copy(
    state: &WebState,
    req: FileCopyRequest,
) -> Result<FileMutationResponse, ProtocolApiError> {
    let root = workspace_root(state)?;
    let source = resolve_existing(&root, &req.source)?;
    let destination = resolve_any(&root, &req.destination)?;

    let overwrite = req.overwrite.unwrap_or(false);
    if destination.exists() && !overwrite {
        return Err(conflict(format!(
            "Destination already exists: {}. Set overwrite=true to replace.",
            destination.display()
        )));
    }

    // Ensure parent of destination exists.
    if let Some(parent) = destination.parent() {
        if !parent.exists() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return Err(internal_error(format!(
                    "Cannot create parent directory: {err}"
                )));
            }
        }
    }

    if source.is_dir() {
        if let Err(err) = copy_dir_recursive(&source, &destination, overwrite) {
            return Err(internal_error(format!("Cannot copy directory: {err}")));
        }
    } else {
        if let Err(err) = std::fs::copy(&source, &destination) {
            return Err(internal_error(format!("Cannot copy file: {err}")));
        }
    }

    Ok(FileMutationResponse {
        ok: true,
        path: destination.to_string_lossy().to_string(),
        hash: None,
    })
}

/// POST /api/files/move — Move a file or directory.
pub async fn files_move_handler(
    State(state): State<WebState>,
    Json(req): Json<FileMoveRequest>,
) -> Response {
    rest_processor_response::<FilesMoveProcessor>(state, ApiMethod::FilesMove, req).await
}

fn files_move(
    state: &WebState,
    req: FileMoveRequest,
) -> Result<FileMutationResponse, ProtocolApiError> {
    // Move is atomic rename when source and dest are on the same filesystem,
    // falling back to copy + delete.
    let root = workspace_root(state)?;
    let source = resolve_existing(&root, &req.source)?;
    let destination = resolve_any(&root, &req.destination)?;

    let overwrite = req.overwrite.unwrap_or(false);
    if destination.exists() && !overwrite {
        return Err(conflict(format!(
            "Destination already exists: {}. Set overwrite=true to replace.",
            destination.display()
        )));
    }

    // If destination exists and overwrite is true, remove it first.
    if destination.exists() {
        if destination.is_dir() {
            if let Err(err) = std::fs::remove_dir_all(&destination) {
                return Err(internal_error(format!(
                    "Cannot remove existing destination: {err}"
                )));
            }
        } else {
            if let Err(err) = std::fs::remove_file(&destination) {
                return Err(internal_error(format!(
                    "Cannot remove existing destination: {err}"
                )));
            }
        }
    }

    // Ensure parent of destination exists.
    if let Some(parent) = destination.parent() {
        if !parent.exists() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return Err(internal_error(format!(
                    "Cannot create parent directory: {err}"
                )));
            }
        }
    }

    if let Err(_err) = std::fs::rename(&source, &destination) {
        // Fallback: copy + delete.
        if source.is_dir() {
            if let Err(copy_err) = copy_dir_recursive(&source, &destination, false) {
                return Err(internal_error(format!(
                    "Cannot move directory (copy failed): {copy_err}"
                )));
            }
            if let Err(del_err) = std::fs::remove_dir_all(&source) {
                return Err(internal_error(format!(
                    "Cannot move directory (cleanup failed): {del_err}"
                )));
            }
        } else {
            if let Err(copy_err) = std::fs::copy(&source, &destination) {
                return Err(internal_error(format!(
                    "Cannot move file (copy failed): {copy_err}"
                )));
            }
            if let Err(del_err) = std::fs::remove_file(&source) {
                return Err(internal_error(format!(
                    "Cannot move file (cleanup failed): {del_err}"
                )));
            }
        }
    }

    Ok(FileMutationResponse {
        ok: true,
        path: destination.to_string_lossy().to_string(),
        hash: None,
    })
}

/// DELETE /api/files — Delete a file or directory.
pub async fn files_delete_handler(
    State(state): State<WebState>,
    Json(req): Json<FileDeleteRequest>,
) -> Response {
    rest_processor_response::<FilesDeleteProcessor>(state, ApiMethod::FilesDelete, req).await
}

fn files_delete(
    state: &WebState,
    req: FileDeleteRequest,
) -> Result<FileMutationResponse, ProtocolApiError> {
    let root = workspace_root(state)?;
    let resolved = resolve_existing(&root, &req.path)?;

    let recursive = req.recursive.unwrap_or(false);

    if resolved.is_dir() {
        if recursive {
            if let Err(err) = std::fs::remove_dir_all(&resolved) {
                return Err(internal_error(format!("Cannot remove directory: {err}")));
            }
        } else {
            // Remove only if directory is empty.
            let is_empty = match std::fs::read_dir(&resolved) {
                Ok(mut r) => r.next().is_none(),
                Err(_) => false,
            };
            if !is_empty {
                return Err(bad_request(format!(
                    "Directory is not empty: {}. Use recursive=true to delete non-empty directories.",
                    resolved.display()
                )));
            }
            if let Err(err) = std::fs::remove_dir(&resolved) {
                return Err(internal_error(format!("Cannot remove directory: {err}")));
            }
        }
    } else {
        if let Err(err) = std::fs::remove_file(&resolved) {
            return Err(internal_error(format!("Cannot remove file: {err}")));
        }
    }

    Ok(FileMutationResponse {
        ok: true,
        path: resolved.to_string_lossy().to_string(),
        hash: None,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn entry_from_path(name: &str, full_path: &Path, meta: &std::fs::Metadata) -> FileEntry {
    let is_symlink = meta.file_type().is_symlink();
    FileEntry {
        name: name.to_string(),
        path: full_path.to_string_lossy().to_string(),
        is_dir: meta.is_dir(),
        is_symlink,
        size: meta.len(),
        modified: meta.modified().map(format_time).unwrap_or_default(),
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path, overwrite: bool) -> Result<(), std::io::Error> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path, overwrite)?;
        } else {
            if dest_path.exists() && !overwrite {
                continue;
            }
            std::fs::copy(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    // Try standard base64 first, then URL-safe.
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(input))
        .map_err(|e| format!("base64 decode error: {e}"))
}

fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "txt" | "md" | "rst" | "adoc" => "text/plain; charset=utf-8",
        "html" | "htm" | "xhtml" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "yaml" | "yml" => "application/x-yaml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "tar" | "gz" | "tgz" => "application/gzip",
        "wasm" => "application/wasm",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "sh" | "bash" | "zsh" | "fish" => "text/x-shellscript; charset=utf-8",
        "py" => "text/x-python; charset=utf-8",
        "rs" => "text/x-rust; charset=utf-8",
        "go" => "text/x-go; charset=utf-8",
        "java" => "text/x-java; charset=utf-8",
        "ts" | "tsx" => "application/typescript; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
