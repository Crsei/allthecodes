//! Native PowerShell AST metadata used by the PowerShell execution gate.
//!
//! The TypeScript upstream does not rely on text matching alone for
//! PowerShell. It asks PowerShell's own parser for command element types,
//! colon-bound parameter children, raw command-name classification, and
//! statement-level security patterns. This module keeps the Rust tool on the
//! same side of that boundary for execution-time validation.

use anyhow::{anyhow, Result};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const POWERSHELL_PARSE_TIMEOUT: Duration = Duration::from_millis(5_000);
const POWERSHELL_PARSE_INPUT_ENV: &str = "ALLTHECODES_POWERSHELL_PARSE_INPUT";

const POWERSHELL_AST_METADATA_SCRIPT: &str = r#"
$tokens = $null
$parseErrors = $null
$source = $env:ALLTHECODES_POWERSHELL_PARSE_INPUT
$ast = [System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$parseErrors)

function Convert-ElementType($node) {
    if ($null -eq $node) { return 'Other' }
    switch ($node.GetType().Name) {
        'ScriptBlockExpressionAst' { return 'ScriptBlock' }
        'SubExpressionAst' { return 'SubExpression' }
        'ArrayExpressionAst' { return 'SubExpression' }
        'ExpandableStringExpressionAst' { return 'ExpandableString' }
        'InvokeMemberExpressionAst' { return 'MemberInvocation' }
        'VariableExpressionAst' { return 'Variable' }
        'StringConstantExpressionAst' { return 'StringConstant' }
        'ConstantExpressionAst' { return 'StringConstant' }
        'CommandParameterAst' { return 'Parameter' }
        'ParenExpressionAst' { return 'SubExpression' }
        'CommandExpressionAst' {
            if ($node.Expression) { return Convert-ElementType $node.Expression }
            return 'Other'
        }
        default { return 'Other' }
    }
}

function Get-SecurityPatterns($node) {
    $patterns = @{}
    foreach ($found in $node.FindAll({ param($x)
        $x -is [System.Management.Automation.Language.InvokeMemberExpressionAst] -or
        $x -is [System.Management.Automation.Language.SubExpressionAst] -or
        $x -is [System.Management.Automation.Language.ArrayExpressionAst] -or
        $x -is [System.Management.Automation.Language.ExpandableStringExpressionAst] -or
        $x -is [System.Management.Automation.Language.ScriptBlockExpressionAst] -or
        $x -is [System.Management.Automation.Language.ParenExpressionAst]
    }, $true)) {
        switch ($found.GetType().Name) {
            'InvokeMemberExpressionAst' { $patterns.hasMemberInvocations = $true }
            'SubExpressionAst' { $patterns.hasSubExpressions = $true }
            'ArrayExpressionAst' { $patterns.hasSubExpressions = $true }
            'ParenExpressionAst' { $patterns.hasSubExpressions = $true }
            'ExpandableStringExpressionAst' { $patterns.hasExpandableStrings = $true }
            'ScriptBlockExpressionAst' { $patterns.hasScriptBlocks = $true }
        }
    }
    if ($patterns.Count -gt 0) { return $patterns }
    return $null
}

function Get-CommandMetadata([System.Management.Automation.Language.CommandAst]$cmd) {
    $elements = [System.Collections.ArrayList]::new()
    foreach ($ce in $cmd.CommandElements) {
        $item = @{
            type = $ce.GetType().Name
            text = $ce.Extent.Text
            elementType = Convert-ElementType $ce
        }
        if ($ce.PSObject.Properties['Value'] -and $null -ne $ce.Value -and $ce.Value -is [string]) {
            $item.value = $ce.Value
        }
        if ($ce -is [System.Management.Automation.Language.CommandExpressionAst]) {
            $item.expressionType = $ce.Expression.GetType().Name
        }
        $arg = $ce.Argument
        if ($arg) {
            $item.children = @(@{
                type = $arg.GetType().Name
                text = $arg.Extent.Text
                elementType = Convert-ElementType $arg
            })
        }
        [void]$elements.Add($item)
    }
    return @{
        text = $cmd.Extent.Text
        elements = @($elements)
    }
}

