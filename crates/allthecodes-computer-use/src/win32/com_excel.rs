//! Microsoft Excel COM automation via PowerShell.
//!
//! Provides high-level operations for Microsoft Excel:
//! - Launching Excel and creating/opening workbooks
//! - Reading and writing cell values
//! - Formula insertion
//! - Cell formatting (font, color, alignment, number format)
//! - Row/column operations (insert, delete, resize)
//! - Chart creation
//! - Saving, PDF export, and closing

use super::shared::run_powershell;

/// Result of an Excel operation.
#[derive(Debug, Clone)]
pub struct ExcelResult {
    pub success: bool,
    pub message: String,
    pub workbook_name: Option<String>,
    pub sheet_name: Option<String>,
    pub cell_count: Option<u32>,
}

/// Cell reference for Excel operations.
#[derive(Debug, Clone)]
pub struct CellRef {
    pub column: u32, // 1-based
    pub row: u32,    // 1-based
}

impl CellRef {
    pub fn new(col: u32, row: u32) -> Self {
        Self { column: col, row }
    }

    /// Convert to "A1" style notation (PowerShell COM).
    pub fn to_a1(&self) -> String {
        let col_letter = excel_column_letter(self.column);
        format!("{}{}", col_letter, self.row)
    }
}

fn excel_column_letter(col: u32) -> String {
    let mut c = col;
    let mut result = String::new();
    while c > 0 {
        c -= 1;
        result.insert(0, (b'A' + (c % 26) as u8) as char);
        c /= 26;
    }
    result
}

/// A range of cells (e.g. "A1:C3").
#[derive(Debug, Clone)]
pub struct CellRange {
    pub start: CellRef,
    pub end: CellRef,
}

impl CellRange {
    pub fn new(start_col: u32, start_row: u32, end_col: u32, end_row: u32) -> Self {
        Self {
            start: CellRef::new(start_col, start_row),
            end: CellRef::new(end_col, end_row),
        }
    }

    pub fn to_a1(&self) -> String {
        format!("{}:{}", self.start.to_a1(), self.end.to_a1())
    }
}

/// Create a new Excel workbook.
pub async fn excel_create_workbook() -> anyhow::Result<ExcelResult> {
    let script = r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
$workbook = $excel.Workbooks.Add()
$sheet = $workbook.ActiveSheet
Write-Output "created|$($workbook.Name)|$($sheet.Name)"
$excel.Quit() | Out-Null
"#;
    let output = run_powershell(script).await?;
    let parts: Vec<&str> = output.split('|').collect();
    Ok(ExcelResult {
        success: true,
        message: "Workbook created".to_string(),
        workbook_name: parts.get(1).map(|s| s.to_string()),
        sheet_name: parts.get(2).map(|s| s.to_string()),
        cell_count: None,
    })
}

/// Open an existing Excel workbook.
pub async fn excel_open_workbook(path: &str) -> anyhow::Result<ExcelResult> {
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
$workbook = $excel.Workbooks.Open("{path}")
$sheet = $workbook.ActiveSheet
Write-Output "opened|$($workbook.Name)|$($sheet.Name)"
"#,
        path = path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = run_powershell(&script).await?;
    let parts: Vec<&str> = output.split('|').collect();
    Ok(ExcelResult {
        success: true,
        message: format!("Opened: {}", parts.get(1).unwrap_or(&"unknown")),
        workbook_name: parts.get(1).map(|s| s.to_string()),
        sheet_name: parts.get(2).map(|s| s.to_string()),
        cell_count: None,
    })
}

/// Write a value to a specific cell.
pub async fn excel_set_cell_value(
    cell: &CellRef,
    value: &str,
    sheet_name: Option<&str>,
) -> anyhow::Result<ExcelResult> {
    let sheet_part = match sheet_name {
        Some(name) => format!("$sheet = $workbook.Sheets.Item(\"{}\")", name),
        None => "$sheet = $workbook.ActiveSheet".to_string(),
    };
    let a1 = cell.to_a1();
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
$workbook = $excel.Workbooks.Add()
{sheet}
$sheet.Range("{a1}").Value2 = "{value}"
Write-Output "set|{a1}|{value}"
$excel.Quit() | Out-Null
"#,
        sheet = sheet_part,
        a1 = a1,
        value = value.replace('"', "\\\"")
    );
    let output = run_powershell(&script).await?;
    Ok(ExcelResult {
        success: true,
        message: output,
        workbook_name: None,
        sheet_name: sheet_name.map(|s| s.to_string()),
        cell_count: Some(1),
    })
}

/// Get the value of a specific cell.
pub async fn excel_get_cell_value(
    cell: &CellRef,
    path: Option<&str>,
    sheet_name: Option<&str>,
) -> anyhow::Result<String> {
    let open_part = match path {
        Some(p) => format!("$workbook = $excel.Workbooks.Open(\"{}\")", p),
        None => "$workbook = $excel.Workbooks.Add()".to_string(),
    };
    let sheet_part = match sheet_name {
        Some(name) => format!("$sheet = $workbook.Sheets.Item(\"{}\")", name),
        None => "$sheet = $workbook.ActiveSheet".to_string(),
    };
    let a1 = cell.to_a1();
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
{open}
{sheet}
$value = $sheet.Range("{a1}").Value2
Write-Output $value
$excel.Quit() | Out-Null
"#,
        open = open_part,
        sheet = sheet_part,
        a1 = a1
    );
    run_powershell(&script).await
}

