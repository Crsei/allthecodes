//! ProcessState — 进程级全局状态单例
//!
//! 对应 TypeScript: bootstrap/state.ts
//!
//! 设计约束:
//! - DO NOT ADD MORE STATE HERE — BE JUDICIOUS WITH GLOBAL STATE
//! - 只放真正需要全局访问的状态
//! - 与 AppState 的区别:
//!   AppState  = 会话级 UI/配置状态容器 (Arc<RwLock<AppState>>)
//!   ProcessState = 进程级不可变身份 + 累计统计 (全局单例)

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use super::diagnostics::{ErrorLog, SlowOperationTracker};
use super::ids::SessionId;
use super::model::ModelStrings;
use super::timing::DurationTracker;

/// 全局单例 — 通过 `PROCESS_STATE` 访问。
///
/// 读多写少场景使用 `RwLock`；计费统计 (`api_duration`, `tool_duration`)
/// 使用内部 `AtomicU64`，无需持锁。
pub static PROCESS_STATE: LazyLock<RwLock<ProcessState>> =
    LazyLock::new(|| RwLock::new(ProcessState::default()));

// ---------------------------------------------------------------------------
// ProcessState
// ---------------------------------------------------------------------------

pub struct ProcessState {
    // ── 路径/会话身份 (启动时设定) ────────────────────────────
    /// 进程启动时的工作目录 — 永不变更。
    /// 区别于 `utils/cwd.rs` 中的动态 CWD (工具执行时可能 cd)。
    pub original_cwd: PathBuf,

    /// 项目根目录 — 启动时通过 git/配置文件探测，不随 worktree 更新。
    /// 用于项目身份标识 (history, skills, sessions)，而非文件操作。
    pub project_root: PathBuf,

    /// 当前会话 ID。
    pub session_id: SessionId,

    /// 父会话 ID (子代理场景)。Lite 版暂不使用，预留。
    pub parent_session_id: Option<SessionId>,

    // ── 计费/性能统计 (累计，只增不减) ─────────────────────────
    /// 总花费 (USD)。由 QueryEngine 写入时同步更新。
    pub total_cost_usd: f64,

    /// API 调用累计耗时 — 使用 AtomicU64，无需持写锁。
    pub api_duration: DurationTracker,

    /// Tool 执行累计耗时 — 使用 AtomicU64，无需持写锁。
    pub tool_duration: DurationTracker,

    // ── 模型配置 ──────────────────────────────────────────────
    /// 用户在运行中通过 /model 命令覆盖的模型。
    pub main_loop_model_override: Option<String>,

    /// 启动时确定的初始模型 (CLI > config > provider default)。
    pub initial_main_loop_model: Option<String>,

    /// 模型显示字符串集合。
    pub model_strings: Option<ModelStrings>,

    // ── 会话标志位 ────────────────────────────────────────────
    /// 是否为交互式会话 (false = print mode / pipe mode)。
    pub is_interactive: bool,

    // ── 调试/诊断 ─────────────────────────────────────────────
    /// 内存错误日志 (最近 100 条，不写磁盘)。
    pub error_log: ErrorLog,

    /// 慢操作记录 (> 500ms 的操作，最近 50 条)。
    pub slow_operations: SlowOperationTracker,

    // ── Skill 追踪 ────────────────────────────────────────────
    /// 已调用的 skills — key: "agentId:skillName"。
    /// 跨 compaction 保留，确保 compaction 后仍能恢复 skill 发现状态。
    pub invoked_skills: HashMap<String, bool>,

    // ── 终端尺寸 (headless 模式由前端推送) ───────────────────
    /// 终端列数。Headless 模式由 `FrontendMessage::Resize` 更新。
    pub terminal_cols: u16,
    /// 终端行数。
    pub terminal_rows: u16,
}

