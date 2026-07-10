use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum PasteImageError {
    ClipboardUnavailable(String),
    #[cfg(any(test, feature = "image"))]
    NoImage(String),
    #[cfg(any(test, feature = "image"))]
    EncodeFailed(String),
    #[cfg(any(test, feature = "image"))]
    IoError(String),
}

impl std::fmt::Display for PasteImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteImageError::ClipboardUnavailable(msg) => write!(f, "clipboard unavailable: {msg}"),
            #[cfg(any(test, feature = "image"))]
            PasteImageError::NoImage(msg) => write!(f, "no image on clipboard: {msg}"),
            #[cfg(any(test, feature = "image"))]
            PasteImageError::EncodeFailed(msg) => write!(f, "could not encode image: {msg}"),
            #[cfg(any(test, feature = "image"))]
            PasteImageError::IoError(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for PasteImageError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedImageFormat {
    Png,
    Jpeg,
    Other,
}

impl EncodedImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            EncodedImageFormat::Png => "PNG",
            EncodedImageFormat::Jpeg => "JPEG",
            EncodedImageFormat::Other => "IMG",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PastedImageInfo {
    pub width: u32,
    pub height: u32,
    pub encoded_format: EncodedImageFormat,
}

/// Capture an image from the system clipboard, encode it as PNG bytes, and
/// return the bytes plus dimensions.
#[cfg(all(test, not(target_os = "android"), feature = "image"))]
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    let (path, info) = paste_image_to_temp_png()?;
    let bytes = std::fs::read(&path).map_err(|e| PasteImageError::IoError(e.to_string()))?;
    let _ = std::fs::remove_file(&path);
    Ok((bytes, info))
}

#[cfg(all(test, not(target_os = "android"), not(feature = "image")))]
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste requires the image feature".into(),
    ))
}

#[cfg(all(test, target_os = "android"))]
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste is unsupported on Android".into(),
    ))
}

/// Write the clipboard image to a temporary PNG file and return its path.
#[cfg(all(
    any(test, feature = "image"),
    not(target_os = "android"),
    feature = "image"
))]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    platform_paste_image_to_temp_png()
}

#[cfg(all(not(test), any(not(feature = "image"), target_os = "android")))]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste requires the image feature on a supported desktop platform".into(),
    ))
}

#[cfg(all(test, not(target_os = "android"), not(feature = "image")))]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste requires the image feature".into(),
    ))
}

#[cfg(all(test, target_os = "android"))]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste is unsupported on Android".into(),
    ))
}

#[cfg(all(feature = "image", target_os = "windows"))]
fn platform_paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let Some(win_path) = try_dump_windows_clipboard_image() else {
        return Err(PasteImageError::NoImage(
            "PowerShell did not find an image on the clipboard".into(),
        ));
    };

    let path = PathBuf::from(win_path);
    image_info_for_png_path(path)
}

#[cfg(all(feature = "image", target_os = "linux"))]
fn platform_paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    if is_probably_wsl() {
        return try_wsl_clipboard_fallback();
    }

    try_clipboard_image_command(&[
        ("wl-paste", &["--type", "image/png"][..]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ])
}

#[cfg(all(feature = "image", target_os = "macos"))]
fn platform_paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    try_clipboard_image_command(&[("pngpaste", &["-"][..])])
}

#[cfg(all(
    feature = "image",
    not(any(target_os = "windows", target_os = "linux", target_os = "macos"))
))]
fn platform_paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste is unsupported on this platform".into(),
    ))
}

#[cfg(all(feature = "image", any(target_os = "linux", target_os = "macos")))]
fn try_clipboard_image_command(
    candidates: &[(&str, &[&str])],
) -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let mut errors = Vec::new();

    for (command, args) in candidates {
        match capture_command_stdout(command, args) {
            Ok(bytes) => return write_png_bytes_to_temp(&bytes),
            Err(err) => errors.push(format!("{command}: {err}")),
        }
    }

    Err(PasteImageError::ClipboardUnavailable(format!(
        "no clipboard image command succeeded ({})",
        errors.join("; ")
    )))
}

#[cfg(all(feature = "image", any(target_os = "linux", target_os = "macos")))]
fn capture_command_stdout(command: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("failed to spawn {command}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return if stderr.is_empty() {
            Err(format!("{command} exited with status {}", output.status))
        } else {
            Err(format!("{command} failed: {stderr}"))
        };
    }

    if output.stdout.is_empty() {
        return Err(format!("{command} produced no image bytes"));
    }

    Ok(output.stdout)
}

