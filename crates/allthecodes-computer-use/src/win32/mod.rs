//! Win32 Computer Use modules (Windows-specific).
//!
//! These modules provide Windows-specific desktop automation capabilities
//! via PowerShell-based COM interop and WinAPI calls:
//!
//! - `com_word`: Microsoft Word automation (launch, edit, save documents)
//! - `com_excel`: Microsoft Excel automation (launch, edit cells, formulas, save)
//! - `ui_automation`: Windows UI Automation tree navigation for element discovery
//! - `virtual_cursor`: Virtual cursor management for window-bound cursor state
//! - `window_border`: Window border rendering and management
//! - `shared`: Common Win32 utilities (window handles, error codes, constants)
//! - `input_indicator`: Visual input indicator overlay

pub mod com_excel;
pub mod com_word;
pub mod input_indicator;
pub mod shared;
pub mod ui_automation;
pub mod virtual_cursor;
pub mod window_border;
