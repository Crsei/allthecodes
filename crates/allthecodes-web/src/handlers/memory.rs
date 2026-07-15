//! Runtime-owned Memory, dream, and background-review API adapters.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, NaiveDate};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use allthecodes_engine::services::background_review::{
    self, BackgroundReviewDecisionError, BackgroundReviewDisposition, BackgroundReviewProposal,
    BackgroundReviewProposalKind,
};
use allthecodes_protocol::v1::memory::{
    MemoryDreamDetailResponse, MemoryDreamListQuery, MemoryDreamListResponse, MemoryDreamSummary,
    MemoryEntry as ApiMemoryEntry, MemoryListQuery, MemoryListResponse, MemoryMigrationReport,
    MemoryProposal, MemoryProposalDecisionResponse,
    MemoryProposalDetailResponse, MemoryProposalDisposition, MemoryProposalKind,
    MemoryProposalListQuery, MemoryProposalListResponse, MemoryScope as ApiMemoryScope,
    MemoryType as ApiMemoryType, MemoryUpdateRequest, MemoryUpdateResponse,
};
use allthecodes_session::memdir::{
    self, MemoryEntry, MemoryEntryUpdate, MemoryImportOutcome, MemoryScope, MemoryType,
};

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 100;
const MAX_CONTENT_BYTES: usize = 100_000;
const MAX_QUERY_CHARS: usize = 200;
const MAX_CATEGORY_CHARS: usize = 100;
const MAX_DESCRIPTION_CHARS: usize = 1_000;
const MAX_MEMORY_KEY_CHARS: usize = 200;
const MAX_TAG_COUNT: usize = 50;
const MAX_TAG_CHARS: usize = 100;
const MAX_SEARCH_TERM_COUNT: usize = 50;
const MAX_SEARCH_TERM_CHARS: usize = 200;
const DREAM_PREVIEW_BYTES: usize = 512;
const DREAM_DETAIL_MAX_BYTES: usize = 64 * 1024;
const PROPOSAL_VALUE_MAX_CHARS: usize = 10_000;

#[derive(Debug, Clone, Deserialize)]
struct LegacyMemoryStore {
    #[serde(default)]
    entries: Vec<LegacyMemoryEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyMemoryEntry {
    id: String,
    timestamp: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    workspace: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pinned: bool,
    updated_at: String,
}

pub async fn memory_list_handler(
    State(state): State<WebState>,
    Query(query): Query<MemoryListQuery>,
) -> Response {
    if query
        .query
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_QUERY_CHARS)
    {
        return validation_error("query", "query is too long");
    }
    if query
        .category
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_CATEGORY_CHARS)
    {
        return validation_error("category", "category is too long");
    }
    let cwd = PathBuf::from(state.engine().cwd());
    let migration = match migrate_legacy_memory_store(&cwd) {
        Ok(report) => report,
        Err(error) => return internal_error("legacy memory migration failed", error),
    };

    let scopes = query
        .scope
        .map(|scope| vec![from_api_scope(scope)])
        .unwrap_or_else(|| {
            vec![
                MemoryScope::Global,
                MemoryScope::Project,
                MemoryScope::Team,
                MemoryScope::Auto,
            ]
        });
    let mut entries = Vec::new();
    for scope in scopes {
        let listed = match memdir::list_memories(scope, &cwd) {
            Ok(entries) => entries,
            Err(error) => return internal_error("failed to list canonical memories", error),
        };
        entries.extend(listed.into_iter().map(|entry| project_entry(scope, entry)));
    }

    if let Some(memory_type) = query.memory_type {
        entries.retain(|entry| entry.memory_type == Some(memory_type));
    }
    if let Some(category) = query.category.as_deref() {
        let category = category.trim();
        entries.retain(|entry| entry.category == category);
    }
    if let Some(needle) = query.query.as_deref() {
        let needle = needle.trim().to_ascii_lowercase();
        if !needle.is_empty() {
            entries.retain(|entry| {
                entry.key.to_ascii_lowercase().contains(&needle)
                    || entry.value.to_ascii_lowercase().contains(&needle)
                    || entry.category.to_ascii_lowercase().contains(&needle)
                    || entry
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(&needle))
            });
        }
    }
    entries.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let (entries, next_cursor) = match paginate(entries, query.limit, query.cursor.as_deref()) {
        Ok(page) => page,
        Err(response) => return response,
    };
    Json(MemoryListResponse {
        profile_id: query.profile_id,
        entries,
        next_cursor,
        migration,
    })
    .into_response()
}