function Get-StatementMetadata($stmt) {
    $commands = [System.Collections.ArrayList]::new()
    foreach ($cmd in $stmt.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] }, $true)) {
        [void]$commands.Add((Get-CommandMetadata $cmd))
    }
    $result = @{
        type = $stmt.GetType().Name
        text = $stmt.Extent.Text
        commands = @($commands)
    }
    $patterns = Get-SecurityPatterns $stmt
    if ($patterns) { $result.securityPatterns = $patterns }
    return $result
}

$statements = [System.Collections.ArrayList]::new()
foreach ($block in @($ast.BeginBlock, $ast.ProcessBlock, $ast.EndBlock, $ast.CleanBlock, $ast.DynamicParamBlock)) {
    if ($block) {
        foreach ($stmt in $block.Statements) {
            [void]$statements.Add((Get-StatementMetadata $stmt))
        }
    }
}
if ($ast.ParamBlock) {
    $paramPatterns = Get-SecurityPatterns $ast.ParamBlock
    if ($paramPatterns) {
        [void]$statements.Add(@{
            type = 'ParamBlockAst'
            text = $ast.ParamBlock.Extent.Text
            commands = @()
            securityPatterns = $paramPatterns
        })
    }
}

$variables = [System.Collections.ArrayList]::new()
foreach ($v in $ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.VariableExpressionAst] }, $true)) {
    [void]$variables.Add(@{
        path = $v.VariablePath.ToString()
        isSplatted = [bool]$v.Splatted
    })
}

$hasStopParsing = $false
foreach ($tok in @($tokens)) {
    $kind = $tok.Kind.ToString()
    $text = $tok.Text -replace '[\u2013\u2014\u2015]', '-'
    if ($kind -eq 'MinusMinus' -or ($kind -eq 'Generic' -and $text -eq '--%')) {
        $hasStopParsing = $true
        break
    }
}

$errors = [System.Collections.ArrayList]::new()
foreach ($err in @($parseErrors)) {
    [void]$errors.Add(@{
        message = $err.Message
        errorId = $err.ErrorId
    })
}

$hasUsingStatements = $false
if ($ast.PSObject.Properties['UsingStatements'] -and $ast.UsingStatements) {
    $hasUsingStatements = @($ast.UsingStatements).Count -gt 0
}
$hasScriptRequirements = $false
if ($ast.PSObject.Properties['ScriptRequirements'] -and $ast.ScriptRequirements) {
    $hasScriptRequirements = $true
}

$output = @{
    valid = (@($parseErrors).Count -eq 0)
    errors = @($errors)
    statements = @($statements)
    variables = @($variables)
    hasStopParsing = $hasStopParsing
    hasUsingStatements = $hasUsingStatements
    hasScriptRequirements = $hasScriptRequirements
}
ConvertTo-Json -InputObject $output -Compress -Depth 16
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PowerShellAstAnalysis {
    pub parser_errors: Vec<String>,
    pub security_diagnostics: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ParsedPowerShellAst {
    #[serde(default)]
    valid: bool,
    #[serde(default, deserialize_with = "vec_or_single")]
    errors: Vec<ParseError>,
    #[serde(default, deserialize_with = "vec_or_single")]
    statements: Vec<ParsedStatement>,
    #[serde(default, deserialize_with = "vec_or_single")]
    variables: Vec<ParsedVariable>,
    #[serde(default, rename = "hasStopParsing")]
    has_stop_parsing: bool,
    #[serde(default, rename = "hasUsingStatements")]
    has_using_statements: bool,
    #[serde(default, rename = "hasScriptRequirements")]
    has_script_requirements: bool,
}

#[derive(Debug, Deserialize)]
struct ParseError {
    #[serde(default)]
    message: String,
    #[serde(default, rename = "errorId")]
    error_id: String,
}

#[derive(Debug, Deserialize)]
struct ParsedStatement {
    #[serde(default, deserialize_with = "vec_or_single")]
    commands: Vec<ParsedCommand>,
    #[serde(default, rename = "securityPatterns")]
    security_patterns: Option<SecurityPatterns>,
}

#[derive(Debug, Deserialize)]
struct ParsedCommand {
    #[serde(default, deserialize_with = "vec_or_single")]
    elements: Vec<ParsedElement>,
}

#[derive(Debug, Deserialize)]
struct ParsedElement {
    #[serde(default, rename = "type")]
    raw_type: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default, rename = "elementType")]
    element_type: String,
    #[serde(default, deserialize_with = "vec_or_single")]
    children: Vec<ParsedChild>,
}

