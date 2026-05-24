//! Computer Use — native desktop control tools.
//!
//! Provides detection/classification for external MCP tools (`detection`),
//! platform-native backends (`screenshot`, `input`), Tool trait wrappers
//! (`tools`), and CLI registration (`setup`).
//!
//! ## General modules
//!
//! * `lock` — concurrency lock preventing simultaneous mouse/keyboard operations
//! * `esc_hotkey` — emergency exit hotkey (e.g. triple-press Escape to abort)
//! * `app_names` — friendly app name → executable name mapping + launch/focus
//! * `host_adapter` — host platform capability probing & permission checks
//! * `drain_run_loop` — macOS CGEvent drain (ensure events are processed)
//! * `input_loader` — detect & select the best available input method per platform
//! * `swift_loader` — macOS Swift helper script loader for privileged APIs
//!
//! ## Win32-specific modules (Windows only)
//!
//! * `win32::com_word` — Word COM automation
//! * `win32::com_excel` — Excel COM automation
//! * `win32::ui_automation` — Windows UI Automation tree navigation
//! * `win32::virtual_cursor` — virtual cursor bound to a specific window
//! * `win32::window_border` — window border/frame metrics & client-area conversion
//! * `win32::shared` — common Win32 utilities (HWND, PowerShell runner, etc.)
//! * `win32::input_indicator` — visual click indicator overlay
//!
//! Reserved tool name prefix: `mcp__computer-use__*`

pub mod detection;
pub mod input;
pub mod screenshot;
pub mod setup;
pub mod tools;

// General Computer Use infrastructure
pub mod app_names;
pub mod drain_run_loop;
pub mod esc_hotkey;
pub mod executor;
pub mod host_adapter;
pub mod input_loader;
pub mod lock;
pub mod swift_loader;

// Win32-specific modules (compiled only on Windows)
#[cfg(target_os = "windows")]
pub mod win32;