pub async fn memory_update_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
    Json(request): Json<MemoryUpdateRequest>,
) -> Response {
    let (scope, key) = match decode_memory_id(&id) {
        Some(value) => value,
        None => return validation_error("id", "invalid memory id"),
    };
    if request
        .value
        .as_deref()
        .is_some_and(|value| value.len() > MAX_CONTENT_BYTES)
    {
        return validation_error("value", "memory value exceeds the maximum size");
    }
    if request
        .category
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_CATEGORY_CHARS)
    {
        return validation_error("category", "category is too long");
    }
    if request
        .description
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_DESCRIPTION_CHARS)
    {
        return validation_error("description", "description is too long");
    }
    let tags = match request.tags.map(normalize_tags) {
        Some(Ok(tags)) => Some(tags),
        Some(Err(message)) => return validation_error("tags", message),
        None => None,
    };
    let search_terms = match request.search_terms.map(normalize_search_terms) {
        Some(Ok(terms)) => Some(terms),
        Some(Err(message)) => return validation_error("search_terms", message),
        None => None,
    };
    let cwd = PathBuf::from(state.engine().cwd());
    let update = MemoryEntryUpdate {
        value: request.value,
        category: request.category.map(|value| value.trim().to_string()),
        memory_type: request.memory_type.map(|value| Some(from_api_type(value))),
        description: request
            .description
            .map(|value| Some(value.trim().to_string())),
        search_terms,
        tags,
        pinned: request.pinned,
    };
    match memdir::update_memory(&key, scope, &cwd, update) {
        Ok(entry) => Json(MemoryUpdateResponse {
            entry: project_entry(scope, entry),
        })
        .into_response(),
        Err(error) if error.to_string().contains("not found") => not_found("memory", &id),
        Err(error) => internal_error("failed to update canonical memory", error),
    }
}

pub async fn memory_dream_list_handler(Query(query): Query<MemoryDreamListQuery>) -> Response {
    let entries = match allthecodes_services::dream::list_dream_memories(DREAM_PREVIEW_BYTES) {
        Ok(entries) => entries
            .into_iter()
            .map(|entry| MemoryDreamSummary {
                date: entry.date.format("%Y-%m-%d").to_string(),
                byte_size: entry.byte_size,
                preview: entry.preview,
            })
            .collect::<Vec<_>>(),
        Err(error) => return internal_error("failed to list dream memories", error),
    };
    let (items, next_cursor) = match paginate(entries, query.limit, query.cursor.as_deref()) {
        Ok(page) => page,
        Err(response) => return response,
    };
    Json(MemoryDreamListResponse { items, next_cursor }).into_response()
}

pub async fn memory_dream_detail_handler(AxumPath(date): AxumPath<String>) -> Response {
    let parsed = match parse_strict_date(&date) {
        Some(date) => date,
        None => return validation_error("date", "date must use YYYY-MM-DD"),
    };
    match allthecodes_services::dream::read_dream_memory(parsed, DREAM_DETAIL_MAX_BYTES) {
        Ok(Some(document)) => Json(MemoryDreamDetailResponse {
            date,
            byte_size: document.byte_size,
            markdown: document.markdown,
            truncated: document.truncated,
        })
        .into_response(),
        Ok(None) => not_found("dream_memory", &date),
        Err(error) => internal_error("failed to read dream memory", error),
    }
}

pub async fn memory_proposal_list_handler(
    State(state): State<WebState>,
    Query(query): Query<MemoryProposalListQuery>,
) -> Response {
    let cwd = PathBuf::from(state.engine().cwd());
    let mut proposals = match background_review::list_background_review_proposals() {
        Ok(proposals) => proposals
            .into_iter()
            .filter(background_review::is_memory_review_proposal)
            .filter(|proposal| proposal_belongs_to_workspace(proposal, &cwd))
            .filter_map(project_proposal)
            .collect::<Vec<_>>(),
        Err(error) => return internal_error("failed to list memory proposals", error),
    };
    proposals.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let (proposals, next_cursor) = match paginate(proposals, query.limit, query.cursor.as_deref()) {
        Ok(page) => page,
        Err(response) => return response,
    };
    Json(MemoryProposalListResponse {
        proposals,
        next_cursor,
        automatic_producer: "workflow_warning".to_string(),
    })
    .into_response()
}

