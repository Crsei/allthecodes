use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;
use uuid::Uuid;

const LOCAL_OWNER: &str = "local";

const MIGRATIONS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS ui_preferences (
        owner_profile_id TEXT PRIMARY KEY NOT NULL,
        payload TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS custom_theme_schemes (
        id TEXT PRIMARY KEY NOT NULL,
        owner_profile_id TEXT NOT NULL,
        name TEXT NOT NULL,
        mode TEXT NOT NULL,
        palette_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_custom_theme_schemes_owner
        ON custom_theme_schemes(owner_profile_id, updated_at)
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS saved_prompts (
        id TEXT PRIMARY KEY NOT NULL,
        owner_profile_id TEXT NOT NULL,
        title TEXT NOT NULL,
        content TEXT NOT NULL,
        category TEXT NOT NULL,
        tags_json TEXT NOT NULL,
        is_favorite INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_saved_prompts_owner
        ON saved_prompts(owner_profile_id, updated_at)
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS workspace_layouts (
        id TEXT PRIMARY KEY NOT NULL,
        owner_profile_id TEXT NOT NULL,
        name TEXT NOT NULL,
        layout_mode TEXT NOT NULL,
        slots_json TEXT NOT NULL,
        is_default INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_workspace_layouts_owner
        ON workspace_layouts(owner_profile_id, updated_at)
    "#,
];

#[derive(Clone)]
pub struct WebUiStore {
    db_path: PathBuf,
    pool: SqlitePool,
    ready: Arc<OnceCell<()>>,
}

impl WebUiStore {
    pub fn new(db_path: PathBuf) -> Self {
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_lazy_with(options);
        Self {
            db_path,
            pool,
            ready: Arc::new(OnceCell::new()),
        }
    }

    pub async fn health(&self) -> Result<()> {
        self.ensure_ready().await?;
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("failed to query web UI state database")?;
        Ok(())
    }

    pub async fn get_preferences(&self, owner_profile_id: &str) -> Result<Value> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let row = sqlx::query("SELECT payload FROM ui_preferences WHERE owner_profile_id = ?")
            .bind(&owner)
            .fetch_optional(&self.pool)
            .await
            .context("failed to load UI preferences")?;
        let Some(row) = row else {
            return Ok(default_preferences());
        };
        let payload: String = row.try_get("payload")?;
        Ok(merge_preferences(
            default_preferences(),
            parse_json_object(&payload),
        ))
    }

    pub async fn update_preferences(&self, owner_profile_id: &str, patch: Value) -> Result<Value> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let mut current = self.get_preferences(&owner).await?;
        merge_patch(&mut current, patch);
        let now = now_iso();
        let payload = serde_json::to_string(&current)?;
        sqlx::query(
            r#"
            INSERT INTO ui_preferences (owner_profile_id, payload, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(owner_profile_id) DO UPDATE SET
                payload = excluded.payload,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&owner)
        .bind(payload)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("failed to update UI preferences")?;
        Ok(current)
    }

    pub async fn preference_fields(
        &self,
        owner_profile_id: &str,
        keys: &[String],
    ) -> Result<Value> {
        let preferences = self.get_preferences(owner_profile_id).await?;
        let mut fields = Map::new();
        for key in keys {
            if let Some(value) = preferences.get(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
        Ok(Value::Object(fields))
    }

    pub async fn list_themes(&self, owner_profile_id: &str) -> Result<Vec<ThemeScheme>> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let rows = sqlx::query(
            r#"
            SELECT id, name, mode, palette_json, created_at, updated_at
            FROM custom_theme_schemes
            WHERE owner_profile_id = ?
            ORDER BY updated_at DESC, name ASC
            "#,
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .context("failed to list custom theme schemes")?;
        rows.into_iter().map(theme_from_row).collect()
    }

    pub async fn create_theme(
        &self,
        owner_profile_id: &str,
        req: ThemeSchemeCreate,
    ) -> Result<ThemeScheme> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let now = now_iso();
        let scheme = ThemeScheme {
            id: req.id.unwrap_or_else(new_id),
            name: req.name.trim().to_string(),
            mode: req.mode.trim().to_string(),
            palette: req.palette,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        sqlx::query(
            r#"
            INSERT INTO custom_theme_schemes
                (id, owner_profile_id, name, mode, palette_json, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&scheme.id)
        .bind(owner)
        .bind(&scheme.name)
        .bind(&scheme.mode)
        .bind(serde_json::to_string(&scheme.palette)?)
        .bind(&scheme.created_at)
        .bind(&scheme.updated_at)
        .execute(&self.pool)
        .await
        .context("failed to create custom theme scheme")?;
        Ok(scheme)
    }

    pub async fn update_theme(
        &self,
        owner_profile_id: &str,
        id: &str,
        req: ThemeSchemeUpdate,
    ) -> Result<Option<ThemeScheme>> {
        let Some(mut scheme) = self.get_theme(owner_profile_id, id).await? else {
            return Ok(None);
        };
        if let Some(name) = req.name {
            scheme.name = name.trim().to_string();
        }
        if let Some(mode) = req.mode {
            scheme.mode = mode.trim().to_string();
        }
        if let Some(palette) = req.palette {
            scheme.palette = palette;
        }
        scheme.updated_at = now_iso();
        sqlx::query(
            r#"
            UPDATE custom_theme_schemes
            SET name = ?, mode = ?, palette_json = ?, updated_at = ?
            WHERE owner_profile_id = ? AND id = ?
            "#,
        )
        .bind(&scheme.name)
        .bind(&scheme.mode)
        .bind(serde_json::to_string(&scheme.palette)?)
        .bind(&scheme.updated_at)
        .bind(normalize_owner(owner_profile_id))
        .bind(id)
        .execute(&self.pool)
        .await
        .context("failed to update custom theme scheme")?;
        Ok(Some(scheme))
    }

    pub async fn delete_theme(&self, owner_profile_id: &str, id: &str) -> Result<bool> {
        self.ensure_ready().await?;
        let result =
            sqlx::query("DELETE FROM custom_theme_schemes WHERE owner_profile_id = ? AND id = ?")
                .bind(normalize_owner(owner_profile_id))
                .bind(id)
                .execute(&self.pool)
                .await
                .context("failed to delete custom theme scheme")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_prompts(
        &self,
        owner_profile_id: &str,
        filter: PromptFilter,
    ) -> Result<PromptList> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let rows = sqlx::query(
            r#"
            SELECT id, title, content, category, tags_json, is_favorite, created_at, updated_at
            FROM saved_prompts
            WHERE owner_profile_id = ?
            ORDER BY updated_at DESC, title ASC
            "#,
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .context("failed to list saved prompts")?;
        let mut prompts = Vec::new();
        for row in rows {
            let prompt = prompt_from_row(row)?;
            if !filter.matches(&prompt) {
                continue;
            }
            prompts.push(prompt);
        }
        let total = prompts.len() as i64;
        let offset = filter.offset.unwrap_or(0).max(0) as usize;
        let limit = filter.limit.unwrap_or(50).clamp(1, 200) as usize;
        Ok(PromptList {
            prompts: prompts.into_iter().skip(offset).take(limit).collect(),
            total,
        })
    }

    pub async fn create_prompt(
        &self,
        owner_profile_id: &str,
        req: PromptCreate,
    ) -> Result<SavedPrompt> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let now = now_iso();
        let prompt = SavedPrompt {
            id: req.id.unwrap_or_else(new_id),
            title: req.title.trim().to_string(),
            content: req.content,
            category: req.category.unwrap_or_else(|| "general".to_string()),
            tags: req.tags.unwrap_or_default(),
            is_favorite: req.is_favorite.unwrap_or(false),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        sqlx::query(
            r#"
            INSERT INTO saved_prompts
                (id, owner_profile_id, title, content, category, tags_json, is_favorite, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&prompt.id)
        .bind(owner)
        .bind(&prompt.title)
        .bind(&prompt.content)
        .bind(&prompt.category)
        .bind(serde_json::to_string(&prompt.tags)?)
        .bind(prompt.is_favorite)
        .bind(&prompt.created_at)
        .bind(&prompt.updated_at)
        .execute(&self.pool)
        .await
        .context("failed to create saved prompt")?;
        Ok(prompt)
    }

    pub async fn update_prompt(
        &self,
        owner_profile_id: &str,
        id: &str,
        req: PromptUpdate,
    ) -> Result<Option<SavedPrompt>> {
        let Some(mut prompt) = self.get_prompt(owner_profile_id, id).await? else {
            return Ok(None);
        };
        if let Some(title) = req.title {
            prompt.title = title.trim().to_string();
        }
        if let Some(content) = req.content {
            prompt.content = content;
        }
        if let Some(category) = req.category {
            prompt.category = category;
        }
        if let Some(tags) = req.tags {
            prompt.tags = tags;
        }
        if let Some(is_favorite) = req.is_favorite {
            prompt.is_favorite = is_favorite;
        }
        prompt.updated_at = now_iso();
        sqlx::query(
            r#"
            UPDATE saved_prompts
            SET title = ?, content = ?, category = ?, tags_json = ?, is_favorite = ?, updated_at = ?
            WHERE owner_profile_id = ? AND id = ?
            "#,
        )
        .bind(&prompt.title)
        .bind(&prompt.content)
        .bind(&prompt.category)
        .bind(serde_json::to_string(&prompt.tags)?)
        .bind(prompt.is_favorite)
        .bind(&prompt.updated_at)
        .bind(normalize_owner(owner_profile_id))
        .bind(id)
        .execute(&self.pool)
        .await
        .context("failed to update saved prompt")?;
        Ok(Some(prompt))
    }

    pub async fn delete_prompt(&self, owner_profile_id: &str, id: &str) -> Result<bool> {
        self.ensure_ready().await?;
        let result = sqlx::query("DELETE FROM saved_prompts WHERE owner_profile_id = ? AND id = ?")
            .bind(normalize_owner(owner_profile_id))
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to delete saved prompt")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_layouts(&self, owner_profile_id: &str) -> Result<Vec<WorkspaceLayout>> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let rows = sqlx::query(
            r#"
            SELECT id, name, layout_mode, slots_json, is_default, created_at, updated_at
            FROM workspace_layouts
            WHERE owner_profile_id = ?
            ORDER BY is_default DESC, updated_at DESC, name ASC
            "#,
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .context("failed to list workspace layouts")?;
        rows.into_iter().map(layout_from_row).collect()
    }

    pub async fn create_layout(
        &self,
        owner_profile_id: &str,
        req: LayoutCreate,
    ) -> Result<WorkspaceLayout> {
        self.ensure_ready().await?;
        let owner = normalize_owner(owner_profile_id);
        let now = now_iso();
        let layout = WorkspaceLayout {
            id: req.id.unwrap_or_else(new_id),
            name: req.name.trim().to_string(),
            layout_mode: req.layout_mode,
            slots: req.slots.unwrap_or_else(|| json!([])),
            is_default: req.is_default.unwrap_or(false),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        if layout.is_default {
            self.clear_default_layouts(&owner).await?;
        }
        sqlx::query(
            r#"
            INSERT INTO workspace_layouts
                (id, owner_profile_id, name, layout_mode, slots_json, is_default, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&layout.id)
        .bind(owner)
        .bind(&layout.name)
        .bind(&layout.layout_mode)
        .bind(serde_json::to_string(&layout.slots)?)
        .bind(layout.is_default)
        .bind(&layout.created_at)
        .bind(&layout.updated_at)
        .execute(&self.pool)
        .await
        .context("failed to create workspace layout")?;
        Ok(layout)
    }

    pub async fn update_layout(
        &self,
        owner_profile_id: &str,
        id: &str,
        req: LayoutUpdate,
    ) -> Result<Option<WorkspaceLayout>> {
        let Some(mut layout) = self.get_layout(owner_profile_id, id).await? else {
            return Ok(None);
        };
        if let Some(name) = req.name {
            layout.name = name.trim().to_string();
        }
        if let Some(layout_mode) = req.layout_mode {
            layout.layout_mode = layout_mode;
        }
        if let Some(slots) = req.slots {
            layout.slots = slots;
        }
        if let Some(is_default) = req.is_default {
            layout.is_default = is_default;
        }
        layout.updated_at = now_iso();
        let owner = normalize_owner(owner_profile_id);
        if layout.is_default {
            self.clear_default_layouts(&owner).await?;
        }
        sqlx::query(
            r#"
            UPDATE workspace_layouts
            SET name = ?, layout_mode = ?, slots_json = ?, is_default = ?, updated_at = ?
            WHERE owner_profile_id = ? AND id = ?
            "#,
        )
        .bind(&layout.name)
        .bind(&layout.layout_mode)
        .bind(serde_json::to_string(&layout.slots)?)
        .bind(layout.is_default)
        .bind(&layout.updated_at)
        .bind(owner)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("failed to update workspace layout")?;
        Ok(Some(layout))
    }

    pub async fn set_default_layout(
        &self,
        owner_profile_id: &str,
        id: &str,
    ) -> Result<Option<WorkspaceLayout>> {
        let Some(mut layout) = self.get_layout(owner_profile_id, id).await? else {
            return Ok(None);
        };
        let owner = normalize_owner(owner_profile_id);
        self.clear_default_layouts(&owner).await?;
        layout.is_default = true;
        layout.updated_at = now_iso();
        sqlx::query(
            "UPDATE workspace_layouts SET is_default = 1, updated_at = ? WHERE owner_profile_id = ? AND id = ?",
        )
        .bind(&layout.updated_at)
        .bind(owner)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("failed to set default workspace layout")?;
        Ok(Some(layout))
    }

    pub async fn delete_layout(&self, owner_profile_id: &str, id: &str) -> Result<bool> {
        self.ensure_ready().await?;
        let result =
            sqlx::query("DELETE FROM workspace_layouts WHERE owner_profile_id = ? AND id = ?")
                .bind(normalize_owner(owner_profile_id))
                .bind(id)
                .execute(&self.pool)
                .await
                .context("failed to delete workspace layout")?;
        Ok(result.rows_affected() > 0)
    }

    async fn ensure_ready(&self) -> Result<()> {
        self.ready
            .get_or_try_init(|| async {
                if let Some(parent) = self.db_path.parent() {
                    tokio::fs::create_dir_all(parent).await.with_context(|| {
                        format!(
                            "failed to create web UI state directory {}",
                            parent.display()
                        )
                    })?;
                }
                for statement in MIGRATIONS {
                    sqlx::query(statement)
                        .execute(&self.pool)
                        .await
                        .context("failed to apply web UI state migration")?;
                }
                Ok::<(), anyhow::Error>(())
            })
            .await?;
        Ok(())
    }

    async fn get_theme(&self, owner_profile_id: &str, id: &str) -> Result<Option<ThemeScheme>> {
        self.ensure_ready().await?;
        let row = sqlx::query(
            r#"
            SELECT id, name, mode, palette_json, created_at, updated_at
            FROM custom_theme_schemes
            WHERE owner_profile_id = ? AND id = ?
            "#,
        )
        .bind(normalize_owner(owner_profile_id))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to load custom theme scheme")?;
        row.map(theme_from_row).transpose()
    }

    async fn get_prompt(&self, owner_profile_id: &str, id: &str) -> Result<Option<SavedPrompt>> {
        self.ensure_ready().await?;
        let row = sqlx::query(
            r#"
            SELECT id, title, content, category, tags_json, is_favorite, created_at, updated_at
            FROM saved_prompts
            WHERE owner_profile_id = ? AND id = ?
            "#,
        )
        .bind(normalize_owner(owner_profile_id))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to load saved prompt")?;
        row.map(prompt_from_row).transpose()
    }

    async fn get_layout(
        &self,
        owner_profile_id: &str,
        id: &str,
    ) -> Result<Option<WorkspaceLayout>> {
        self.ensure_ready().await?;
        let row = sqlx::query(
            r#"
            SELECT id, name, layout_mode, slots_json, is_default, created_at, updated_at
            FROM workspace_layouts
            WHERE owner_profile_id = ? AND id = ?
            "#,
        )
        .bind(normalize_owner(owner_profile_id))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to load workspace layout")?;
        row.map(layout_from_row).transpose()
    }

    async fn clear_default_layouts(&self, owner_profile_id: &str) -> Result<()> {
        self.ensure_ready().await?;
        sqlx::query("UPDATE workspace_layouts SET is_default = 0 WHERE owner_profile_id = ?")
            .bind(owner_profile_id)
            .execute(&self.pool)
            .await
            .context("failed to clear default workspace layouts")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeScheme {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub palette: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSchemeCreate {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub mode: String,
    pub palette: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSchemeUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub palette: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPrompt {
    pub id: String,
    pub title: String,
    pub content: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCreate {
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub is_favorite: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptUpdate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub is_favorite: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptFilter {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub favorite: Option<bool>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

impl PromptFilter {
    fn matches(&self, prompt: &SavedPrompt) -> bool {
        if let Some(category) = &self.category {
            if prompt.category != *category {
                return false;
            }
        }
        if let Some(tag) = &self.tag {
            if !prompt.tags.iter().any(|candidate| candidate == tag) {
                return false;
            }
        }
        if let Some(favorite) = self.favorite {
            if prompt.is_favorite != favorite {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptList {
    pub prompts: Vec<SavedPrompt>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayout {
    pub id: String,
    pub name: String,
    pub layout_mode: String,
    pub slots: Value,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutCreate {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub layout_mode: String,
    #[serde(default)]
    pub slots: Option<Value>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub layout_mode: Option<String>,
    #[serde(default)]
    pub slots: Option<Value>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

pub fn normalize_owner(owner_profile_id: &str) -> String {
    let trimmed = owner_profile_id.trim();
    if trimmed.is_empty() {
        LOCAL_OWNER.to_string()
    } else {
        trimmed.to_string()
    }
}

fn default_preferences() -> Value {
    json!({
        "theme": "dark",
        "selectedSchemeByMode": {
            "dark": "dark-default",
            "light": "light-default"
        },
        "schemePaletteByMode": {
            "dark": null,
            "light": null
        },
        "customThemeSchemes": [],
        "sidebarMode": "simple",
        "sidebarCollapsed": false,
        "rightSidebarOpen": false,
        "rightSidebarTab": "file-commits",
        "rightSidebarWidth": 360,
        "chatMode": "normal",
        "customChatModes": [],
        "locale": "en"
    })
}

fn merge_preferences(mut base: Value, overlay: Value) -> Value {
    merge_patch(&mut base, overlay);
    base
}

fn merge_patch(target: &mut Value, patch: Value) {
    let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) else {
        return;
    };
    for (key, value) in patch {
        if key == "profile_id" || key == "profileId" {
            continue;
        }
        target.insert(key.clone(), value.clone());
    }
}

fn parse_json_object(raw: &str) -> Value {
    match serde_json::from_str::<Value>(raw) {
        Ok(value) if value.is_object() => value,
        _ => json!({}),
    }
}

fn parse_json_array(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn parse_json_value(raw: &str, fallback: Value) -> Value {
    serde_json::from_str::<Value>(raw).unwrap_or(fallback)
}

fn theme_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ThemeScheme> {
    let palette_json: String = row.try_get("palette_json")?;
    Ok(ThemeScheme {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        mode: row.try_get("mode")?,
        palette: parse_json_value(&palette_json, json!({})),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn prompt_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SavedPrompt> {
    let tags_json: String = row.try_get("tags_json")?;
    Ok(SavedPrompt {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        content: row.try_get("content")?,
        category: row.try_get("category")?,
        tags: parse_json_array(&tags_json),
        is_favorite: row.try_get("is_favorite")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn layout_from_row(row: sqlx::sqlite::SqliteRow) -> Result<WorkspaceLayout> {
    let slots_json: String = row.try_get("slots_json")?;
    Ok(WorkspaceLayout {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        layout_mode: row.try_get("layout_mode")?,
        slots: parse_json_value(&slots_json, json!([])),
        is_default: row.try_get("is_default")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preferences_are_profile_scoped() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = WebUiStore::new(temp.path().join("state.db"));

        let alice = store
            .update_preferences("alice", json!({ "theme": "light" }))
            .await
            .expect("alice update");
        let bob = store.get_preferences("bob").await.expect("bob get");

        assert_eq!(alice["theme"], "light");
        assert_eq!(bob["theme"], "dark");
    }

    #[tokio::test]
    async fn prompt_filters_apply_before_pagination() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = WebUiStore::new(temp.path().join("state.db"));
        store
            .create_prompt(
                "profile",
                PromptCreate {
                    id: None,
                    title: "React".into(),
                    content: "Use hooks".into(),
                    category: Some("code".into()),
                    tags: Some(vec!["react".into()]),
                    is_favorite: Some(true),
                },
            )
            .await
            .expect("create prompt");
        store
            .create_prompt(
                "profile",
                PromptCreate {
                    id: None,
                    title: "Ops".into(),
                    content: "Check logs".into(),
                    category: Some("ops".into()),
                    tags: Some(vec!["logs".into()]),
                    is_favorite: Some(false),
                },
            )
            .await
            .expect("create prompt");

        let list = store
            .list_prompts(
                "profile",
                PromptFilter {
                    tag: Some("react".into()),
                    favorite: Some(true),
                    ..PromptFilter::default()
                },
            )
            .await
            .expect("list prompts");

        assert_eq!(list.total, 1);
        assert_eq!(list.prompts[0].title, "React");
    }
}