#[cfg(all(feature = "image", any(target_os = "linux", target_os = "macos")))]
fn write_png_bytes_to_temp(bytes: &[u8]) -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let dyn_img =
        image::load_from_memory(bytes).map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
    let width = dyn_img.width();
    let height = dyn_img.height();

    let mut png = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png);
        dyn_img
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
    }

    let tmp = tempfile::Builder::new()
        .prefix("allthecodes-clipboard-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| PasteImageError::IoError(e.to_string()))?;
    std::fs::write(tmp.path(), &png).map_err(|e| PasteImageError::IoError(e.to_string()))?;
    let (_file, path) = tmp
        .keep()
        .map_err(|e| PasteImageError::IoError(e.error.to_string()))?;

    Ok((
        path,
        PastedImageInfo {
            width,
            height,
            encoded_format: EncodedImageFormat::Png,
        },
    ))
}

#[cfg(all(feature = "image", target_os = "linux"))]
fn try_wsl_clipboard_fallback() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let Some(win_path) = try_dump_windows_clipboard_image() else {
        return Err(PasteImageError::NoImage(
            "PowerShell did not find an image on the Windows clipboard".into(),
        ));
    };
    let Some(mapped_path) = convert_windows_path_to_wsl(&win_path) else {
        return Err(PasteImageError::IoError(format!(
            "could not map Windows clipboard path into WSL: {win_path}"
        )));
    };

    image_info_for_png_path(mapped_path)
}

#[cfg(all(feature = "image", any(target_os = "windows", target_os = "linux")))]
fn try_dump_windows_clipboard_image() -> Option<String> {
    let script = r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $img = Get-Clipboard -Format Image; if ($img -ne $null) { $p=[System.IO.Path]::GetTempFileName(); $p = [System.IO.Path]::ChangeExtension($p,'png'); $img.Save($p,[System.Drawing.Imaging.ImageFormat]::Png); Write-Output $p } else { exit 1 }"#;

    for command in ["powershell.exe", "pwsh", "powershell"] {
        match std::process::Command::new(command)
            .args(["-NoProfile", "-Command", script])
            .output()
        {
            Ok(output) if output.status.success() => {
                let win_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !win_path.is_empty() {
                    return Some(win_path);
                }
            }
            Ok(_) | Err(_) => {}
        }
    }

    None
}

#[cfg(all(feature = "image", not(target_os = "android")))]
fn image_info_for_png_path(path: PathBuf) -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let (width, height) =
        image::image_dimensions(&path).map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
    Ok((
        path,
        PastedImageInfo {
            width,
            height,
            encoded_format: EncodedImageFormat::Png,
        },
    ))
}

/// Normalize pasted text that may represent a filesystem path.
///
/// Supports file URLs, Windows/UNC paths, simple quoted paths, and a single
/// shell-escaped path.
pub fn normalize_pasted_path(pasted: &str) -> Option<PathBuf> {
    let pasted = pasted.trim();
    let unquoted = pasted
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| pasted.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(pasted);

    if let Ok(url) = url::Url::parse(unquoted) {
        if url.scheme() == "file" {
            return url.to_file_path().ok();
        }
    }

    if let Some(path) = normalize_windows_path(unquoted) {
        return Some(path);
    }

    let parts = shell_words::split(pasted).ok()?;
    if parts.len() == 1 {
        let part = parts.into_iter().next()?;
        if let Some(path) = normalize_windows_path(&part) {
            return Some(path);
        }
        return Some(PathBuf::from(part));
    }

    None
}

#[cfg(target_os = "linux")]
pub(crate) fn is_probably_wsl() -> bool {
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let version_lower = version.to_lowercase();
        if version_lower.contains("microsoft") || version_lower.contains("wsl") {
            return true;
        }
    }

    std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
}

#[cfg(target_os = "linux")]
fn convert_windows_path_to_wsl(input: &str) -> Option<PathBuf> {
    if input.starts_with("\\\\") {
        return None;
    }

    let drive_letter = input.chars().next()?.to_ascii_lowercase();
    if !drive_letter.is_ascii_lowercase() {
        return None;
    }
    if input.get(1..2) != Some(":") {
        return None;
    }

    let mut result = PathBuf::from(format!("/mnt/{drive_letter}"));
    for component in input
        .get(2..)?
        .trim_start_matches(['\\', '/'])
        .split(['\\', '/'])
        .filter(|component| !component.is_empty())
    {
        result.push(component);
    }

    Some(result)
}