pub async fn memory_proposal_detail_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    let cwd = PathBuf::from(state.engine().cwd());
    match background_review::load_background_review_proposal(&id) {
        Ok(proposal)
            if background_review::is_memory_review_proposal(&proposal)
                && proposal_belongs_to_workspace(&proposal, &cwd) =>
        {
            match project_proposal(proposal) {
                Some(proposal) => Json(MemoryProposalDetailResponse { proposal }).into_response(),
                None => not_found("memory_proposal", &id),
            }
        }
        _ => not_found("memory_proposal", &id),
    }
}

pub async fn memory_proposal_approve_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    decide_memory_proposal(&state, &id, BackgroundReviewDisposition::Approved)
}

pub async fn memory_proposal_reject_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    decide_memory_proposal(&state, &id, BackgroundReviewDisposition::Rejected)
}

fn decide_memory_proposal(
    state: &WebState,
    id: &str,
    disposition: BackgroundReviewDisposition,
) -> Response {
    let cwd = PathBuf::from(state.engine().cwd());
    match background_review::load_background_review_proposal(id) {
        Ok(proposal)
            if background_review::is_memory_review_proposal(&proposal)
                && proposal_belongs_to_workspace(&proposal, &cwd) =>
        {
            // The exclusive decision service reloads the proposal after this
            // workspace/domain authorization preflight.
        }
        _ => return not_found("memory_proposal", id),
    }
    match background_review::decide_memory_background_review_proposal(id, &cwd, disposition) {
        Ok(outcome) => Json(MemoryProposalDecisionResponse {
            proposal_id: outcome.proposal.id,
            disposition: match outcome.receipt.disposition {
                BackgroundReviewDisposition::Approved => MemoryProposalDisposition::Approved,
                BackgroundReviewDisposition::Rejected => MemoryProposalDisposition::Rejected,
            },
            decided_at: outcome.receipt.decided_at.to_rfc3339(),
            entry: outcome.memory.map(|entry| {
                let scope = entry
                    .effective_memory_type()
                    .map(memory_scope_for_type)
                    .unwrap_or(MemoryScope::Project);
                project_entry(scope, entry)
            }),
        })
        .into_response(),
        Err(BackgroundReviewDecisionError::AlreadyClaimed) => conflict(
            "proposal_in_progress",
            "memory proposal is already being decided",
        ),
        Err(BackgroundReviewDecisionError::AlreadyDecided(receipt)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "proposal_already_decided",
                "error": "memory proposal was already decided",
                "details": {
                    "proposal_id": receipt.proposal_id,
                    "disposition": receipt.disposition,
                }
            })),
        )
            .into_response(),
        Err(
            BackgroundReviewDecisionError::InvalidId
            | BackgroundReviewDecisionError::NotFound
            | BackgroundReviewDecisionError::WrongDomain,
        ) => not_found("memory_proposal", id),
        Err(BackgroundReviewDecisionError::InvalidPayload) => validation_error(
            "proposal",
            "memory proposal does not contain a valid bounded memory payload",
        ),
        Err(BackgroundReviewDecisionError::Io(error)) => {
            internal_error("failed to decide memory proposal", error)
        }
    }
}

fn project_entry(scope: MemoryScope, entry: MemoryEntry) -> ApiMemoryEntry {
    let memory_type = entry.effective_memory_type().map(to_api_type);
    let api_scope = to_api_scope(scope);
    let (value, value_truncated) = truncate_utf8_bytes(&entry.value, MAX_CONTENT_BYTES);
    ApiMemoryEntry {
        id: encode_memory_id(scope, &entry.key),
        scope: api_scope,
        key: entry.key,
        value: value.clone(),
        value_truncated,
        category: entry.category,
        memory_type,
        description: entry.description,
        search_terms: entry.search_terms,
        source_session_id: entry.source_session_id.clone(),
        approval_id: entry.approval_id,
        tags: entry.tags,
        pinned: entry.pinned,
        created_at: entry.created_at.clone(),
        updated_at: entry.updated_at,
        content: value,
        timestamp: entry.created_at,
        session_id: entry.source_session_id,
        workspace: scope.as_str().to_string(),
    }
}

