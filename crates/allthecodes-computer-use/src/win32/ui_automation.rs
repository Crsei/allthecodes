//! Windows UI Automation tree navigation.
//!
//! Uses PowerShell to access the Windows UI Automation API for discovering
//! and interacting with UI elements across applications. Provides:
//!
//! - Finding elements by name, class, control type, or automation ID
//! - Walking the accessibility tree (parent, siblings, children)
//! - Reading element properties (name, role, value, bounds, state)
//! - Performing actions (click, focus, expand/collapse)
//! - Taking accessibility snapshots

use super::shared::run_powershell;

/// Control types commonly used in UI Automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlType {
    Button,
    CheckBox,
    ComboBox,
    Edit,
    Hyperlink,
    Image,
    List,
    ListItem,
    Menu,
    MenuItem,
    Pane,
    ProgressBar,
    RadioButton,
    ScrollBar,
    Slider,
    Spinner,
    StatusBar,
    Tab,
    TabItem,
    Table,
    Text,
    Thumb,
    TitleBar,
    ToolBar,
    ToolTip,
    Tree,
    TreeItem,
    Window,
    Custom,
    Unknown,
}

impl ControlType {
    /// Parse from UI Automation control type ID.
    pub fn from_id(id: i32) -> Self {
        match id {
            50000 => ControlType::Button,
            50001 => ControlType::CheckBox,
            50003 => ControlType::ComboBox,
            50004 => ControlType::Edit,
            50005 => ControlType::Hyperlink,
            50006 => ControlType::Image,
            50008 => ControlType::List,
            50009 => ControlType::ListItem,
            50010 => ControlType::Menu,
            50011 => ControlType::MenuItem,
            50033 => ControlType::Pane,
            50034 => ControlType::ProgressBar,
            50035 => ControlType::RadioButton,
            50036 => ControlType::ScrollBar,
            50037 => ControlType::Slider,
            50038 => ControlType::Spinner,
            50039 => ControlType::StatusBar,
            50040 => ControlType::Tab,
            50041 => ControlType::TabItem,
            50042 => ControlType::Table,
            50044 => ControlType::Text,
            50045 => ControlType::Thumb,
            50046 => ControlType::TitleBar,
            50047 => ControlType::ToolBar,
            50048 => ControlType::ToolTip,
            50049 => ControlType::Tree,
            50050 => ControlType::TreeItem,
            50032 => ControlType::Window,
            _ => ControlType::Unknown,
        }
    }
}

/// A discovered UI element with its properties.
#[derive(Debug, Clone)]
pub struct UiElement {
    pub name: String,
    pub control_type: String,
    pub automation_id: String,
    pub class_name: String,
    pub bounds: Option<(i32, i32, i32, i32)>, // left, top, right, bottom
    pub is_enabled: bool,
    pub is_visible: bool,
    pub is_offscreen: bool,
    pub has_keyboard_focus: bool,
    pub value: Option<String>,
    pub children_count: u32,
}

/// Find UI elements by name (partial match) within a window or the desktop.
///
/// If `window_name` is `None`, searches from the desktop root.
pub async fn find_elements_by_name(
    name: &str,
    window_name: Option<&str>,
) -> anyhow::Result<Vec<UiElement>> {
    let window_condition = match window_name {
        Some(wn) => format!(
            r#"
$cond = New-Object System.Windows.Automation.Condition
$cond = $automation.CreatePropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "{wn}")
$root = $automation.GetRootElement()
$window = $root.FindFirst([System.Windows.Automation.TreeScope]::Subtree, $cond)
if (-not $window) {{ Write-Output "WINDOW_NOT_FOUND"; exit }}
$scope = $window
"#
        ),
        None => "$scope = $automation.GetRootElement()".to_string(),
    };

    let script = format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$automation = New-Object System.Windows.Automation.Automation
{window_condition}

$cond = $automation.CreatePropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "{name}")
$elements = $scope.FindAll([System.Windows.Automation.TreeScope]::Subtree, $cond)