fn normalize_windows_path(input: &str) -> Option<PathBuf> {
    let drive = input
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
        && input.get(1..2) == Some(":")
        && input
            .get(2..3)
            .map(|s| s == "\\" || s == "/")
            .unwrap_or(false);
    let unc = input.starts_with("\\\\");
    if !drive && !unc {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        if is_probably_wsl() {
            if let Some(converted) = convert_windows_path_to_wsl(input) {
                return Some(converted);
            }
        }
    }

    Some(PathBuf::from(input))
}

/// Infer an image format for a pasted path based on its extension.
pub fn pasted_image_format(path: &Path) -> EncodedImageFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => EncodedImageFormat::Png,
        Some("jpg") | Some("jpeg") => EncodedImageFormat::Jpeg,
        _ => EncodedImageFormat::Other,
    }
}

#[cfg(test)]
mod pasted_paths_tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn normalize_file_url() {
        let result =
            normalize_pasted_path("file:///tmp/example.png").expect("should parse file URL");
        assert_eq!(result, PathBuf::from("/tmp/example.png"));
    }

    #[test]
    fn normalize_windows_drive_path() {
        let input = r"C:\Temp\example.png";
        let result = normalize_pasted_path(input).expect("should parse Windows path");

        #[cfg(target_os = "linux")]
        let expected = if is_probably_wsl() {
            convert_windows_path_to_wsl(input).unwrap_or_else(|| PathBuf::from(input))
        } else {
            PathBuf::from(input)
        };
        #[cfg(not(target_os = "linux"))]
        let expected = PathBuf::from(input);

        assert_eq!(result, expected);
    }

    #[test]
    fn normalize_shell_escaped_single_path() {
        let result = normalize_pasted_path("/home/user/My\\ File.png")
            .expect("should unescape shell-escaped path");
        assert_eq!(result, PathBuf::from("/home/user/My File.png"));
    }

    #[test]
    fn normalize_simple_quoted_path_fallback() {
        let result =
            normalize_pasted_path("\"/home/user/My File.png\"").expect("should trim simple quotes");
        assert_eq!(result, PathBuf::from("/home/user/My File.png"));
    }

    #[test]
    fn normalize_multiple_tokens_returns_none() {
        let result = normalize_pasted_path("/home/user/a\\ b.png /home/user/c.png");
        assert!(result.is_none());
    }

    #[test]
    fn normalize_unc_windows_path() {
        let input = r"\\server\share\folder\file.jpg";
        let result = normalize_pasted_path(input).expect("should accept UNC path");
        assert_eq!(result, PathBuf::from(input));
    }

    #[test]
    fn pasted_image_format_png_jpeg_unknown() {
        assert_eq!(
            pasted_image_format(Path::new("/a/b/c.PNG")),
            EncodedImageFormat::Png
        );
        assert_eq!(
            pasted_image_format(Path::new("/a/b/c.jpg")),
            EncodedImageFormat::Jpeg
        );
        assert_eq!(
            pasted_image_format(Path::new("/a/b/c.JPEG")),
            EncodedImageFormat::Jpeg
        );
        assert_eq!(
            pasted_image_format(Path::new("/a/b/c.webp")),
            EncodedImageFormat::Other
        );
        assert_eq!(EncodedImageFormat::Png.label(), "PNG");
        assert_eq!(EncodedImageFormat::Jpeg.label(), "JPEG");
        assert_eq!(EncodedImageFormat::Other.label(), "IMG");
    }

    #[test]
    fn pasted_image_info_fields_and_stubbed_clipboard_errors_are_stable() {
        let info = PastedImageInfo {
            width: 320,
            height: 200,
            encoded_format: EncodedImageFormat::Png,
        };
        assert_eq!(info.width, 320);
        assert_eq!(info.height, 200);
        assert_eq!(info.encoded_format.label(), "PNG");

        let err = paste_image_as_png().expect_err("test build has no image feature");
        assert!(err.to_string().contains("clipboard"));
        let err = paste_image_to_temp_png().expect_err("test build has no image feature");
        assert!(err.to_string().contains("clipboard"));
    }

    #[test]
    fn paste_image_error_variants_have_display_messages() {
        assert!(PasteImageError::NoImage("empty".to_string())
            .to_string()
            .contains("no image"));
        assert!(PasteImageError::EncodeFailed("bad".to_string())
            .to_string()
            .contains("encode"));
        assert!(PasteImageError::IoError("disk".to_string())
            .to_string()
            .contains("io error"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn normalize_windows_path_in_wsl() {
        if !is_probably_wsl() {
            return;
        }
        let input = r"C:\Users\Alice\Pictures\example image.png";
        let result = normalize_pasted_path(input).expect("should convert Windows path on WSL");
        assert_eq!(
            result,
            PathBuf::from("/mnt/c/Users/Alice/Pictures/example image.png")
        );
    }
}
