//! Build script: track web-ui source changes without auto-building the SPA.
//!
//! - Sets cargo:rerun-if-changed so rebuilds are triggered on source changes
//! - Does not run npm during Cargo builds; build web-ui manually when needed
//!
//! Paths are resolved relative to the workspace root, which lives two levels
//! above this crate's manifest directory (`<workspace>/crates/allthecodes`).

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <workspace>/crates/allthecodes
    let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return PathBuf::from(".");
    };
    let manifest_dir = PathBuf::from(manifest_dir);
    manifest_dir
        .parent() // <workspace>/crates
        .and_then(|p| p.parent()) // <workspace>
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir)
}

fn main() {
    let root = workspace_root();

    // Rerun if frontend source changes.
    for rel in [
        "web-ui/src/",
        "web-ui/package.json",
        "web-ui/index.html",
        "web-ui/vite.config.ts",
        "web-ui/tailwind.config.js",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(rel).display());
    }

    // Automatic web-ui npm builds are intentionally disabled for Cargo builds.
    // To refresh the browser UI assets, run:
    //   cd web-ui && npm install && npm run build
}
