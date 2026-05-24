/// query loop 的终止原因
///
/// 对应 TypeScript: query.ts 中 return { reason: '...' } 的所有终止路径
#[derive(Debug, Clone)]
pub enum Terminal {
    /// 正常完成 (模型回复无工具调用)
    Completed,
    /// 流式阶段用户中断 (Ctrl+C / abort)
    AbortedStreaming,
    /// 工具执行阶段用户中断
    AbortedTools,
    /// 达到 token 硬上限 (auto-compact 关闭时)
    BlockingLimit,
    /// prompt 过长且不可恢复
    PromptTooLong,
    /// 图片大小/格式错误
    ImageError,
    /// API 调用异常
    ModelError { error: String },
    /// PostToolUse hook 阻止继续
    HookStopped,
    /// Stop hook 阻止
    StopHookPrevented,
    /// 达到最大轮次 (maxTurns)
    MaxTurns { turn_count: usize },
}

/// query loop 的继续原因
///
/// 对应 TypeScript: state.transition 的所有 continue 路径
#[derive(Debug, Clone)]
pub enum Continue {
    /// 正常循环: 工具结果已收集，回模型
    NextTurn,
    /// 上下文折叠排空后重试 (prompt_too_long 恢复第一步)
    CollapseDrainRetry { committed: usize },
    /// 响应式压缩后重试 (prompt_too_long 恢复第二步)
    ReactiveCompactRetry,
    /// 输出 token 上限升级 (8k → 64k)
    MaxOutputTokensEscalate,
    /// 输出 token 超限恢复 (注入续写消息)
    MaxOutputTokensRecovery { attempt: usize },
    /// stop hook 返回阻塞错误
    StopHookBlocking,
    /// token 预算未达 90% 继续
    TokenBudgetContinuation,
}