#[derive(Debug, Deserialize)]
struct ParsedChild {
    #[serde(default, rename = "elementType")]
    element_type: String,
}

#[derive(Debug, Deserialize)]
struct ParsedVariable {
    #[serde(default, rename = "isSplatted")]
    is_splatted: bool,
}

#[derive(Debug, Default, Deserialize)]
struct SecurityPatterns {
    #[serde(default, rename = "hasMemberInvocations")]
    has_member_invocations: bool,
    #[serde(default, rename = "hasSubExpressions")]
    has_sub_expressions: bool,
    #[serde(default, rename = "hasExpandableStrings")]
    has_expandable_strings: bool,
    #[serde(default, rename = "hasScriptBlocks")]
    has_script_blocks: bool,
}

fn vec_or_single<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| T::deserialize(value).map_err(D::Error::custom))
            .collect(),
        other => T::deserialize(other)
            .map(|single| vec![single])
            .map_err(D::Error::custom),
    }
}

pub(super) async fn analyze(command: &str) -> Result<PowerShellAstAnalysis> {
    let mut parser = tokio::process::Command::new(powershell_executable());
    parser
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(POWERSHELL_AST_METADATA_SCRIPT)
        .env(POWERSHELL_PARSE_INPUT_ENV, command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let output = match tokio::time::timeout(POWERSHELL_PARSE_TIMEOUT, parser.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => return Err(anyhow!("failed to start PowerShell parser: {}", err)),
        Err(_) => return Err(anyhow!("PowerShell parser timed out")),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!(
            "PowerShell parser exited with status {}{}",
            output.status,
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {}", stderr)
            }
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("PowerShell parser returned empty output"));
    }

    let parsed: ParsedPowerShellAst = serde_json::from_str(trimmed)
        .map_err(|err| anyhow!("failed to parse PowerShell parser output: {}", err))?;
    let parser_errors = parsed
        .errors
        .iter()
        .map(|error| {
            if error.error_id.is_empty() {
                error.message.clone()
            } else {
                format!("{}: {}", error.error_id, error.message)
            }
        })
        .collect::<Vec<_>>();
    let security_diagnostics = if parsed.valid {
        security_diagnostics(&parsed)
    } else {
        Vec::new()
    };

    Ok(PowerShellAstAnalysis {
        parser_errors,
        security_diagnostics,
    })
}

fn powershell_executable() -> &'static str {
    if cfg!(target_os = "windows") {
        "powershell.exe"
    } else {
        "pwsh"
    }
}

fn security_diagnostics(parsed: &ParsedPowerShellAst) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();

    if parsed.has_stop_parsing {
        push_unique(
            &mut diagnostics,
            &mut seen,
            "PowerShell stop-parsing token prevents static validation",
        );
    }
    if parsed.has_using_statements {
        push_unique(
            &mut diagnostics,
            &mut seen,
            "PowerShell using statements can load external modules or assemblies",
        );
    }
    if parsed.has_script_requirements {
        push_unique(
            &mut diagnostics,
            &mut seen,
            "PowerShell #Requires directives can load modules before execution",
        );
    }
    if parsed.variables.iter().any(|variable| variable.is_splatted) {
        push_unique(
            &mut diagnostics,
            &mut seen,
            "PowerShell splatting obscures command arguments",
        );
    }

    for statement in &parsed.statements {
        if let Some(patterns) = &statement.security_patterns {
            if patterns.has_sub_expressions {
                push_unique(
                    &mut diagnostics,
                    &mut seen,
                    "PowerShell statement contains subexpressions that can hide command execution",
                );
            }
            if patterns.has_expandable_strings {
                push_unique(
                    &mut diagnostics,
                    &mut seen,
                    "PowerShell statement contains expandable strings with runtime expressions",
                );
            }
            if patterns.has_member_invocations {
                push_unique(
                    &mut diagnostics,
                    &mut seen,
                    "PowerShell statement invokes .NET members",
                );
            }
            if patterns.has_script_blocks && !script_blocks_are_safe(statement) {
                push_unique(
                    &mut diagnostics,
                    &mut seen,
                    "PowerShell statement contains a script block that cannot be statically validated",
                );
            }
        }

        for command in &statement.commands {
            validate_command(command, &mut diagnostics, &mut seen);
        }
    }

    diagnostics
}

