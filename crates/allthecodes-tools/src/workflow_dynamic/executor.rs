//! DAG validation and WorkflowPlan parsing.
//!
//! Validates that the JSON plan is a well-formed DAG:
//! - All stage IDs are unique
//! - depends_on references only valid stage IDs
//! - No cycles (topological sort succeeds)
//! - Stage kinds are recognized (agent/map/reduce)

use std::collections::{HashMap, VecDeque};

use anyhow::{bail, Context, Result};
use serde_json::Value;

pub const ALLOWED_SUBAGENT_TYPES: &[&str] = &[
    "general-purpose",
    "Explore",
    "Plan",
    "code-reviewer",
    "worker",
    "statusline-setup",
];

// ---------------------------------------------------------------------------
// Parsed stage
// ---------------------------------------------------------------------------

/// A single stage in the workflow plan, after JSON field extraction.
#[derive(Debug, Clone)]
pub struct WorkflowStage {
    /// Stage identifier, unique within a plan.
    pub id: String,
    /// Stage kind: "agent", "map", or "reduce".
    pub kind: String,
    /// Prompt for the sub-agent.
    pub prompt: String,
    /// Optional subagent type override for this stage.
    pub subagent_type: Option<String>,
    /// IDs of stages this one depends on.
    pub depends_on: Vec<String>,
    /// Items to iterate over (map stage only).
    pub items: Option<Vec<String>>,
    /// Comma-separated stage IDs fed into this reduce.
    pub reduce_input: Option<String>,
}