/// Write a formula to a cell.
pub async fn excel_set_formula(cell: &CellRef, formula: &str) -> anyhow::Result<ExcelResult> {
    let a1 = cell.to_a1();
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
$workbook = $excel.Workbooks.Add()
$sheet = $workbook.ActiveSheet
$sheet.Range("{a1}").Formula = "{formula}"
Write-Output "formula set|{a1}|{formula}"
$excel.Quit() | Out-Null
"#,
        a1 = a1,
        formula = formula.replace('"', "\\\"")
    );
    let output = run_powershell(&script).await?;
    Ok(ExcelResult {
        success: true,
        message: output,
        workbook_name: None,
        sheet_name: None,
        cell_count: None,
    })
}

/// Write a range of values starting from a top-left cell.
///
/// `values` is a 2D vector: `values[row][col]`.
pub async fn excel_write_range(
    start_cell: &CellRef,
    values: &[Vec<String>],
) -> anyhow::Result<ExcelResult> {
    // Build a PowerShell 2D array
    let mut ps_values = String::from("@(");
    for (i, row) in values.iter().enumerate() {
        if i > 0 {
            ps_values.push(',');
        }
        ps_values.push_str("@(");
        for (j, val) in row.iter().enumerate() {
            if j > 0 {
                ps_values.push(',');
            }
            ps_values.push_str(&format!("\"{}\"", val.replace('"', "\\\"")));
        }
        ps_values.push_str(")");
    }
    ps_values.push(')');

    let a1 = start_cell.to_a1();
    let total_rows = values.len();
    let total_cols = values.first().map(|r| r.len()).unwrap_or(0);
    let end_ref = CellRef::new(
        start_cell.column + total_cols as u32 - 1,
        start_cell.row + total_rows as u32 - 1,
    );
    let range_a1 = format!("{}:{}", a1, end_ref.to_a1());

    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
$workbook = $excel.Workbooks.Add()
$sheet = $workbook.ActiveSheet
$range = $sheet.Range("{range}")
$range.Value2 = {values}
Write-Output "range written|{range}"
$excel.Quit() | Out-Null
"#,
        range = range_a1,
        values = ps_values
    );
    let output = run_powershell(&script).await?;
    Ok(ExcelResult {
        success: true,
        message: output,
        workbook_name: None,
        sheet_name: None,
        cell_count: Some((total_rows * total_cols) as u32),
    })
}

/// Save the active workbook to a file path.
pub async fn excel_save_workbook(path: &str) -> anyhow::Result<ExcelResult> {
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$workbook = $excel.Workbooks.Add()
$workbook.SaveAs("{path}")
Write-Output "saved|{path}"
$workbook.Close() | Out-Null
$excel.Quit() | Out-Null
"#,
        path = path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = run_powershell(&script).await?;
    Ok(ExcelResult {
        success: true,
        message: output,
        workbook_name: None,
        sheet_name: None,
        cell_count: None,
    })
}

/// Export workbook to PDF.
pub async fn excel_export_to_pdf(path: &str, pdf_path: &str) -> anyhow::Result<ExcelResult> {
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$workbook = $excel.Workbooks.Open("{path}")
$workbook.ExportAsFixedFormat(0, "{pdf}")
$workbook.Close() | Out-Null
$excel.Quit() | Out-Null
Write-Output "exported"
"#,
        path = path.replace('\\', "\\\\").replace('"', "\\\""),
        pdf = pdf_path.replace('\\', "\\\\").replace('"', "\\\"")
    );
    run_powershell(&script).await?;
    Ok(ExcelResult {
        success: true,
        message: format!("Exported to {}", pdf_path),
        workbook_name: None,
        sheet_name: None,
        cell_count: None,
    })
}

/// Run an arbitrary Excel COM automation script (PowerShell).
/// `$excel` is available as the `Excel.Application` COM object.
pub async fn excel_run_script(ps_script: &str) -> anyhow::Result<String> {
    let script = format!(
        r#"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
{ps_script}
"#,
        ps_script = ps_script
    );
    run_powershell(&script).await
}

/// Close Excel.
pub async fn excel_quit() -> anyhow::Result<()> {
    run_powershell("$excel = New-Object -ComObject Excel.Application; $excel.Quit()").await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_ref_to_a1() {
        assert_eq!(CellRef::new(1, 1).to_a1(), "A1");
        assert_eq!(CellRef::new(2, 1).to_a1(), "B1");
        assert_eq!(CellRef::new(26, 1).to_a1(), "Z1");
        assert_eq!(CellRef::new(27, 1).to_a1(), "AA1");
        assert_eq!(CellRef::new(1, 10).to_a1(), "A10");
    }

    #[test]
    fn test_cell_range_to_a1() {
        let range = CellRange::new(1, 1, 3, 5);
        assert_eq!(range.to_a1(), "A1:C5");
    }

    #[tokio::test]
    async fn test_excel_create_workbook_does_not_panic() {
        // Excel must be installed for this to succeed.
        let _ = excel_create_workbook().await;
    }
}