fn validate_command(
    command: &ParsedCommand,
    diagnostics: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    let Some(name_element) = command.elements.first() else {
        return;
    };

    if name_element.element_type != "StringConstant" {
        push_unique(
            diagnostics,
            seen,
            "PowerShell command name is a dynamic expression which cannot be statically validated",
        );
    }

    if let Some(raw_name) = raw_command_name(name_element) {
        if let Some(reason) = application_command_name_rejection(&raw_name) {
            push_unique(diagnostics, seen, reason);
        }
    }

    for element in command.elements.iter().skip(1) {
        match element.element_type.as_str() {
            "ScriptBlock" => {
                if !command_name(command)
                    .as_deref()
                    .is_some_and(is_safe_script_block_consumer)
                {
                    push_unique(
                        diagnostics,
                        seen,
                        "PowerShell command argument contains a script block",
                    );
                }
            }
            "SubExpression" => push_unique(
                diagnostics,
                seen,
                "PowerShell command argument contains a subexpression",
            ),
            "ExpandableString" => push_unique(
                diagnostics,
                seen,
                "PowerShell command argument contains an expandable string",
            ),
            "MemberInvocation" => push_unique(
                diagnostics,
                seen,
                "PowerShell command argument invokes a .NET member",
            ),
            _ => {}
        }

        if element.element_type == "Parameter"
            && element
                .children
                .iter()
                .any(|child| child.element_type != "StringConstant")
        {
            push_unique(
                diagnostics,
                seen,
                "PowerShell colon-bound parameter contains an expression that cannot be statically validated",
            );
        }
    }
}

fn script_blocks_are_safe(statement: &ParsedStatement) -> bool {
    let mut saw_script_block_owner = false;
    for command in &statement.commands {
        let owns_script_block = command
            .elements
            .iter()
            .skip(1)
            .any(|element| element.element_type == "ScriptBlock");
        if owns_script_block {
            saw_script_block_owner = true;
            if !command_name(command)
                .as_deref()
                .is_some_and(is_safe_script_block_consumer)
            {
                return false;
            }
        }
    }
    saw_script_block_owner
}

fn command_name(command: &ParsedCommand) -> Option<String> {
    command
        .elements
        .first()
        .and_then(raw_command_name)
        .map(|name| {
            strip_powershell_module_prefix(&name)
                .to_ascii_lowercase()
                .to_string()
        })
}

fn raw_command_name(element: &ParsedElement) -> Option<String> {
    let raw = if element.raw_type == "StringConstantExpressionAst" {
        element.value.as_deref().unwrap_or(&element.text)
    } else {
        &element.text
    };
    let trimmed = raw.trim().trim_matches(|ch| ch == '\'' || ch == '"');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn application_command_name_rejection(raw_name: &str) -> Option<&'static str> {
    if !raw_name.is_ascii() {
        return Some(
            "PowerShell command name contains non-ASCII characters and cannot be matched safely",
        );
    }

    let lower = raw_name.to_ascii_lowercase();
    if lower == "where.exe" || is_trusted_module_qualified_cmdlet(raw_name) {
        return None;
    }

    if ends_with_script_extension(&lower) {
        return Some("PowerShell application-style command names can execute local scripts");
    }

    if is_path_like_command_name(raw_name) {
        return Some(
            "PowerShell path-like command names can execute local files outside cmdlet validation",
        );
    }

    None
}

fn is_path_like_command_name(name: &str) -> bool {
    name.starts_with(".\\")
        || name.starts_with("./")
        || name.starts_with("..\\")
        || name.starts_with("../")
        || name.starts_with('\\')
        || name.starts_with('/')
        || name.get(1..2) == Some(":")
        || name.contains('/')
        || name.contains('\\')
}