pub fn validate_subagent_type_name(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        bail!("subagent_type must not be empty");
    }
    if value.chars().any(char::is_control) {
        bail!("subagent_type must not contain control characters");
    }
    if !ALLOWED_SUBAGENT_TYPES.contains(&value) {
        bail!(
            "subagent_type '{}' is not allowed for DynamicWorkflow; allowed values: {}",
            value,
            ALLOWED_SUBAGENT_TYPES.join(", ")
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Parsed plan
// ---------------------------------------------------------------------------

/// A validated workflow plan (DAG).
#[derive(Debug, Clone)]
pub struct WorkflowPlan {
    pub name: String,
    pub max_concurrency: u8,
    pub stages: Vec<WorkflowStage>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Parse and validate the JSON DAG plan.
///
/// Accepts either a direct plan object (with `stages` array), or the
/// model-provided `plan` field.
pub fn validate_workflow_plan(plan_value: &Value) -> Result<WorkflowPlan> {
    let obj = plan_value
        .as_object()
        .context("workflow plan must be a JSON object")?;

    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unnamed")
        .to_string();
    let max_concurrency = match obj.get("max_concurrency") {
        Some(value) => value
            .as_u64()
            .filter(|number| (1..=64).contains(number))
            .map(|number| number as u8)
            .context("workflow plan max_concurrency must be an integer between 1 and 64")?,
        None => 8,
    };

    let stages_arr = obj
        .get("stages")
        .and_then(Value::as_array)
        .context("workflow plan must have a non-empty `stages` array")?;

    if stages_arr.is_empty() {
        bail!("workflow plan must have at least one stage");
    }

    // Extract stage data
    let mut stages: Vec<WorkflowStage> = Vec::with_capacity(stages_arr.len());
    let mut seen_ids: HashMap<String, usize> = HashMap::new();

    for (i, stage_val) in stages_arr.iter().enumerate() {
        let stage = parse_stage(stage_val, i).context(format!("stage[{}]", i))?;

        // Check duplicate IDs
        if let Some(prev_idx) = seen_ids.get(&stage.id) {
            bail!(
                "duplicate stage id '{}' at indices {} and {}",
                stage.id,
                prev_idx,
                i
            );
        }
        seen_ids.insert(stage.id.clone(), i);
        stages.push(stage);
    }

    // Validate depends_on references
    for stage in &stages {
        for dep in &stage.depends_on {
            if !seen_ids.contains_key(dep.as_str()) {
                bail!("stage '{}' depends on unknown stage '{}'", stage.id, dep);
            }
        }
        if let Some(reduce_input) = &stage.reduce_input {
            for input_id in split_stage_refs(reduce_input) {
                if !seen_ids.contains_key(input_id.as_str()) {
                    bail!(
                        "stage '{}' reduce_input references unknown stage '{}'",
                        stage.id,
                        input_id
                    );
                }
                if !stage.depends_on.iter().any(|dep| dep == &input_id) {
                    bail!(
                        "stage '{}' reduce_input '{}' must also be listed in depends_on",
                        stage.id,
                        input_id
                    );
                }
            }
        }
    }

    // Validate cycle-free (topological sort)
    let sorted = topological_sort(&stages, &seen_ids).context("workflow plan contains cycles")?;

    // Build sorted output
    let id_to_idx: HashMap<&str, usize> = seen_ids.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let sorted_stages: Vec<WorkflowStage> = sorted
        .iter()
        .map(|id| {
            let index = *id_to_idx
                .get(id.as_str())
                .with_context(|| format!("sorted stage '{}' was not found", id))?;
            Ok(stages[index].clone())
        })
        .collect::<Result<_>>()?;

    Ok(WorkflowPlan {
        name,
        max_concurrency,
        stages: sorted_stages,
    })
}

/// Parse a single stage from JSON, extracting known fields.
fn parse_stage(value: &Value, index: usize) -> Result<WorkflowStage> {
    let obj = value
        .as_object()
        .with_context(|| format!("stage[{}] must be a JSON object", index))?;

    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .with_context(|| format!("stage[{}] requires non-empty 'id'", index))?;

    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .map(|s| s.to_lowercase())
        .filter(|s| matches!(s.as_str(), "agent" | "map" | "reduce"))
        .with_context(|| format!("stage '{}' has invalid kind; must be agent/map/reduce", id))?;

    let prompt = obj
        .get("prompt")
        .and_then(Value::as_str)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .with_context(|| format!("stage '{}' requires non-empty 'prompt'", id))?;

    let subagent_type = obj
        .get("subagent_type")
        .and_then(Value::as_str)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    if let Some(subagent_type) = &subagent_type {
        validate_subagent_type_name(subagent_type)?;
    }

    let depends_on: Vec<String> = match obj.get("depends_on") {
        Some(Value::Array(arr)) => {
            let mut deps = Vec::with_capacity(arr.len());
            for dep in arr {
                let dep = dep
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .with_context(|| format!("stage '{}' depends_on must contain strings", id))?;
                deps.push(dep.to_string());
            }
            deps
        }
        Some(_) => bail!("stage '{}' depends_on must be an array of strings", id),
        None => Vec::new(),
    };

    let items: Option<Vec<String>> = match obj.get("items") {
        Some(Value::Array(arr)) => {
            let mut items = Vec::with_capacity(arr.len());
            for item in arr {
                let item = item
                    .as_str()
                    .with_context(|| format!("stage '{}' items must contain strings", id))?;
                items.push(item.to_string());
            }
            Some(items)
        }
        Some(_) => bail!("stage '{}' items must be an array of strings", id),
        None => None,
    };
    if kind == "map" && items.as_ref().is_none_or(Vec::is_empty) {
        bail!("map stage '{}' requires non-empty items", id);
    }

    let reduce_input = obj
        .get("reduce_input")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    Ok(WorkflowStage {
        id,
        kind,
        prompt,
        subagent_type,
        depends_on,
        items,
        reduce_input,
    })
}

fn split_stage_refs(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

// ---------------------------------------------------------------------------
// Topological sort (Kahn's algorithm)
// ---------------------------------------------------------------------------

/// Returns stage IDs in topological order, or an error if a cycle is detected.
fn topological_sort(
    stages: &[WorkflowStage],
    id_map: &HashMap<String, usize>,
) -> Result<Vec<String>> {
    let n = stages.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    // Build adjacency list and in-degree counts
    for (i, stage) in stages.iter().enumerate() {
        for dep in &stage.depends_on {
            if let Some(&dep_idx) = id_map.get(dep.as_str()) {
                adj[dep_idx].push(i);
                in_degree[i] += 1;
            }
        }
    }

    // Queue nodes with in-degree 0
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut sorted: Vec<String> = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        sorted.push(stages[idx].id.clone());
        for &next in &adj[idx] {
            in_degree[next] = in_degree[next].saturating_sub(1);
            if in_degree[next] == 0 {
                queue.push_back(next);
            }
        }
    }

    if sorted.len() != n {
        bail!("cycle detected: sorted {} of {} stages", sorted.len(), n);
    }

    Ok(sorted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- basic parsing ----

    #[test]
    fn test_parse_simple_plan() {
        let plan = json!({
            "name": "test",
            "stages": [
                {"id": "s1", "kind": "agent", "prompt": "do it"}
            ]
        });
        let result = validate_workflow_plan(&plan).unwrap();
        assert_eq!(result.name, "test");
        assert_eq!(result.stages.len(), 1);
        assert_eq!(result.stages[0].id, "s1");
        assert_eq!(result.stages[0].kind, "agent");
        assert_eq!(result.stages[0].prompt, "do it");
    }

    // ---- DAG validation ----

    #[test]
    fn test_parse_dag_dependencies() {
        let plan = json!({
            "stages": [
                {"id": "a", "kind": "agent", "prompt": "A", "depends_on": []},
                {"id": "b", "kind": "agent", "prompt": "B", "depends_on": ["a"]},
                {"id": "c", "kind": "reduce", "prompt": "C", "depends_on": ["a", "b"]}
            ]
        });
        let result = validate_workflow_plan(&plan).unwrap();
        assert_eq!(result.stages.len(), 3);
        // Topological order: a before b before c
        let ids: Vec<&str> = result.stages.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.iter().position(|&id| id == "a") < ids.iter().position(|&id| id == "b"));
        assert!(ids.iter().position(|&id| id == "b") < ids.iter().position(|&id| id == "c"));
    }

    // ---- cycle detection ----

    #[test]
    fn test_rejects_cycle() {
        let plan = json!({
            "stages": [
                {"id": "a", "kind": "agent", "prompt": "A", "depends_on": ["b"]},
                {"id": "b", "kind": "agent", "prompt": "B", "depends_on": ["a"]}
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(err.to_string().contains("cycle"));
    }

    // ---- duplicate IDs ----

    #[test]
    fn test_rejects_duplicate_ids() {
        let plan = json!({
            "stages": [
                {"id": "x", "kind": "agent", "prompt": "first"},
                {"id": "x", "kind": "agent", "prompt": "second"}
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    // ---- unknown dependency ----

    #[test]
    fn test_rejects_bad_dependency() {
        let plan = json!({
            "stages": [
                {"id": "a", "kind": "agent", "prompt": "A", "depends_on": ["nonexistent"]}
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    // ---- invalid kind ----

    #[test]
    fn test_rejects_invalid_kind() {
        let plan = json!({
            "stages": [
                {"id": "x", "kind": "invalid", "prompt": "test"}
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(
            format!("{err:#}").contains("invalid kind")
                || format!("{err:#}").contains("invalid kind")
        );
    }

    // ---- empty stages ----

    #[test]
    fn test_rejects_empty_stages() {
        let plan = json!({
            "stages": []
        });
        assert!(validate_workflow_plan(&plan).is_err());
    }

    #[test]
    fn test_rejects_invalid_max_concurrency() {
        let plan = json!({
            "max_concurrency": 128,
            "stages": [
                {"id": "x", "kind": "agent", "prompt": "test"}
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(err.to_string().contains("max_concurrency"));
    }

    // ---- missing prompt ----

    #[test]
    fn test_rejects_missing_prompt() {
        let plan = json!({
            "stages": [
                {"id": "x", "kind": "agent"}
            ]
        });
        assert!(validate_workflow_plan(&plan).is_err());
    }

    // ---- multiple roots ----

    #[test]
    fn test_multiple_root_stages() {
        let plan = json!({
            "stages": [
                {"id": "a", "kind": "agent", "prompt": "A"},
                {"id": "b", "kind": "agent", "prompt": "B"},
                {"id": "c", "kind": "reduce", "prompt": "C", "depends_on": ["a", "b"]}
            ]
        });
        let result = validate_workflow_plan(&plan).unwrap();
        assert_eq!(result.stages.len(), 3);
    }

    // ---- map stage with items ----

    #[test]
    fn test_map_stage_parses_items() {
        let plan = json!({
            "stages": [
                {
                    "id": "m1",
                    "kind": "map",
                    "prompt": "review {item}",
                    "items": ["alpha", "beta"]
                }
            ]
        });
        let result = validate_workflow_plan(&plan).unwrap();
        assert_eq!(result.stages[0].kind, "map");
        assert_eq!(result.stages[0].items.as_ref().unwrap(), &["alpha", "beta"]);
    }

    #[test]
    fn test_rejects_non_string_map_items() {
        let plan = json!({
            "stages": [
                {
                    "id": "m1",
                    "kind": "map",
                    "prompt": "review {item}",
                    "items": ["alpha", 42]
                }
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(format!("{err:#}").contains("items must contain strings"));
    }

    #[test]
    fn test_rejects_disallowed_subagent_type() {
        let plan = json!({
            "stages": [
                {
                    "id": "a",
                    "kind": "agent",
                    "prompt": "A",
                    "subagent_type": "unknown-agent"
                }
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(format!("{err:#}").contains("not allowed"));
    }

    // ---- reduce input referencing ----

    #[test]
    fn test_reduce_stage_with_inputs() {
        let plan = json!({
            "stages": [
                {"id": "a", "kind": "agent", "prompt": "A"},
                {"id": "b", "kind": "agent", "prompt": "B"},
                {
                    "id": "c",
                    "kind": "reduce",
                    "prompt": "C",
                    "depends_on": ["a", "b"],
                    "reduce_input": "a,b"
                }
            ]
        });
        let result = validate_workflow_plan(&plan).unwrap();
        assert_eq!(result.stages.len(), 3);
        assert_eq!(result.stages[2].reduce_input.as_deref(), Some("a,b"));
    }

    #[test]
    fn test_reduce_input_must_be_dependency() {
        let plan = json!({
            "stages": [
                {"id": "a", "kind": "agent", "prompt": "A"},
                {
                    "id": "c",
                    "kind": "reduce",
                    "prompt": "C",
                    "reduce_input": "a"
                }
            ]
        });
        let err = validate_workflow_plan(&plan).unwrap_err();
        assert!(err
            .to_string()
            .contains("must also be listed in depends_on"));
    }

    // ---- topological_sort direct tests ----

    #[test]
    fn test_topological_sort_simple() {
        let stages = vec![
            WorkflowStage {
                id: "a".into(),
                kind: "agent".into(),
                prompt: "A".into(),
                subagent_type: None,
                depends_on: vec![],
                items: None,
                reduce_input: None,
            },
            WorkflowStage {
                id: "b".into(),
                kind: "agent".into(),
                prompt: "B".into(),
                subagent_type: None,
                depends_on: vec!["a".into()],
                items: None,
                reduce_input: None,
            },
        ];
        let id_map: HashMap<String, usize> = [("a".into(), 0usize), ("b".into(), 1)]
            .iter()
            .cloned()
            .collect();
        let sorted = topological_sort(&stages, &id_map).unwrap();
        assert_eq!(sorted, vec!["a", "b"]);
    }

    #[test]
    fn test_topological_sort_cycle_detected() {
        let stages = vec![
            WorkflowStage {
                id: "a".into(),
                kind: "agent".into(),
                prompt: "A".into(),
                subagent_type: None,
                depends_on: vec!["b".into()],
                items: None,
                reduce_input: None,
            },
            WorkflowStage {
                id: "b".into(),
                kind: "agent".into(),
                prompt: "B".into(),
                subagent_type: None,
                depends_on: vec!["a".into()],
                items: None,
                reduce_input: None,
            },
        ];
        let id_map: HashMap<String, usize> = [("a".into(), 0usize), ("b".into(), 1)]
            .iter()
            .cloned()
            .collect();
        assert!(topological_sort(&stages, &id_map).is_err());
    }
}
