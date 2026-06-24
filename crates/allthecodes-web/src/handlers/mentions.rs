//! @Mention autocomplete handler.
//!
//! Searches across sessions, workspace files, and skills to provide mention
//! chip suggestions for the rich editor.

use std::collections::VecDeque;
use std::path::PathBuf;

use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;

use allthecodes_protocol::v1::mentions::{
    MentionAutocompleteQuery, MentionAutocompleteResponse, MentionSuggestion,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_skills::get_all_skills;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MentionAutocompleteProcessor {
    state: WebState,
}

impl From<WebState> for MentionAutocompleteProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for MentionAutocompleteProcessor {
    type Request = MentionAutocompleteQuery;
    type Response = MentionAutocompleteResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "mentions.autocomplete"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, query: Self::Request) -> Result<Self::Response, Self::Error> {
        let limit = query.limit.unwrap_or(10).clamp(1, 50) as usize;
        let q = query.q.to_lowercase();
        let mut suggestions: Vec<MentionSuggestion> = Vec::new();

        if kind_matches(query.kind.as_deref(), "session") {
            push_session_suggestions(&q, limit, &mut suggestions);
        }
        if kind_matches(query.kind.as_deref(), "file") && suggestions.len() < limit {
            push_file_suggestions(&self.state, &q, limit, &mut suggestions);
        }
        if kind_matches(query.kind.as_deref(), "skill") && suggestions.len() < limit {
            push_skill_suggestions(&q, limit, &mut suggestions);
        }

        Ok(MentionAutocompleteResponse { suggestions })
    }
}

fn kind_matches(filter: Option<&str>, candidate: &str) -> bool {
    filter.is_none_or(|kind| kind == candidate)
}

fn matches_query(q: &str, values: &[&str]) -> bool {
    q.is_empty() || values.iter().any(|value| value.to_lowercase().contains(q))
}

fn push_session_suggestions(q: &str, limit: usize, suggestions: &mut Vec<MentionSuggestion>) {
    let Ok(sessions) = allthecodes_session::storage::list_sessions() else {
        return;
    };
    for session in sessions {
        if !matches_query(q, &[&session.title, &session.session_id]) {
            continue;
        }
        let name = if session.title.trim().is_empty() {
            session.session_id.clone()
        } else {
            session.title.clone()
        };
        suggestions.push(MentionSuggestion {
            kind: "session".into(),
            name,
            target: session.session_id,
            description: Some(format!("{} messages", session.message_count)),
        });
        if suggestions.len() >= limit {
            break;
        }
    }
}

fn push_file_suggestions(
    state: &WebState,
    q: &str,
    limit: usize,
    suggestions: &mut Vec<MentionSuggestion>,
) {
    let root = PathBuf::from(state.engine().cwd());
    let Ok(root) = root.canonicalize() else {
        return;
    };
    if !root.is_dir() {
        return;
    }

    let mut queue = VecDeque::from([root.clone()]);
    let mut visited = 0usize;
    while let Some(dir) = queue.pop_front() {
        visited += 1;
        if visited > 1_000 || suggestions.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if suggestions.len() >= limit {
                break;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if should_skip_path(name) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                queue.push_back(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Ok(relative) = path.strip_prefix(&root) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            let target = relative.replace('\\', "/");
            if !matches_query(q, &[name, &target]) {
                continue;
            }
            let description = entry
                .metadata()
                .ok()
                .map(|meta| format!("{} bytes", meta.len()));
            suggestions.push(MentionSuggestion {
                kind: "file".into(),
                name: target.clone(),
                target,
                description,
            });
        }
    }
}

fn should_skip_path(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "target" | "dist" | "build" | ".next" | "coverage"
        )
}

fn push_skill_suggestions(q: &str, limit: usize, suggestions: &mut Vec<MentionSuggestion>) {
    for skill in get_all_skills() {
        let display_name = skill.display_name().to_string();
        let description = skill.frontmatter.description.clone();
        let when_to_use = skill.frontmatter.when_to_use.clone().unwrap_or_default();
        if !matches_query(q, &[&skill.name, &display_name, &description, &when_to_use]) {
            continue;
        }
        suggestions.push(MentionSuggestion {
            kind: "skill".into(),
            name: display_name,
            target: skill.name,
            description: if description.is_empty() {
                None
            } else {
                Some(description)
            },
        });
        if suggestions.len() >= limit {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge
// ---------------------------------------------------------------------------

async fn autocomplete_handler(
    State(state): State<WebState>,
    Query(query): Query<MentionAutocompleteQuery>,
) -> Response {
    rest_processor_response::<MentionAutocompleteProcessor>(
        state,
        ApiMethod::MentionAutocomplete,
        query,
    )
    .await
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(ApiMethod::MentionAutocomplete, get(autocomplete_handler))
}