fn project_proposal(proposal: BackgroundReviewProposal) -> Option<MemoryProposal> {
    let memory = proposal.payload.get("memory").unwrap_or(&proposal.payload);
    let kind = match proposal.kind {
        BackgroundReviewProposalKind::MemoryAdd => MemoryProposalKind::MemoryAdd,
        BackgroundReviewProposalKind::MemoryReplace => MemoryProposalKind::MemoryReplace,
        BackgroundReviewProposalKind::WorkflowWarning => MemoryProposalKind::WorkflowWarning,
        BackgroundReviewProposalKind::SkillCreate | BackgroundReviewProposalKind::SkillPatch => {
            return None;
        }
    };
    let writes_memory = matches!(
        proposal.kind,
        BackgroundReviewProposalKind::MemoryAdd | BackgroundReviewProposalKind::MemoryReplace
    );
    Some(MemoryProposal {
        id: proposal.id,
        kind,
        summary: truncate_chars(&proposal.summary, 500),
        source_session_id: truncate_chars(&proposal.source_session_id, 200),
        created_at: proposal.created_at.to_rfc3339(),
        target: memory
            .get("target")
            .and_then(|value| value.as_str())
            .map(|value| truncate_chars(value, 50)),
        key: memory
            .get("key")
            .and_then(|value| value.as_str())
            .map(|value| truncate_chars(value, 200)),
        value: memory
            .get("value")
            .and_then(|value| value.as_str())
            .map(|value| truncate_chars(value, PROPOSAL_VALUE_MAX_CHARS)),
        writes_memory_on_approval: writes_memory,
    })
}

fn proposal_belongs_to_workspace(proposal: &BackgroundReviewProposal, cwd: &Path) -> bool {
    let expected = cwd.to_string_lossy();
    match proposal.payload.get("cwd").and_then(|value| value.as_str()) {
        Some(proposal_cwd) => proposal_cwd == expected,
        None => {
            let memory = proposal.payload.get("memory").unwrap_or(&proposal.payload);
            memory.get("target").and_then(|value| value.as_str()) == Some("user")
        }
    }
}

