use super::message::Message;
use super::transitions::Continue;

/// 自动压缩跟踪状态
///
/// 对应 TypeScript: AutoCompactTrackingState
#[derive(Debug, Clone)]
pub struct AutoCompactTracking {
    pub compacted: bool,
    pub turn_counter: usize,
    pub turn_id: String,
    pub consecutive_failures: usize,
}

/// query loop 的可变状态
///
/// 对应 TypeScript: query.ts 中的 State type
///
/// 每次循环迭代开始时解构此状态;
/// continue 站点通过 `state = State { ... }` 整体替换
#[derive(Debug)]
pub struct QueryLoopState {
    /// 当前对话历史
    pub messages: Vec<Message>,

    /// 自动压缩跟踪
    pub auto_compact_tracking: Option<AutoCompactTracking>,

    /// 输出 token 超限恢复计数 (最多 MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3)
    pub max_output_tokens_recovery_count: usize,

    /// 是否已尝试响应式压缩
    pub has_attempted_reactive_compact: bool,

    /// 是否已尝试 prompt-too-long 的上下文折叠排空
    pub has_attempted_collapse_drain: bool,

    /// 输出 token 上限覆盖 (escalate 时设为 ESCALATED_MAX_TOKENS)
    pub max_output_tokens_override: Option<usize>,

    /// 上一轮的工具使用摘要 (异步生成, 下一轮开始时消费)
    pub pending_tool_use_summary: Option<String>,

    /// stop hook 是否激活
    pub stop_hook_active: Option<bool>,

    /// 当前轮次计数
    pub turn_count: usize,

    /// 上一次迭代的 continue 原因 (第一次迭代为 None)
    pub transition: Option<Continue>,
}

impl QueryLoopState {
    /// 创建初始状态
    pub fn initial(messages: Vec<Message>) -> Self {
        Self {
            messages,
            auto_compact_tracking: None,
            max_output_tokens_recovery_count: 0,
            has_attempted_reactive_compact: false,
            has_attempted_collapse_drain: false,
            max_output_tokens_override: None,
            pending_tool_use_summary: None,
            stop_hook_active: None,
            turn_count: 1,
            transition: None,
        }
    }
}

/// Token 预算跟踪器
///
/// 对应 TypeScript: query/tokenBudget.ts 的 BudgetTracker
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    pub continuation_count: usize,
    pub last_delta_tokens: u64,
    pub last_global_turn_tokens: u64,
    pub started_at: i64,
}

impl BudgetTracker {
    pub fn new() -> Self {
        Self {
            continuation_count: 0,
            last_delta_tokens: 0,
            last_global_turn_tokens: 0,
            started_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Token 预算决策
#[derive(Debug, Clone)]
pub enum TokenBudgetDecision {
    Continue {
        nudge_message: String,
        continuation_count: usize,
        pct: usize,
        turn_tokens: u64,
        budget: u64,
    },
    Stop {
        completion_event: Option<BudgetCompletionEvent>,
    },
}

#[derive(Debug, Clone)]
pub struct BudgetCompletionEvent {
    pub continuation_count: usize,
    pub pct: usize,
    pub turn_tokens: u64,
    pub budget: u64,
    pub diminishing_returns: bool,
    pub duration_ms: u64,
}
