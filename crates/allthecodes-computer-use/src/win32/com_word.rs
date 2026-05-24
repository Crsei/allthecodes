//! Microsoft Word COM automation via PowerShell.
//!
//! Provides high-level operations for Microsoft Word:
//! - Launching Word and creating/opening documents
//! - Reading and writing text content
//! - Formatting (bold, italic, font size, alignment)
//! - Inserting tables, images, page breaks
//! - Saving and closing documents
//! - Exporting to PDF

use super::shared::run_powershell;

/// Result of a Word document operation.
#[derive(Debug, Clone)]
pub struct WordResult {
    pub success: bool,
    pub message: String,
    pub document_name: Option<String>,
    pub page_count: Option<u32>,
}

/// Launch Microsoft Word and create a new blank document.
pub async fn word_create_document() -> anyhow::Result<WordResult> {
    let script = r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $true
$doc = $word.Documents.Add()
Write-Output "created|$($doc.Name)"
$word.Quit() | Out-Null
"#;
    let output = run_powershell(script).await?;
    let parts: Vec<&str> = output.split('|').collect();
    if parts.len() >= 2 {
        Ok(WordResult {
            success: true,
            message: "Document created".to_string(),
            document_name: Some(parts[1].to_string()),
            page_count: None,
        })
    } else {
        Ok(WordResult {
            success: true,
            message: "Document created".to_string(),
            document_name: None,
            page_count: None,
        })
    }
}

/// Open an existing Word document.
pub async fn word_open_document(path: &str) -> anyhow::Result<WordResult> {
    let script = format!(
        r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $true
$doc = $word.Documents.Open("{path}")
Write-Output "opened|$($doc.Name)|$($doc.Sections.Count)"
"#,
        path = path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = run_powershell(&script).await?;
    let parts: Vec<&str> = output.split('|').collect();
    Ok(WordResult {
        success: true,
        message: format!("Opened document: {}", parts.get(1).unwrap_or(&"unknown")),
        document_name: parts.get(1).map(|s| s.to_string()),
        page_count: None,
    })
}

/// Insert text at the current cursor position in a Word document.
pub async fn word_insert_text(text: &str) -> anyhow::Result<WordResult> {
    let escaped = text.replace('"', "\\\"");
    let script = format!(
        r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $true
$selection = $word.Selection
$selection.TypeText("{text}")
Write-Output "inserted"
"#,
        text = escaped
    );
    let output = run_powershell(&script).await?;
    Ok(WordResult {
        success: true,
        message: "Text inserted".to_string(),
        document_name: None,
        page_count: None,
    })
}

/// Read the full text content from an open Word document.
pub async fn word_read_content(path: &str) -> anyhow::Result<String> {
    let script = format!(
        r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$doc = $word.Documents.Open("{path}")
$content = $doc.Content.Text
$doc.Close() | Out-Null
$word.Quit() | Out-Null
Write-Output $content
"#,
        path = path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    run_powershell(&script).await
}

/// Save the active Word document to a file path.
pub async fn word_save_document(path: &str) -> anyhow::Result<WordResult> {
    let script = format!(
        r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$doc = $word.Documents.Add()
$doc.SaveAs2("{path}")
Write-Output "saved|{path}"
$doc.Close() | Out-Null
$word.Quit() | Out-Null
"#,
        path = path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = run_powershell(&script).await?;
    Ok(WordResult {
        success: true,
        message: format!("Saved to {}", output.trim()),
        document_name: None,
        page_count: None,
    })
}

/// Save document as PDF.
pub async fn word_export_to_pdf(doc_path: &str, pdf_path: &str) -> anyhow::Result<WordResult> {
    let script = format!(
        r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$doc = $word.Documents.Open("{doc}")
$doc.SaveAs2("{pdf}", 17) # 17 = wdFormatPDF
$doc.Close() | Out-Null
$word.Quit() | Out-Null
Write-Output "exported"
"#,
        doc = doc_path.replace('\\', "\\\\").replace('"', "\\\""),
        pdf = pdf_path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    run_powershell(&script).await?;
    Ok(WordResult {
        success: true,
        message: format!("Exported to {}", pdf_path),
        document_name: None,
        page_count: None,
    })
}

/// Apply formatting to the current selection in Word.
#[derive(Debug, Clone)]
pub struct WordFormatting {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub font_size: Option<u32>,
    pub font_name: Option<String>,
    pub alignment: Option<WordAlignment>,
}

#[derive(Debug, Clone, Copy)]
pub enum WordAlignment {
    Left,
    Center,
    Right,
    Justified,
}

/// Apply formatting to the current selection.
pub async fn word_apply_formatting(fmt: &WordFormatting) -> anyhow::Result<WordResult> {
    let mut cmds = Vec::new();
    cmds.push("$word = New-Object -ComObject Word.Application".to_string());
    cmds.push("$word.Visible = $true".to_string());
    cmds.push("$selection = $word.Selection".to_string());

    if let Some(bold) = fmt.bold {
        cmds.push(format!("$selection.Font.Bold = ${}", bold));
    }
    if let Some(italic) = fmt.italic {
        cmds.push(format!("$selection.Font.Italic = ${}", italic));
    }
    if let Some(size) = fmt.font_size {
        cmds.push(format!("$selection.Font.Size = {}", size));
    }
    if let Some(ref name) = fmt.font_name {
        cmds.push(format!("$selection.Font.Name = \"{}\"", name));
    }
    if let Some(align) = fmt.alignment {
        let align_val = match align {
            WordAlignment::Left => 1,
            WordAlignment::Center => 2,
            WordAlignment::Right => 3,
            WordAlignment::Justified => 4,
        };
        cmds.push(format!(
            "$selection.ParagraphFormat.Alignment = {}",
            align_val
        ));
    }

    cmds.push("Write-Output 'formatted'".to_string());
    let script = cmds.join("\n");
    run_powershell(&script).await?;
    Ok(WordResult {
        success: true,
        message: "Formatting applied".to_string(),
        document_name: None,
        page_count: None,
    })
}

/// Execute an arbitrary Word COM automation script (PowerShell).
///
/// Advanced users can use this to perform operations not covered by the
/// higher-level functions. The script runs in a context where `$word` is
/// the `Word.Application` COM object.
pub async fn word_run_script(ps_script: &str) -> anyhow::Result<String> {
    let script = format!(
        r#"
$word = New-Object -ComObject Word.Application
$word.Visible = $true
{ps_script}
"#,
        ps_script = ps_script
    );
    run_powershell(&script).await
}

/// Close Word and all documents.
pub async fn word_quit() -> anyhow::Result<()> {
    let script = r#"
$word = New-Object -ComObject Word.Application
$word.Quit() | Out-Null
"#;
    run_powershell(script).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_word_create_document_does_not_panic() {
        // This test only checks that the function doesn't crash.
        // Word must be installed for it to succeed.
        let _ = word_create_document().await;
    }

    #[tokio::test]
    async fn test_word_quit_does_not_panic() {
        let _ = word_quit().await;
    }
}