fn migrate_legacy_memory_store(cwd: &Path) -> anyhow::Result<Option<MemoryMigrationReport>> {
    let path = legacy_entries_path();
    if !path.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let store: LegacyMemoryStore = serde_json::from_str(&content)?;
    let mut canonical = BTreeMap::new();
    let mut conflicts = Vec::new();

    // Validate and materialize the entire batch before the first canonical
    // write, preventing malformed rows from producing a partial migration.
    for legacy in store.entries {
        if legacy.id.trim().is_empty()
            || legacy.content.len() > MAX_CONTENT_BYTES
            || DateTime::parse_from_rfc3339(&legacy.timestamp).is_err()
            || DateTime::parse_from_rfc3339(&legacy.updated_at).is_err()
        {
            conflicts.push(legacy.id);
            continue;
        }
        let tags = match normalize_tags(legacy.tags) {
            Ok(tags) => tags,
            Err(_) => {
                conflicts.push(legacy.id);
                continue;
            }
        };
        let key = legacy_key(&legacy.id);
        let entry = MemoryEntry {
            key: key.clone(),
            value: legacy.content,
            category: "legacy".to_string(),
            memory_type: None,
            description: (!legacy.workspace.trim().is_empty())
                .then(|| "Migrated from the legacy Web memory store".to_string()),
            search_terms: Vec::new(),
            tags,
            pinned: legacy.pinned,
            source_session_id: (!legacy.session_id.trim().is_empty()).then_some(legacy.session_id),
            approval_id: None,
            created_at: legacy.timestamp,
            updated_at: legacy.updated_at,
        };
        if canonical.insert(key.clone(), entry).is_some() {
            conflicts.push(key);
        }
    }
    if !conflicts.is_empty() {
        return Ok(Some(MemoryMigrationReport {
            imported: 0,
            already_present: 0,
            conflicts,
            completed: false,
        }));
    }

    let mut report = MemoryMigrationReport::default();
    for entry in canonical.values() {
        match memdir::classify_memory_import(entry, MemoryScope::Global, cwd)? {
            MemoryImportOutcome::Imported => {}
            MemoryImportOutcome::AlreadyPresent => report.already_present += 1,
            MemoryImportOutcome::Conflict => report.conflicts.push(entry.key.clone()),
        }
    }
    if !report.conflicts.is_empty() {
        return Ok(Some(report));
    }

    for entry in canonical.values() {
        if memdir::classify_memory_import(entry, MemoryScope::Global, cwd)?
            == MemoryImportOutcome::AlreadyPresent
        {
            continue;
        }
        match memdir::import_memory_entry(entry, MemoryScope::Global, cwd)? {
            MemoryImportOutcome::Imported => report.imported += 1,
            MemoryImportOutcome::AlreadyPresent => {}
            MemoryImportOutcome::Conflict => report.conflicts.push(entry.key.clone()),
        }
    }
    report.completed = report.conflicts.is_empty();
    if report.completed {
        let backup = path.with_file_name("entries.legacy-backup.json");
        if !backup.exists() {
            std::fs::rename(&path, &backup)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(Some(report))
}

fn legacy_entries_path() -> PathBuf {
    allthecodes_config::paths::data_root()
        .join("memory")
        .join("entries.json")
}

fn legacy_key(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    format!("legacy-{}", URL_SAFE_NO_PAD.encode(&digest[..15]))
}

fn encode_memory_id(scope: MemoryScope, key: &str) -> String {
    format!(
        "{}:{}",
        scope.as_str(),
        URL_SAFE_NO_PAD.encode(key.as_bytes())
    )
}

fn decode_memory_id(id: &str) -> Option<(MemoryScope, String)> {
    if id.len() > 400 {
        return None;
    }
    let (scope, key) = id.split_once(':')?;
    let scope = match scope {
        "global" => MemoryScope::Global,
        "project" => MemoryScope::Project,
        "team" => MemoryScope::Team,
        "auto" => MemoryScope::Auto,
        _ => return None,
    };
    let key = String::from_utf8(URL_SAFE_NO_PAD.decode(key).ok()?).ok()?;
    (!key.is_empty()
        && key.chars().count() <= MAX_MEMORY_KEY_CHARS
        && !key.chars().any(char::is_control))
    .then_some((scope, key))
}

fn paginate<T>(
    items: Vec<T>,
    requested_limit: Option<usize>,
    cursor: Option<&str>,
) -> Result<(Vec<T>, Option<String>), Response> {
    let limit = requested_limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(validation_error("limit", "limit must be between 1 and 100"));
    }
    let offset = match cursor {
        Some(cursor) => decode_cursor(cursor)
            .ok_or_else(|| validation_error("cursor", "invalid pagination cursor"))?,
        None => 0,
    };
    if offset > items.len() {
        return Err(validation_error("cursor", "pagination cursor is stale"));
    }
    let end = offset.saturating_add(limit).min(items.len());
    let next_cursor = (end < items.len()).then(|| encode_cursor(end));
    Ok((
        items.into_iter().skip(offset).take(limit).collect(),
        next_cursor,
    ))
}

fn encode_cursor(offset: usize) -> String {
    URL_SAFE_NO_PAD.encode(format!("offset:{offset}"))
}

fn decode_cursor(cursor: &str) -> Option<usize> {
    let decoded = String::from_utf8(URL_SAFE_NO_PAD.decode(cursor).ok()?).ok()?;
    decoded.strip_prefix("offset:")?.parse().ok()
}

fn normalize_tags(raw: Vec<String>) -> Result<Vec<String>, &'static str> {
    if raw.len() > MAX_TAG_COUNT {
        return Err("too many tags");
    }
    normalize_bounded_strings(raw, MAX_TAG_COUNT, MAX_TAG_CHARS).ok_or("invalid tag")
}

fn normalize_search_terms(raw: Vec<String>) -> Result<Vec<String>, &'static str> {
    if raw.len() > MAX_SEARCH_TERM_COUNT {
        return Err("too many search terms");
    }
    normalize_bounded_strings(raw, MAX_SEARCH_TERM_COUNT, MAX_SEARCH_TERM_CHARS)
        .ok_or("invalid search term")
}

fn normalize_bounded_strings(
    raw: Vec<String>,
    limit: usize,
    max_chars: usize,
) -> Option<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for value in raw {
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        if value.chars().count() > max_chars {
            return None;
        }
        if seen.insert(value.clone()) {
            values.push(value);
        }
    }
    (values.len() <= limit).then_some(values)
}

