use super::*;

static TODO_STORE: std::sync::LazyLock<Mutex<HashMap<String, Vec<TodoItem>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn todo_owner_key(session_id: &str, agent_id: Option<&str>) -> String {
    agent_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| session_id.to_string())
}

pub fn parse_todo_items(input: &Value) -> std::result::Result<Vec<TodoItem>, String> {
    let Some(todos_value) = input.get("todos") else {
        return Err("todos is required".to_string());
    };
    let mut todos: Vec<TodoItem> = serde_json::from_value(todos_value.clone())
        .map_err(|err| format!("invalid todos array: {err}"))?;

    for (index, todo) in todos.iter_mut().enumerate() {
        todo.content = todo.content.trim().to_string();
        if todo.content.is_empty() {
            return Err(format!("todos[{index}].content is required"));
        }
        if !matches!(
            todo.status.as_str(),
            "pending" | "in_progress" | "completed"
        ) {
            return Err(format!("todos[{index}].status is invalid"));
        }
        todo.active_form = todo.active_form.as_ref().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
    }

    Ok(todos)
}

pub fn replace_todos_for_key(key: &str, todos: Vec<TodoItem>) -> TodoWriteOutcome {
    let all_done = todos.iter().all(|todo| todo.status == "completed");
    let verification_nudge_needed = all_done
        && todos.len() >= 3
        && !todos
            .iter()
            .any(|todo| todo.content.to_ascii_lowercase().contains("verif"));
    let stored = if all_done { Vec::new() } else { todos };

    TODO_STORE.lock().insert(key.to_string(), stored.clone());

    TodoWriteOutcome {
        todos: stored,
        cleared: all_done,
        verification_nudge_needed,
    }
}

pub fn todos_for_key(key: &str) -> Vec<TodoItem> {
    TODO_STORE.lock().get(key).cloned().unwrap_or_default()
}
