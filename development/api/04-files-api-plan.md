# Files API Backend Plan

## Scope

Implement workspace file APIs:

```http
GET    /api/files/tree?path=&profile_id=
GET    /api/files/stat?path=&profile_id=
GET    /api/files/read?path=&profile_id=
PUT    /api/files/write
POST   /api/files/upload
GET    /api/files/download?path=&profile_id=
POST   /api/files/mkdir
POST   /api/files/rename
POST   /api/files/copy
POST   /api/files/move
DELETE /api/files
```

Frontend contracts:

- `FileTreeResponse`
- `FileReadResponse`
- `FileStat`
- `FileMutationResponse`
- `FileUploadResponse`

## Current Gap

`capabilities.rs` reports `files=false`, and none of the file routes are
registered. File APIs are used by the Files page, image preview, chat upload,
and rollback/file side panels.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/files.rs
```

Routes:

```rust
.route("/api/files/tree", get(handlers::files_tree_handler))
.route("/api/files/stat", get(handlers::files_stat_handler))
.route("/api/files/read", get(handlers::files_read_handler))
.route("/api/files/write", put(handlers::files_write_handler))
.route("/api/files/upload", post(handlers::files_upload_handler))
.route("/api/files/download", get(handlers::files_download_handler))
.route("/api/files/mkdir", post(handlers::files_mkdir_handler))
.route("/api/files/rename", post(handlers::files_rename_handler))
.route("/api/files/copy", post(handlers::files_copy_handler))
.route("/api/files/move", post(handlers::files_move_handler))
.route("/api/files", delete(handlers::files_delete_handler))
```

Flip `files=true` after MVP tests pass.

## Root Resolution

Resolve all paths relative to the active workspace root:

1. Use profile/workspace selection if available.
2. Fall back to `state.engine().app_state().cwd`.
3. Canonicalize and reject paths outside root.

Never allow absolute path access unless it canonicalizes under the workspace
root.

## Behavior

- `tree`: list directory entries, shallow by default, cap child count.
- `stat`: return metadata and hash/revision for conflict checks.
- `read`: return text content; detect binary/image; cap max bytes.
- `write`: enforce expected hash/revision when provided.
- `upload`: multipart upload to a target directory.
- `download`: stream file bytes with safe headers.
- `mkdir`, `rename`, `copy`, `move`, `delete`: return mutation response and
  refreshed entry/stat when possible.

## Safety

- Path traversal protection is mandatory.
- Use atomic writes where practical.
- Return `too_large` for files above read cap.
- Reject overwrite unless `overwrite=true`.
- Do not follow symlink escapes outside workspace root.

## Tests

Add tests for:

- Tree lists temp workspace.
- Read text file and binary metadata.
- Write rejects stale expected hash.
- Upload stores under requested parent.
- Rename/move/copy/delete stay inside root.
- Traversal attempts return `400` or `403`.

Run:

```bash
cargo test -p allthecodes-web files
```

## Acceptance

- Files page can browse/read/write local workspace files.
- Chat upload no longer depends on an unimplemented endpoint.
- Capability can be changed to `files=true`.