foreach ($elem in $elements) {{
    $bounds = $elem.Current.BoundingRectangle
    $valPattern = $elem.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $val = if ($valPattern) {{ $valPattern.Current.Value }} else {{ "" }}
    Write-Output "ELEMENT|$($elem.Current.Name)|$($elem.Current.ControlType.ProgrammaticName)|$($elem.Current.AutomationId)|$($elem.Current.ClassName)|$($bounds.Left)|$($bounds.Top)|$($bounds.Right)|$($bounds.Bottom)|$($elem.Current.IsEnabled)|$($elem.Current.IsOffscreen)|$($elem.Current.HasKeyboardFocus)|$val"
}}
"#,
        window_condition = window_condition,
        name = name.replace('"', "\\\"")
    );

    let output = run_powershell(&script).await?;
    Ok(parse_elements_output(&output))
}

/// Find the first UI element matching a condition.
pub async fn find_first_element(
    property: &str,
    value: &str,
    scope: Option<&str>,
) -> anyhow::Result<Option<UiElement>> {
    let script = format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$automation = New-Object System.Windows.Automation.Automation
$root = $automation.GetRootElement()

$cond = $automation.CreatePropertyCondition([System.Windows.Automation.AutomationElement]::{property}Property, "{value}")
$elem = $root.FindFirst([System.Windows.Automation.TreeScope]::{scope}, $cond)

if ($elem) {{
    $bounds = $elem.Current.BoundingRectangle
    Write-Output "FOUND|$($elem.Current.Name)|$($elem.Current.ControlType.ProgrammaticName)|$($elem.Current.AutomationId)|$($bounds.Left)|$($bounds.Top)|$($bounds.Right)|$($bounds.Bottom)|$($elem.Current.IsEnabled)|$($elem.Current.IsOffscreen)"
}} else {{
    Write-Output "NOT_FOUND"
}}
"#,
        property = property,
        value = value.replace('"', "\\\""),
        scope = scope.unwrap_or("Subtree"),
    );

    let output = run_powershell(&script).await?;
    let elements = parse_elements_output(&output);
    Ok(elements.into_iter().next())
}

/// Get the children of a specific UI element.
pub async fn get_element_children(parent_name: &str) -> anyhow::Result<Vec<UiElement>> {
    let script = format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$automation = New-Object System.Windows.Automation.Automation
$root = $automation.GetRootElement()
$cond = $automation.CreatePropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "{name}")
$parent = $root.FindFirst([System.Windows.Automation.TreeScope]::Subtree, $cond)

if (-not $parent) {{ Write-Output "PARENT_NOT_FOUND"; exit }}

$children = $parent.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($elem in $children) {{
    $bounds = $elem.Current.BoundingRectangle
    Write-Output "ELEMENT|$($elem.Current.Name)|$($elem.Current.ControlType.ProgrammaticName)|$($elem.Current.AutomationId)|$($elem.Current.ClassName)|$($bounds.Left)|$($bounds.Top)|$($bounds.Right)|$($bounds.Bottom)|$($elem.Current.IsEnabled)|$($elem.Current.IsOffscreen)|$($elem.Current.HasKeyboardFocus)|"
}}
"#,
        name = parent_name.replace('"', "\\\"")
    );

    let output = run_powershell(&script).await?;
    Ok(parse_elements_output(&output))
}

/// Get the accessibility tree root (desktop).
pub async fn get_desktop_root() -> anyhow::Result<UiElement> {
    let script = r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$automation = New-Object System.Windows.Automation.Automation
$root = $automation.GetRootElement()
Write-Output "ROOT|$($root.Current.Name)|Desktop|$($root.Current.AutomationId)|$($root.Current.ClassName)"
"#;
    let output = run_powershell(script).await?;
    let elements = parse_elements_output(&output);
    elements
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No desktop root found"))
}

