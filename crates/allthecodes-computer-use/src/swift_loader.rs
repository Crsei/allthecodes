//! Swift helper script loader for macOS.
//!
//! On macOS, certain Computer Use operations require privileged access to
//! system APIs (accessibility, screen recording, CGEvent post). These are
//! best accomplished via small Swift scripts compiled or interpreted at
//! runtime, as Swift has native access to the required frameworks.
//!
//! This module provides a loader that finds the Swift runtime and executes
//! helper scripts, caching compilation results where possible.

#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::time::Duration;

/// The Swift script cache directory (under ~/.allthecodes/cache/swift/).
#[cfg(target_os = "macos")]
fn cache_dir() -> PathBuf {
    let home = std::env::var("CC_RUST_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|p| p.join(".cc-rust"))
                .unwrap_or_else(|| PathBuf::from(".cc-rust"))
        });
    home.join("cache").join("swift")
}

/// Whether Swift is available on this platform.
pub async fn swift_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        tokio::time::timeout(
            Duration::from_secs(2),
            tokio::process::Command::new("swift")
                .args(["--version"])
                .output(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|o| o.status.success())
        .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Run a Swift script string and return its stdout.
///
/// The script is compiled on-the-fly using `swift -e`. For longer-running
/// scripts, consider `compile_and_run` for better performance.
pub async fn run_swift_script(script: &str) -> anyhow::Result<String> {
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("swift")
            .args(["-e", script])
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run Swift script: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Swift script failed: {}", stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = script;
        anyhow::bail!("Swift is only available on macOS");
    }
}

/// Compile a Swift source file and cache the binary, then run it.
///
/// This is useful for complex or long-running helpers that should be compiled
/// once and reused.
pub async fn compile_and_run(source_name: &str, source: &str) -> anyhow::Result<String> {
    #[cfg(target_os = "macos")]
    {
        let cache = cache_dir();
        tokio::fs::create_dir_all(&cache).await?;

        let binary = cache.join(source_name);
        let source_path = cache.join(format!("{}.swift", source_name));

        // Write source if not cached.
        if !tokio::fs::try_exists(&binary).await.unwrap_or(false) {
            tokio::fs::write(&source_path, source).await?;

            let output = tokio::process::Command::new("swiftc")
                .args([
                    "-o",
                    binary.to_str().unwrap(),
                    source_path.to_str().unwrap(),
                ])
                .output()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to compile Swift script: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("Swift compilation failed: {}", stderr.trim());
            }
        }

        // Run the compiled binary.
        let output = tokio::process::Command::new(&binary)
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run compiled Swift: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Swift binary failed: {}", stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = source_name;
        let _ = source;
        anyhow::bail!("Swift is only available on macOS");
    }
}

/// Run an Accessibility permission check via Swift.
///
/// Returns `true` if the app has Accessibility permissions.
pub async fn check_accessibility_via_swift() -> bool {
    let script = r#"
import Cocoa
import ApplicationServices

let options: NSDictionary = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): false]
let trusted = AXIsProcessTrustedWithOptions(options)
print(trusted ? "true" : "false")
"#;
    run_swift_script(script)
        .await
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Get the frontmost application's bundle identifier via Swift.
pub async fn frontmost_app_bundle_id() -> Option<String> {
    let script = r#"
import Cocoa

if let app = NSWorkspace.shared.frontmostApplication {
    print(app.bundleIdentifier ?? "")
}
"#;
    run_swift_script(script)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Get the frontmost application's name via Swift.
pub async fn frontmost_app_name() -> Option<String> {
    let script = r#"
import Cocoa

if let app = NSWorkspace.shared.frontmostApplication {
    print(app.localizedName ?? "")
}
"#;
    run_swift_script(script)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Get a list of all running applications.
pub async fn running_apps() -> Vec<String> {
    let script = r#"
import Cocoa

let apps = NSWorkspace.shared.runningApplications
for app in apps {
    if let name = app.localizedName {
        print(name)
    }
}
"#;
    run_swift_script(script)
        .await
        .ok()
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_swift_available() {
        // Should not panic, but may return false on non-macOS or CI.
        let _ = swift_available().await;
    }

    #[tokio::test]
    async fn test_simple_swift_script() {
        if swift_available().await {
            let result = run_swift_script(r#"print("hello from swift")"#)
                .await
                .unwrap();
            assert_eq!(result, "hello from swift");
        }
    }
}