fn parse_strict_date(value: &str) -> Option<NaiveDate> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    (date.format("%Y-%m-%d").to_string() == value).then_some(date)
}

fn to_api_scope(scope: MemoryScope) -> ApiMemoryScope {
    match scope {
        MemoryScope::Global => ApiMemoryScope::Global,
        MemoryScope::Project => ApiMemoryScope::Project,
        MemoryScope::Team => ApiMemoryScope::Team,
        MemoryScope::Auto => ApiMemoryScope::Auto,
    }
}

fn from_api_scope(scope: ApiMemoryScope) -> MemoryScope {
    match scope {
        ApiMemoryScope::Global => MemoryScope::Global,
        ApiMemoryScope::Project => MemoryScope::Project,
        ApiMemoryScope::Team => MemoryScope::Team,
        ApiMemoryScope::Auto => MemoryScope::Auto,
    }
}

fn to_api_type(memory_type: MemoryType) -> ApiMemoryType {
    match memory_type {
        MemoryType::User => ApiMemoryType::User,
        MemoryType::Feedback => ApiMemoryType::Feedback,
        MemoryType::Project => ApiMemoryType::Project,
        MemoryType::Reference => ApiMemoryType::Reference,
    }
}

fn from_api_type(memory_type: ApiMemoryType) -> MemoryType {
    match memory_type {
        ApiMemoryType::User => MemoryType::User,
        ApiMemoryType::Feedback => MemoryType::Feedback,
        ApiMemoryType::Project => MemoryType::Project,
        ApiMemoryType::Reference => MemoryType::Reference,
    }
}

fn memory_scope_for_type(memory_type: MemoryType) -> MemoryScope {
    match memory_type {
        MemoryType::User => MemoryScope::Global,
        MemoryType::Feedback | MemoryType::Project | MemoryType::Reference => MemoryScope::Project,
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

fn validation_error(field: &str, message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(
            ProtocolApiError::Validation {
                field: field.to_string(),
                message: message.into(),
            }
            .into_body(),
        ),
    )
        .into_response()
}

fn not_found(entity: &'static str, id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(
            ProtocolApiError::NotFound {
                entity,
                id: id.to_string(),
            }
            .into_body(),
        ),
    )
        .into_response()
}

fn conflict(code: &'static str, message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({"code": code, "error": message})),
    )
        .into_response()
}

fn internal_error(message: &str, error: impl std::fmt::Display) -> Response {
    tracing::warn!(error = %error, "{message}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(
            ProtocolApiError::Internal {
                message: message.to_string(),
            }
            .into_body(),
        ),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_memory_id_round_trips_scope_and_unicode_key() {
        let id = encode_memory_id(MemoryScope::Project, "release/计划");
        assert_eq!(
            decode_memory_id(&id),
            Some((MemoryScope::Project, "release/计划".to_string()))
        );
        assert!(decode_memory_id("project:../escape").is_none());
    }

    #[test]
    fn strict_dream_date_rejects_traversal_and_noncanonical_dates() {
        assert!(parse_strict_date("2026-07-16").is_some());
        assert!(parse_strict_date("2026-7-16").is_none());
        assert!(parse_strict_date("../../2026-07-16").is_none());
    }

    #[test]
    fn cursors_are_opaque_and_reject_modified_values() {
        let cursor = encode_cursor(42);
        assert_eq!(decode_cursor(&cursor), Some(42));
        assert_eq!(decode_cursor("42"), None);
    }

    #[test]
    fn proposal_projection_never_contains_raw_workflow_evidence() {
        let proposal = BackgroundReviewProposal {
            id: "proposal-1".to_string(),
            source_session_id: "session-1".to_string(),
            kind: BackgroundReviewProposalKind::WorkflowWarning,
            summary: "summary".to_string(),
            payload: json!({
                "cwd": "/private/project",
                "tool_errors": ["secret error"],
                "similar_session_hits": ["private session"]
            }),
            created_at: chrono::Utc::now(),
        };
        let value = serde_json::to_value(project_proposal(proposal).unwrap()).unwrap();
        assert!(value.get("cwd").is_none());
        assert!(value.get("tool_errors").is_none());
        assert!(value.get("similar_session_hits").is_none());
    }
}

#[cfg(test)]
#[path = "memory_api_tests.rs"]
mod api_tests;