/// Click a UI element by name.
pub async fn click_element_by_name(name: &str, window_name: Option<&str>) -> anyhow::Result<bool> {
    let elements = find_elements_by_name(name, window_name).await?;
    if let Some(elem) = elements.first() {
        if let Some((left, top, right, bottom)) = elem.bounds {
            let cx = (left + right) / 2;
            let cy = (top + bottom) / 2;
            // Use our input module to click at the center of the element
            crate::input::execute_input(crate::input::InputAction::Click {
                x: cx,
                y: cy,
                button: crate::input::MouseButton::Left,
            })
            .await?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Take an accessibility snapshot: dump the full UI Automation tree as text.
pub async fn accessibility_snapshot(depth: u32) -> anyhow::Result<String> {
    let script = format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function WalkTree($element, $depth) {{
    if ($depth -gt {max_depth}) {{ return }}
    $indent = "  " * $depth
    $bounds = $element.Current.BoundingRectangle
    $info = "$indent$($element.Current.ControlType.ProgrammaticName): `"$($element.Current.Name)`" [$($bounds.Left),$($bounds.Top) - $($bounds.Right),$($bounds.Bottom)]"
    Write-Output $info
    $children = $element.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($child in $children) {{
        WalkTree $child ($depth + 1)
    }}
}}

$automation = New-Object System.Windows.Automation.Automation
$root = $automation.GetRootElement()
WalkTree $root 0
"#,
        max_depth = depth
    );

    run_powershell(&script).await
}

/// Parse the "|"-delimited output format from PowerShell UIA scripts.
fn parse_elements_output(output: &str) -> Vec<UiElement> {
    let mut elements = Vec::new();
    for line in output.lines() {
        if !line.starts_with("ELEMENT|")
            && !line.starts_with("FOUND|")
            && !line.starts_with("ROOT|")
        {
            continue;
        }
        let parts: Vec<&str> = line.splitn(13, '|').collect();
        if parts.len() < 5 {
            continue;
        }
        let name = parts.get(1).unwrap_or(&"").to_string();
        let ctrl_type = parts.get(2).unwrap_or(&"").to_string();
        let automation_id = parts.get(3).unwrap_or(&"").to_string();
        let class_name = parts.get(4).unwrap_or(&"").to_string();

        let bounds = if parts.len() >= 8 {
            let left = parts.get(5).and_then(|s| s.parse::<i32>().ok());
            let top = parts.get(6).and_then(|s| s.parse::<i32>().ok());
            let right = parts.get(7).and_then(|s| s.parse::<i32>().ok());
            let bottom = parts.get(8).and_then(|s| s.parse::<i32>().ok());
            match (left, top, right, bottom) {
                (Some(l), Some(t), Some(r), Some(b)) => Some((l, t, r, b)),
                _ => None,
            }
        } else {
            None
        };

        let is_enabled = parts
            .get(8)
            .map(|s| *s == "True" || *s == "true")
            .unwrap_or(true);
        let is_offscreen = parts
            .get(9)
            .map(|s| *s == "True" || *s == "true")
            .unwrap_or(false);
        let has_keyboard_focus = parts
            .get(10)
            .map(|s| *s == "True" || *s == "true")
            .unwrap_or(false);
        let value = parts
            .get(11)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        elements.push(UiElement {
            name,
            control_type: ctrl_type,
            automation_id,
            class_name,
            bounds,
            is_enabled,
            is_visible: !is_offscreen,
            is_offscreen,
            has_keyboard_focus,
            value,
            children_count: 0,
        });
    }
    elements
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_type_from_id() {
        assert_eq!(ControlType::from_id(50000), ControlType::Button);
        assert_eq!(ControlType::from_id(50032), ControlType::Window);
        assert_eq!(ControlType::from_id(99999), ControlType::Unknown);
    }

    #[tokio::test]
    async fn test_get_desktop_root_does_not_panic() {
        // May fail on non-Windows or if UIA is not available.
        let _ = get_desktop_root().await;
    }

    #[test]
    fn test_parse_elements_output_empty() {
        let elements = parse_elements_output("");
        assert!(elements.is_empty());
    }
}
