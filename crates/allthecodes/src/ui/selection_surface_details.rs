use super::{SelectionAction, SelectionItem};

pub(super) fn push_item_details(
    lines: &mut Vec<String>,
    item: &SelectionItem,
    rendered_rows: &mut usize,
    height: usize,
) {
    for detail in item.preview_lines.iter().chain(action_lines(item).iter()) {
        if *rendered_rows >= height {
            break;
        }
        lines.push(format!("  {detail}"));
        *rendered_rows += 1;
    }
}

pub(super) fn item_search_terms(item: &SelectionItem) -> Vec<&str> {
    let mut terms = vec![
        item.label.as_str(),
        item.id.as_str(),
        item.description.as_str(),
    ];

    if let Some(reason) = &item.disabled_reason {
        terms.push(reason.as_str());
    }

    terms.extend(item.preview_lines.iter().map(String::as_str));
    terms.extend(item.actions.iter().flat_map(action_search_terms));
    terms.extend(item.search_terms.iter().map(String::as_str));
    terms
}

fn action_lines(item: &SelectionItem) -> Vec<String> {
    item.actions.iter().map(render_action_line).collect()
}

fn render_action_line(action: &SelectionAction) -> String {
    let state = if action.enabled {
        String::new()
    } else {
        action
            .disabled_reason
            .as_deref()
            .map(|reason| format!(" disabled: {reason}"))
            .unwrap_or_else(|| " disabled".to_string())
    };
    format!("action {}: {}{state}", action.id, action.label)
}

fn action_search_terms(action: &SelectionAction) -> Vec<&str> {
    let mut terms = vec![action.id.as_str(), action.label.as_str()];
    if let Some(reason) = &action.disabled_reason {
        terms.push(reason.as_str());
    }
    terms
}