fn ends_with_script_extension(lower: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        ".ps1", ".psm1", ".psd1", ".bat", ".cmd", ".vbs", ".js", ".jse", ".wsf",
    ];
    EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn is_trusted_module_qualified_cmdlet(name: &str) -> bool {
    let Some((module, command)) = name.rsplit_once('\\') else {
        return false;
    };
    module.contains('.')
        && !module.contains('/')
        && !module.starts_with('.')
        && module.get(1..2).is_none_or(|value| value != ":")
        && is_ascii_cmdlet_name(command)
}

fn is_ascii_cmdlet_name(name: &str) -> bool {
    let Some((verb, noun)) = name.split_once('-') else {
        return false;
    };
    !verb.is_empty()
        && !noun.is_empty()
        && verb.chars().all(|ch| ch.is_ascii_alphabetic())
        && noun
            .chars()
            .enumerate()
            .all(|(idx, ch)| ch.is_ascii_alphanumeric() || (idx > 0 && ch == '_'))
        && noun
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
}

fn strip_powershell_module_prefix(name: &str) -> &str {
    if name.starts_with(".\\")
        || name.starts_with("..\\")
        || name.starts_with("\\\\")
        || name.get(1..2) == Some(":")
    {
        return name;
    }

    name.rsplit_once('\\')
        .map(|(_, stripped)| stripped)
        .unwrap_or(name)
}

fn is_safe_script_block_consumer(name: &str) -> bool {
    matches!(
        name,
        "where-object"
            | "where"
            | "?"
            | "sort-object"
            | "sort"
            | "select-object"
            | "select"
            | "group-object"
            | "group"
            | "format-table"
            | "ft"
            | "format-list"
            | "fl"
            | "format-wide"
            | "fw"
            | "format-custom"
            | "fc"
    )
}

fn push_unique(diagnostics: &mut Vec<String>, seen: &mut HashSet<String>, message: &str) {
    if seen.insert(message.to_string()) {
        diagnostics.push(message.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_application_name_rejection_keeps_safe_exceptions() {
        assert!(application_command_name_rejection("scripts\\Get-Process").is_some());
        assert!(application_command_name_rejection(".\\payload.ps1").is_some());
        assert!(application_command_name_rejection("C:\\tmp\\payload.exe").is_some());
        assert!(application_command_name_rejection("where.exe").is_none());
        assert!(application_command_name_rejection(
            "Microsoft.PowerShell.Management\\Get-ChildItem"
        )
        .is_none());
    }

    #[test]
    fn test_dynamic_name_detection_from_metadata() {
        let command = ParsedCommand {
            elements: vec![ParsedElement {
                raw_type: "VariableExpressionAst".to_string(),
                text: "$cmd".to_string(),
                value: None,
                element_type: "Variable".to_string(),
                children: Vec::new(),
            }],
        };
        let mut diagnostics = Vec::new();
        let mut seen = HashSet::new();
        validate_command(&command, &mut diagnostics, &mut seen);
        assert!(diagnostics
            .iter()
            .any(|message| message.contains("dynamic expression")));
    }

    #[test]
    fn test_colon_bound_child_detection_from_metadata() {
        let command = ParsedCommand {
            elements: vec![
                ParsedElement {
                    raw_type: "StringConstantExpressionAst".to_string(),
                    text: "Get-Process".to_string(),
                    value: Some("Get-Process".to_string()),
                    element_type: "StringConstant".to_string(),
                    children: Vec::new(),
                },
                ParsedElement {
                    raw_type: "CommandParameterAst".to_string(),
                    text: "-Name:($env:PROCESSOR_ARCHITECTURE)".to_string(),
                    value: None,
                    element_type: "Parameter".to_string(),
                    children: vec![ParsedChild {
                        element_type: "SubExpression".to_string(),
                    }],
                },
            ],
        };
        let mut diagnostics = Vec::new();
        let mut seen = HashSet::new();
        validate_command(&command, &mut diagnostics, &mut seen);
        assert!(diagnostics
            .iter()
            .any(|message| message.contains("colon-bound parameter")));
    }
}