impl Default for ProcessState {
    fn default() -> Self {
        Self {
            original_cwd: PathBuf::new(),
            project_root: PathBuf::new(),
            session_id: SessionId::new(),
            parent_session_id: None,
            total_cost_usd: 0.0,
            api_duration: DurationTracker::new(),
            tool_duration: DurationTracker::new(),
            main_loop_model_override: None,
            initial_main_loop_model: None,
            model_strings: None,
            is_interactive: true,
            error_log: ErrorLog::default(),
            slow_operations: SlowOperationTracker::default(),
            invoked_skills: HashMap::new(),
            terminal_cols: 80,
            terminal_rows: 24,
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience accessors (avoid holding locks longer than necessary)
// ---------------------------------------------------------------------------

impl ProcessState {
    /// 获取当前生效的模型 ID — override 优先于 initial。
    pub fn effective_model(&self) -> Option<&str> {
        self.main_loop_model_override
            .as_deref()
            .or(self.initial_main_loop_model.as_deref())
    }

    /// 记录一条诊断错误。
    pub fn log_error(&mut self, message: impl Into<String>, context: Option<String>) {
        self.error_log.push(message, context);
    }

    /// 记录一次可能的慢操作。
    pub fn record_operation(&mut self, name: impl Into<String>, duration_ms: u64) {
        self.slow_operations.record(name, duration_ms);
    }

    /// 标记一个 skill 已被调用。
    pub fn mark_skill_invoked(&mut self, agent_id: &str, skill_name: &str) {
        let key = format!("{}:{}", agent_id, skill_name);
        self.invoked_skills.insert(key, true);
    }

    /// 检查某个 skill 是否已被调用。
    pub fn is_skill_invoked(&self, agent_id: &str, skill_name: &str) -> bool {
        let key = format!("{}:{}", agent_id, skill_name);
        self.invoked_skills.contains_key(&key)
    }
}

// ---------------------------------------------------------------------------
// Free-standing helpers for common read-only access patterns
// (avoid requiring callers to manually acquire the RwLock)
// ---------------------------------------------------------------------------

/// 获取当前会话 ID (clone)。
pub fn session_id() -> SessionId {
    PROCESS_STATE.read().session_id.clone()
}

/// 获取 original_cwd (clone)。
pub fn original_cwd() -> PathBuf {
    PROCESS_STATE.read().original_cwd.clone()
}

/// 获取 project_root (clone)。
pub fn project_root() -> PathBuf {
    PROCESS_STATE.read().project_root.clone()
}

/// 获取当前总花费。
pub fn total_cost_usd() -> f64 {
    PROCESS_STATE.read().total_cost_usd
}

// ---------------------------------------------------------------------------
// Initialization helper
// ---------------------------------------------------------------------------

/// Initialize the global ProcessState at startup.
///
/// Must be called once during Phase B (full initialization) in main.rs.
/// Sets the immutable identity fields (paths, session, model, mode).
pub fn init(
    cwd: std::path::PathBuf,
    project_root: std::path::PathBuf,
    session_id: SessionId,
    is_interactive: bool,
    initial_model: Option<String>,
) {
    let mut state = PROCESS_STATE.write();
    state.original_cwd = cwd;
    state.project_root = project_root;
    state.session_id = session_id;
    state.is_interactive = is_interactive;
    state.initial_main_loop_model = initial_model;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_has_valid_session_id() {
        let state = ProcessState::default();
        assert!(!state.session_id.as_str().is_empty());
    }

    #[test]
    fn effective_model_override_wins() {
        let state = ProcessState {
            initial_main_loop_model: Some("sonnet".into()),
            main_loop_model_override: Some("opus".into()),
            ..Default::default()
        };
        assert_eq!(state.effective_model(), Some("opus"));
    }

    #[test]
    fn effective_model_falls_back_to_initial() {
        let state = ProcessState {
            initial_main_loop_model: Some("sonnet".into()),
            ..Default::default()
        };
        assert_eq!(state.effective_model(), Some("sonnet"));
    }

    #[test]
    fn skill_tracking() {
        let mut state = ProcessState::default();
        assert!(!state.is_skill_invoked("main", "commit"));
        state.mark_skill_invoked("main", "commit");
        assert!(state.is_skill_invoked("main", "commit"));
    }

    #[test]
    fn diagnostics_integration() {
        let mut state = ProcessState::default();
        state.log_error("test error", Some("context".into()));
        assert_eq!(state.error_log.len(), 1);

        state.record_operation("slow_api_call", 1000);
        assert_eq!(state.slow_operations.len(), 1);

        // Below threshold — not recorded
        state.record_operation("fast_op", 10);
        assert_eq!(state.slow_operations.len(), 1);
    }
}
