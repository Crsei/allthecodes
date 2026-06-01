# 全面功能差距分析：Claude Code 近两个月更新 vs allthecodes 实现

> 生成日期: 2026-05-31 (更新: 2026-06-01)
> 范围: Claude Code changelog v2.1.86 (2026-03-27) 至 v2.1.156 (2026-05-29) — 共 ~83 个版本
> 来源: `docs/reference/anthropic/what's_changed.md`
> 对比目标: allthecodes (Rust) 代码库 — 通过子代理深度审计
> 参考实现: `codex/` (OpenAI Codex) 和 `claude-code-bun/` (Claude Code Bun 实现)

---

## 总览

| 维度 | 数量 |
|------|------|
| **审计功能点总数** | ~100+ |
| **已实现 (IMPLEMENTED)** | ~30 |
| **部分实现 (PARTIALLY IMPLEMENTED)** | ~25 |
| **缺失 (MISSING)** | ~45+ |
| **参考代码库搜索** | `codex/` + `claude-code-bun/` — 8 个子代理，~100+ 文件 |

---

## 一、命令系统功能差距

### 1.1 /goal 命令（v2.1.139）
**状态：部分实现**
- 存在: `GoalRecord`, `GoalStatus`, 预算追踪, 持久化
- 工具存在: `GetGoalTool`, `CreateGoalTool`, `UpdateGoalTool` 等模型可调工具
- 缺失: **用户可用的 `/goal` 斜杠命令未注册**

**参考实现 — codex/codex-rs/tui/src/slash_command.rs:39**
- `SlashCommand::Goal` 变体在第 39 行注册
- 描述: `"set or view the goal for a long-running task"`（第 118 行）
- 支持内联参数（第 155 行）
- **实现方式**: TUI `SlashCommand` 枚举变体 + 事件派发

**参考实现 — codex 数据模型 (`codex-rs/state/src/model/thread_goal.rs:12-69`)**
```rust
pub(crate) enum ThreadGoalStatus { Active, Paused, Blocked, UsageLimited, BudgetLimited, Complete }
pub(crate) struct ThreadGoal {
    pub thread_id, pub goal_id, pub objective, pub status,
    pub token_budget, pub tokens_used, pub time_used_seconds,
    pub created_at, pub updated_at
}
```

**参考实现 — codex TUI 完整模块:**
- `tui/src/goal_display.rs` — 格式化目标耗时/状态/用量
- `tui/src/chatwidget/goal_status.rs` — `GoalStatusState` 状态指示器
- `tui/src/chatwidget/goal_menu.rs` — `show_goal_summary()`, `show_goal_edit_prompt()`
- `tui/src/chatwidget/goal_validation.rs` — 目标文本验证
- `tui/src/bottom_pane/footer.rs:97-105` — `GoalStatusIndicator` 枚举（底部状态栏显示）

**参考实现 — claude-code-bun: 无 `/goal` 命令**
- `goal` 仅作为字段名出现在 `autonomyFlows.ts:71`（`AutonomyFlowRecord.goal`）和 `insights.ts:262`（`underlying_goal`）
- `log.ts:26` 注释 "tick/goal tag" 指代主动模式的滴答标签，不是 goal 命令

**实现要点:**
- codex 使用 `SlashCommand::Goal` 枚举变体 + `ThreadGoalSetMode` 事件驱动
- 底部状态栏 `GoalStatusIndicator` 显示 Active/Paused/Blocked 等状态
- SQLite 持久化 (`state/src/model/thread_goal.rs`)

---

### 1.2 /feedback 命令（v2.1.141）
**状态：部分实现**
- 存在: "feedback" 作为 `/plan reject` 的参数
- 缺失: **无独立 `/feedback` 命令**, 无会话附件机制

**参考实现 — claude-code-bun: `src/commands/feedback/index.ts:6-25`**
- 类型: `local-jsx` 命令
- 别名: `bug`
- 描述: `'Submit feedback about Claude Code'`
- 启用条件: 非 Bedrock/Vertex/Foundry/essential-traffic-only/Ant 内部
- 实现: `src/commands/feedback/feedback.tsx:32-39` — 渲染 `<Feedback>` 组件
- 传递 `messages`, `abortSignal`, 可选 `initialDescription`

**参考实现 — codex: 全套反馈系统 (`codex-rs/feedback/src/lib.rs`)**
- `CodexFeedback`（第 164 行）— 主要反馈收集器，包装日志环回缓冲区
- `FeedbackMakeWriter`/`FeedbackWriter`（第 261/275 行）— 日志行捕获
- `FeedbackSnapshot`（第 338 行）— 序列化快照上传
- `FeedbackUploadOptions`（第 371 行）— 上传控制（分类、附件、标签）
- `FeedbackAttachment`/`FeedbackAttachmentPath`（第 357/345 行）

**参考实现 — codex TUI: `tui/src/bottom_pane/feedback_view.rs`**
- `FeedbackAudience`（第 40 行）— `OpenAiEmployee` 或 `External` 路由
- `FeedbackNoteView`（第 48 行）— 可选注释文本输入
- `feedback_selection_params()`（第 396 行）— 类别选择（错误/坏结果/好结果/安全检查/其他）
- `feedback_upload_consent_params()`（第 471 行）— 日志上传同意
- `feedback_success_cell()`（第 311 行）— GitHub/Slack 跟进链接

**参考实现 — codex slash dispatch: `tui/src/chatwidget/slash_dispatch.rs:151`**
- 检查 `config.feedback_enabled` → 启动选择弹出窗口

**实现要点:**
- claude-code-bun: 最小化实现，使用 `<Feedback>` React 组件
- codex: 完整反馈管道（日志捕获 → 分类 → 附件 → 上传 → 跟进链接）
- 两者都使用分类选择（错误/功能请求等）

---

### 1.3 /code-review --fix 和 /simplify（v2.1.147/152）
**状态：部分实现**
- 存在: `/simplify` 多智能体审查命令, `/review` PR 审查包装器
- 缺失: `/review` 无 `--fix` 标志（自动应用修复），无 `/code-review` 命令

**参考实现 — claude-code-bun `/review`: `src/commands/review.ts:9-43`**
- 类型: `prompt` 命令
- 构建 GH CLI 使用提示进行 PR 审查

**参考实现 — claude-code-bun `/ultrareview`: `src/commands/review.ts:48-54`**
- 类型: `local-jsx` 命令
- 通过 `teleportToRemote()` 启动远程 CCR 会话
- GrowthBook 功能门控 (`tengu_review_bughunter_config`)
- 文件: `ultrareviewCommand.tsx`, `ultrareviewEnabled.ts`, `UltrareviewOverageDialog.tsx`
- 计费: Extra Usage 系统 + 超额确认对话框

**参考实现 — claude-code-bun `/simplify`: `src/skills/bundled/simplify.ts:1-69`**
- 类型: **bundled skill**（非正式命令）
- 三阶段审查: git diff → 3 路并行审查代理（Code Reuse, Code Quality, Efficiency）→ 修复发现的问题
- 通过 `AgentTool` 启动并行代理
- 注册: `src/skills/bundled/index.ts:35` → `registerSimplifySkill()`

**参考实现 — codex: `tui/src/slash_command.rs:30`**
- `SlashCommand::Review` 变体，描述 `"review my current changes and find issues"`
- 无独立 `--fix` 标志；通过 Guardian 审批 + 自动审查拒绝处理
- 功能标志: `Feature::GuardianApproval`（`features/src/lib.rs:1096-1099`）

**实现要点:**
- claude-code-bun 的 `/simplify` 使用 bundled skill + AgentTool 并行化
- 修复通过 AgentTool 自动应用，不需要 `--fix` 标志
- codex 无相同实现

---

### 1.4 /tui fullscreen / 无闪烁渲染（v2.1.110）
**状态：部分实现**
- 存在: 全屏概念（注释中）, 转录模式 (Ctrl+O), 替代屏幕
- 缺失: **无 `/tui fullscreen` 命令**, 无显式无闪烁渲染切换

**参考实现 — codex: `protocol/src/config_types.rs:561+`**
- `AltScreenMode` 枚举: `Auto`, `Always`, `Never`
  ```rust
  /// Controls whether the TUI uses the terminal's alternate screen buffer.
  ```

**参考实现 — codex: `config/src/types.rs:682`**
- `pub alternate_screen: AltScreenMode` 配置字段

**参考实现 — codex: `tui/src/lib.rs:1732-1867`**
- `no_alt_screen` CLI 标志
- `determine_alt_screen_mode()` 函数
- `tui.set_alt_screen_enabled()` 方法
- 处理逻辑: CLI 标志优先 → 配置设置 → 默认 true

**参考实现 — codex CLI: `tui/src/cli.rs`**
- `"Disable alternate screen mode"` 标志

**参考实现 — claude-code-bun: `utils/fullscreen.ts:112`**
- `isFullscreenEnvEnabled()` 由 `CLAUDE_CODE_NO_FLICKER` 控制
- Ant 内部默认 ON (`CLAUDE_CODE_NO_FLICKER=0` 退出)
- 外部用户默认 OFF (`CLAUDE_CODE_NO_FLICKER=1` 启用)
- tmux -CC 自动禁用（iTerm2 集成模式）

**参考实现 — claude-code-bun: `packages/@ant/ink/src/components/AlternateScreen.tsx:1-65`**
- DEC 1049 alt screen 完整实现
- 鼠标跟踪启用/禁用
- `useInsertionEffect` 确保正确时序

**实现要点:**
- codex: `AltScreenMode` 三态枚举 + CLI 标志 + 配置字段
- claude-code-bun: env var 控制 + React 组件 + Ink alt screen 管理

---

### 1.5 /reload-skills 命令（v2.1.152）
**状态：部分实现**
- 存在: `reload_skills_with_extra()`, `/reload-plugins` 命令
- 缺失: 无 `/reload-skills` 命令或别名

**参考实现 — claude-code-bun: `src/commands/reload-plugins/`**
- 名称: `/reload-plugins`（无 `/reload-skills`）
- 类型: `local` 命令
- 实现: `reload-plugins.ts:10-61`
  - CCR/远程模式: 先 `redownloadUserSettings()`
  - 调用 `refreshActivePlugins()`
  - 返回已启用插件/命令/技能/代理/hook/MCP/LSP 服务器计数
  - 报告加载错误

**参考实现 — codex: `tui/src/app_command.rs:92-250`**
- `AppCommand::ReloadUserConfig`（第 92 行）
- `AppCommand::ListSkills { cwds, force_reload }`（第 95 行）
- `list_skills()` 构造函数（第 250 行）
- `tui/src/chatwidget.rs:1760` — `refresh_skills_for_current_cwd(force_reload: bool)`
- `tui/src/bottom_pane/skills_toggle_view.rs:223` — 以 `force_reload: true` 调用

**实现要点:**
- 两者都无独立的 `/reload-skills` 命令
- claude-code-bun 有 `/reload-plugins`（包含技能重新加载）
- codex 通过 `ReloadUserConfig` / `ListSkills(force_reload)` 实现

---

### 1.6 /usage 统一（v2.1.118）
**状态：部分实现**
- 存在: `/cost` (别名 "usage"), `/extra-usage` (隐藏), `/context`
- 缺失: 无合并 `/cost` + `/stats` 的统一 `/usage` 命令

**参考实现 — claude-code-bun: `src/commands/usage/`**
- 注册: `index.ts:3-9` — `name: 'usage'`, 别名: `['cost', 'stats']`
- 描述: `'Show session cost, plan usage, and activity stats'`
- 实现: `usage.tsx:14-16` → 渲染 `<Settings defaultTab="Usage">` 组件
- **统一命令**: 合并 `/cost` + `/stats`（作为别名保留）
- claude.ai 订阅者: 显示计划限制 + 超额用量
- API/非订阅者: 显示会话开销、token 计数、活动统计

**参考实现 — codex: `tui/src/slash_command.rs:100`**
- `SlashCommand::Status` — `"show current session configuration and token usage"`
- `tui/src/token_usage.rs` — `TokenUsage` 结构体（input/cached_input/output/reasoning_output/total_tokens）
- `tui/src/status/card.rs` — `StatusTokenUsageData`，在状态卡中渲染用量数据
- `tui/src/chatwidget/footer.rs:544-563` — 状态行中的目标用量显示
- 参考 URL: `"https://chatgpt.com/codex/settings/usage"`

**实现要点:**
- claude-code-bun 已统一 `/usage`（合并 `/cost` 和 `/stats`）
- codex 使用 `/status` 显示 token 用量 + 配置
- 关键差异: codex 有 `TokenUsageBreakdown`（类别细分），allthecodes 可通过 `/context` 查看

---

### 1.7 /scroll-speed 命令（v2.1.139）
**状态：部分实现**
- 存在: `ALLTHECODES_SCROLL_SPEED` 环境变量
- 缺失: **无 `/scroll-speed` 斜杠命令**

**参考实现 — claude-code-bun: `src/components/ScrollKeybindingHandler.tsx:305-316`**
```typescript
export function readScrollSpeedBase(): number {
  const raw = process.env.CLAUDE_CODE_SCROLL_SPEED;
  if (!raw) return 1;
  const n = parseFloat(raw);
  return Number.isNaN(n) || n <= 0 ? 1 : Math.min(n, 20);
}
```
- **实现方式**: 环境变量（非斜杠命令）
- 范围: (0, 20]，默认 1
- 两种模式: 原生终端（线性斜坡倍率）/ xterm.js（指数衰减曲线 + 突发检测）
- 惰性读取: `initAndLogWheelAccel()`（第 340 行）

**参考实现 — codex: 滚动基础设施**
- `tui/src/bottom_pane/scroll_state.rs` — `ScrollState` 带 `move_up_wrap()`, `move_down_wrap()`, `page_up_clamped()` 等
- `tui/src/pager_overlay.rs` — 完整寻呼机滚动实现（scroll_offset, scroll_up/down 键绑定）
- `tui/src/app_backtrack.rs` — 终端回滚滚动管理

**实现要点:**
- 两者都无 `/scroll-speed` 命令（只有 env var 或内置键绑定）
- claude-code-bun: `CLAUDE_CODE_SCROLL_SPEED` env var（配置文件可设置）
- codex: 无滚动速度配置，使用固定滚动步长

---

### 1.8 /proactive → /loop 别名（v2.1.105）
**状态：部分实现**
- 存在: `/loop` 命令, 守护进程主动滴答循环
- 缺失: `/proactive` 别名未注册

**参考实现 — claude-code-bun: `src/commands/proactive.ts:15-57`**
- **存在 `/proactive` 命令**（local-jsx）
- 描述: `'Toggle proactive (autonomous) mode'`
- 启用条件: `feature('PROACTIVE') || feature('KAIROS')`
- 状态机: `src/proactive/index.ts:15-136` — `active`, `paused`, `contextBlocked`
- React hook: `src/proactive/useProactive.ts:35-140` — 30s 间隔 `<tick>` 提示生成
- `/loop`: `src/skills/bundled/loop.ts:1-92` — **bundled skill**（非命令）
  - 注册: `registerLoopSkill()` (`src/skills/bundled/index.ts:38`)
  - cron 表达式调度 + CronCreate 工具调用
  - 门控: `isKairosCronEnabled`

**参考实现 — codex: 无 `/proactive` 或 `/loop` 命令**
- "proactive" 仅用于内部刷新上下文（`session_lifecycle.rs:631`）
- "loop" 仅用于宠物动画（`pets/model.rs:158`，serde rename）

**实现要点:**
- claude-code-bun 两个都有: `/proactive`（local-jsx 切换）+ `/loop`（bundled skill）
- `/proactive` 已有独立状态机，`/loop` 是 cron 调度工具
- 两者是独立的，非常不同

---

### 1.9 /undo → /rewind 别名（v2.1.108）
**状态：缺失**
- 存在: `/rewind` 命令已完整实现
- 缺失: **`/undo` 别名未注册**

**参考实现 — claude-code-bun: `src/commands/rewind/`**
- 注册: `index.ts:3-14` — `name: 'rewind'`, 别名: `['checkpoint']`
- 实现: `rewind.ts:4-13` — 调用 `context.openMessageSelector()`
- 无 `/undo` 命令

**参考实现 — codex: 已移除功能**
- `Feature::GhostCommit` — 功能已移除（`features/src/lib.rs:210-211`）
- 旧配置中的 `"undo"` 键被静默跳过（`features/src/lib.rs:422`）
- 测试: `undo_is_removed_and_disabled_by_default()`（`features/src/tests.rs:58`）
- 回溯系统: `tui/src/app_backtrack.rs` — 用于"回退"到更早用户消息

**实现要点:**
- 两者都无 `/undo` 命令
- claude-code-bun: `/rewind` + `checkpoint` 别名，打开消息选择器
- codex: `undo` 是已移除的功能标志；回溯系统可达到类似效果

---

### 1.10 /powerup 交互式教学（v2.1.90）
**状态：缺失**
- 代码库中完全没有 "powerup" / "interactive_teach" 的任何引用

**参考实现 — 两者都未找到**
- codex: 无引用
- claude-code-bun: 无引用

**实现要点:**
- 两个参考代码库都未实现此功能

---

### 1.11 /buddy 愚人节彩蛋（v2.1.89）
**状态：缺失**
- 代码库中完全没有 "buddy" 或 "easter egg" 的任何引用

**参考实现 — claude-code-bun: 完整彩蛋系统 (`src/commands/buddy/` + `src/buddy/`)**
- 注册: `index.ts:4-17` — `type: 'local-jsx'`, `name: 'buddy'`
- 子命令: `/buddy off`（静音）, `/buddy on`（取消静音）, `/buddy pet`（心形动画）
- 孵化: `buddy.ts:137-171` — 确定性种子生成 + 随机特质
- 模块: `companion.ts`（生成）, `types.ts`（类型）, `sprites.ts`（ASCII 精灵）
- 组件: `CompanionCard.tsx`, `CompanionSprite.tsx`
- 反应系统: `companionReact.ts` + `prompt.ts`（45s 最小间隔）
- 预告窗口: April 1-7, 2026
- 功能门控: `feature('BUDDY')`
- 18 个物种, 5 个稀有度, 8 顶帽子, 6 种眼睛, 5 项属性
- 1% 闪光概率

**参考实现 — codex: 宠物系统 (`tui/src/pets/`)**
- `SlashCommand::Pets`（`slash_command.rs:52-53`）— `"choose or hide the terminal pet"`
- 7 个内置宠物: codex, dewey, fireball, rocky, seedy, stacky, bsod
- 模块: `model.rs`（宠物加载和动画规格）, `catalog.rs`（内置宠物数组）
- `ambient.rs`（环境宠物绘制循环）, `picker.rs`（选择器 UI）
- `image_protocol.rs`（Kitty/Sixel 传输）
- 默认: `DEFAULT_PET_ID: "codex"`, `DISABLED_PET_ID: "disabled"`

**实现要点:**
- claude-code-bun: 功能门控的愚人节彩蛋（18 个物种 + 反应系统 + 闪光变体）
- codex: 全年的终端宠物系统（7 个宠物 + Kitty/Sixel 图像协议）
- 两者都使用确定性种子生成和持久化存储

---

### 1.12 /color 无参随机选色（v2.1.128）
**状态：缺失**
- 无 `/color` 命令；仅有智能体专用的颜色选择器 (agent color picker)

**参考实现 — claude-code-bun: `src/commands/color/`**
- **存在 `/color` 命令**（local-jsx, `index.ts:7-15`）
- 描述: `'Set the prompt bar color for this session'`
- 参数: `<color|default>`
- 实现: `color.ts:20-93`
  - 验证 `AGENT_COLORS` 列表
  - 重置别名: `default`, `reset`, `none`, `gray`, `grey`
  - 持久化: `saveAgentColor()` → 会话转录
  - 立即生效: 更新 `standaloneAgentContext.color`（AppState）
  - 禁用: swarm teammates（颜色由团队负责人分配）
- **无随机颜色功能**

**参考实现 — codex: `/theme` 命令**
- `SlashCommand::Theme`（`slash_command.rs:104`）— `"choose a syntax highlighting theme"`
- `tui/src/theme_picker.rs` — 完整主题选择器 UI，支持自定义 `.tmTheme` 文件
- `tui/src/terminal_palette.rs` — `StdoutColorLevel`（TrueColor/Ansi256/Ansi16）
- `tui/src/color.rs` — 颜色工具（`is_light()`, `blend()`, `perceptual_distance()` CIE76）
- `exec/src/cli.rs:60` — `--color` CLI 标志用于执行子进程

**实现要点:**
- claude-code-bun 有 `/color`（设置提示栏颜色），但无随机功能
- codex 有 `/theme`（设置语法高亮主题）
- 两者都没有无参随机选色

---

### 1.13 /release-notes 交互式版本选择器（v2.1.92）
**状态：缺失**
- 无对应的命令

**参考实现 — claude-code-bun: `src/commands/release-notes/`**
- **存在 `/release-notes` 命令**（local, `index.ts:3-11`）
- 实现: `release-notes.ts:19-50`
  - 从 GitHub raw URL 获取最新 changelog（500ms 超时）
  - 回退: `~/.claude/cache/changelog.md`
  - 解析 markdown 为 `[version, notes[]]` 对
  - 格式化为项目符号文本
  - 最终回退: 显示 GitHub changelog URL
- 核心逻辑: `src/utils/releaseNotes.ts`
  - URL: `https://raw.githubusercontent.com/anthropics/claude-code/refs/heads/main/CHANGELOG.md`
  - 缓存到 `~/.claude/cache/changelog.md`
  - 启动时 `checkForReleaseNotes()`
  - 最多显示 5 条（`MAX_RELEASE_NOTES_SHOWN = 5`）
  - Ant 内部构建: 编译时 `MACRO.VERSION_CHANGELOG`

**参考实现 — codex: 更新提示 (`tui/src/update_prompt.rs`)**
- `RELEASE_NOTES_URL = "https://github.com/openai/codex/releases/latest"`
- `run_update_prompt_if_needed()` — 显示更新对话框
- `tui/src/updates.rs` — `fetch_latest_github_release_version()`, `check_for_update()`
- `tui/src/update_action.rs` — `UpdateAction` 枚举（npm/bun/brew/standalone）
- **无独立 `/release-notes` 命令**

**实现要点:**
- claude-code-bun 有完整 `/release-notes` 命令（获取 + 缓存 + 格式化）
- codex 将发布信息集成在更新提示中，无独立命令

---

### 1.14 /team-onboarding（v2.1.101）
**状态：已实现**
- 已在 allthecodes 中完整实现并注册

---

### 1.15 其他缺失命令

| 命令 | codex | claude-code-bun | allthecodes 状态 |
|------|-------|------------------|------------------|
| **`/less-permission-prompts`** | ❌ 未找到 | ❌ 未找到 | **缺失** |
| **`/ultraplan`** | ❌ 未找到 | ✅ 已实现 | **缺失**（allthecodes: 仅常量定义） |
| **`/ultrareview`** | ❌ 未找到 | ✅ 已实现 | **缺失**（allthecodes: 仅常量定义） |
| **`claude agents --json`** | ❌ 未找到 | ❌ 未找到 | **缺失** |

**参考实现 — claude-code-bun `/ultraplan`: `src/commands/ultraplan.tsx:499-507`**
- 注册: `type: 'local-jsx'`, `name: 'ultraplan'`
- 描述: `'~10-30 min · Claude Code on the web drafts an advanced plan...'`
- 启用条件: `isUltraplanEnabled()`（GrowthBook: `tengu_ultraplan_config`）
- 实现（ultraplan.tsx:257-507）:
  1. 检查远程代理资格
  2. 构建提示（`buildUltraplanPrompt()`）
  3. `teleportToRemote()` with `ultraplan: true`
  4. 注册 `RemoteAgentTask` + `startDetachedPoll()`
  5. 批准后: 在本地或远程执行计划
- 文件: `ultraplan.tsx`, `UltraplanLaunchDialog.tsx`, `UltraplanChoiceDialog.tsx`
- 提示: `src/utils/ultraplan/prompt.txt` + `prompt.ts`

**参考实现 — claude-code-bun `/ultrareview`: `src/commands/review.ts:48-54`**
- 注册: `type: 'local-jsx'`, `name: 'ultrareview'`
- 文件: `ultrareviewCommand.tsx`, `ultrareviewEnabled.ts`
- GrowthBook: `tengu_review_bughunter_config`

---

## 二、插件系统功能差距

### 2.1 plugin defaultEnabled: false（v2.1.154）
**状态：缺失**
- 插件配置结构中无 `defaultEnabled` 字段

**参考实现 — codex: 核心设计模式**
- `features/src/lib.rs:713` — `FeatureSpec.default_enabled: bool`
- `features/src/lib.rs:268-269` — `Feature::default_enabled()` 方法
- `config/src/types.rs:58` — `const fn default_enabled() -> bool { true }`
- 用于: `AppsDefaultConfig.enabled`, `PluginConfig.enabled`, `PluginMcpServerConfig.enabled`, `AppConfig.enabled`, `SkillConfig.enabled`
- `app-server-protocol/src/protocol/v2/experimental_feature.rs:60` — 实验功能也有 `default_enabled`

**参考实现 — claude-code-bun: `src/types/plugin.ts:34`**
```typescript
export type BuiltinPluginDefinition = {
  name: string
  description: string
  version?: string
  skills?: BundledSkillDefinition[]
  hooks?: HooksSettings
  mcpServers?: Record<string, McpServerConfig>
  isAvailable?: () => boolean
  defaultEnabled?: boolean  // <-- 第 34 行
}
```
- 解析: `src/plugins/builtinPlugins.ts:74-76`
  ```typescript
  userSetting !== undefined ? userSetting : (definition.defaultEnabled ?? true)
  ```
- 用户设置优先 → `defaultEnabled` → 默认 `true`

**实现要点:**
- codex: 使用 `default_enabled()` 函数 + `#[serde(default = "default_enabled")]` 属性
- claude-code-bun: 可选的 `defaultEnabled?: boolean` 字段 + 三元表达式优先序
- allthecodes 可在 `PluginConfig` 中添加 `default_enabled: bool` 字段

---

### 2.2 插件依赖强制实施（v2.1.143）
**状态：部分实现**
- 存在: `dependency_resolver.rs` — 解决和验证插件间依赖
- 缺失: disable 不检查依赖; enable 不自动启用传递依赖

**参考实现 — codex: MCP 技能依赖（非插件依赖）**
- `core-skills/src/loader.rs:60-68` — `Dependencies` + `SkillDependencies` 结构体（`tools: Vec<SkillToolDependency>`）
- `core-skills/src/loader.rs:823-870` — `resolve_dependencies()` 解析技能 YAML
- `core/src/mcp_skill_dependencies.rs:34-430` — 完整 MCP 依赖安装管道
- `features/src/lib.rs:497-506` — `normalize_dependencies()` 用于功能依赖（父功能自动启用子功能）
- **无插件 enable/disable 依赖强制执行**

**参考实现 — claude-code-bun: 无插件依赖强制**
- 插件系统无依赖传播机制

**实现要点:**
- codex 的功能依赖系统（`normalize_dependencies()`）是 closest 参考
- MCP 技能依赖用于工具安装，非启用/禁用链

---

### 2.3 /plugin details / 组件清单（v2.1.139）
**状态：缺失**
- 无 `claude plugin details` 命令展示组件清单和 token 成本估算

**参考实现 — codex: 完整插件详情系统**
- `tui/src/chatwidget/plugins.rs` — 插件详情弹出:
  - `open_plugin_detail_loading_popup()`（第 395 行）
  - `on_plugin_detail_loaded()`（第 421 行）
  - `plugin_detail_popup_params()`（第 1621 行）— 完整详情视图，含安装/卸载操作
  - `plugin_detail_hint_line()`（第 1898 行）
  - `plugin_detail_description()`（第 2115 行）
- `tui/src/app/event_dispatch.rs:472-616` — 事件处理
- `tui/src/app/background_requests.rs:157-166` — `fetch_plugin_detail()` 发送 `PluginRead` 请求
- `core-plugins/src/remote.rs:755-854` — `fetch_remote_plugin_detail()`, `build_remote_plugin_detail()`
- `core-plugins/src/manager.rs:1307-1339` — `read_plugin_detail_for_marketplace_plugin()`
- `app-server/src/request_processors/plugins.rs:906-1953` — 插件读取请求处理器
- 协议: `app-server-protocol/src/protocol/v2/plugin.rs` — `PluginDetail` 类型

**参考实现 — claude-code-bun: `src/commands/plugin/`**
- `/plugin` 命令（别名: `plugins`, `marketplace`）
- 组件: `PluginSettings.tsx`（主 UI）+ `ManagePlugins.tsx`（已安装列表）+ `BrowseMarketplace.tsx`（浏览）+ `DiscoverPlugins.tsx`（发现）
- **无详细组件清单或 token 成本显示**

**实现要点:**
- codex 有最完整的插件详情系统（RPC 获取 + 弹出窗口 + 安装/卸载
- claude-code-bun 通过 JSX 组件集合管理插件
- 两者都有基本的插件详情查看功能

---

### 2.4 claude plugin prune（v2.1.121）
**状态：部分实现**
- 存在: `reconciler.rs` 内部检测孤立插件
- 缺失: **无用户可用的 `/plugin prune` 命令**

**参考实现 — claude-code-bun: 内部清理**
- `src/utils/plugins/loadPluginHooks.ts:179-207` — `pruneRemovedPluginHooks()`
  - 内部机制: 过滤已注册的 hook，只保留启用插件中的
  - 原子操作: 清除 + 重新注册幸存者
  - **无用户可用的命令**

**参考实现 — codex: 未找到**
- "prune" 仅用于不相关上下文（keymap 清理、日志保留、图片附件清理）
- 无插件清理命令或 reconciler

**实现要点:**
- 两者都无用户可用的 `/plugin prune` 命令
- claude-code-bun 有内部 `pruneRemovedPluginHooks()`

---

### 2.5 claude plugin tag（v2.1.118）
**状态：缺失**
- 无 git 发布标签创建命令

**参考实现 — 两者都未找到**
- codex: 无
- claude-code-bun: `/tag` 命令存在但仅用于会话标签（`src/commands/tag/index.ts`，仅 Ant 内部）
- 无 `plugin tag` 概念

---

### 2.6 pluginSuggestionMarketplaces 托管设置（v2.1.152）
**状态：缺失**
- 代码库中完全不存在

**参考实现 — claude-code-bun: 完整市场系统**
- `src/utils/plugins/marketplaceManager.ts` — 市场源管理
- `src/utils/plugins/marketplaceHelpers.ts` — `loadMarketplacesWithGracefulDegradation()`
- `src/utils/plugins/officialMarketplace.ts` — 官方 Anthropic 市场
- `src/commands/plugin/BrowseMarketplace.tsx` — 浏览 UI（分页、搜索、安装计数）
- `src/commands/plugin/AddMarketplace.tsx` — 添加市场源（GitHub URL 等）
- 插件标识格式: `plugin:{name}@{marketplace}`

**参考实现 — codex: 完整市场系统**
- `core-plugins/src/marketplace_add/` — `source.rs:18-381`（解析/暂存/验证）+ `mod.rs:23-337`（添加/删除）
- `tui/src/app_event.rs:372-438` — 市场事件（FetchPluginMarketplaceState, OpenMarketplaceAddPrompt, MarketplaceAddLoaded 等）
- `tui/src/chatwidget/plugins.rs:290-1271` — 市场 UI（添加提示、加载、错误状态）
- `core-plugins/src/startup_remote_sync.rs:16-91` — 启动时远程插件同步
- 源代码市场存储在 codex home 目录下

**实现要点:**
- 两者都有完整的市场系统（添加/删除/浏览/安装）
- `PluginSuggestionMarketplaces` 作为标识符未在任一代码库中找到

---

### 2.7 --plugin-url 标志（v2.1.129）
**状态：缺失**
- CLI 中无此标志

**参考实现 — 两者都未找到**
- codex: 无
- claude-code-bun: 无

---

## 三、Hook 系统功能差距

### 3.1 MessageDisplay hook（v2.1.152）
**状态：缺失**
- HookEvent 枚举中无此变体

**参考实现 — 两者都未找到**
- codex: `HookEventName` 枚举（`protocol/src/protocol.rs:1355-1366`）无 `MessageDisplay` 变体
- claude-code-bun: hook 事件类型（`entrypoints/agentSdkTypes.ts`）无 `MessageDisplay`
- codex 有 `UserMessageDisplay` 结构体（`tui/src/chatwidget/user_messages.rs:496`），但那是 TUI 渲染结构，非 hook 事件

---

### 3.2 SessionStart reloadSkills: true（v2.1.152）
**状态：部分实现**
- 存在: `reload_skills_with_extra()` 函数
- 缺失: SessionStart hook 配置中无 `reloadSkills: true` 参数

**参考实现 — claude-code-bun: 未找到 `reloadSkills` 配置字段**
- `SessionStart` hook 事件存在
- 无关联的 `reloadSkills` 配置字段

**参考实现 — codex: 无特定字段**
- `SessionStart` hook 事件存在（`hooks/src/schema.rs:99-120`）
- `reload_user_config: true` 在 `hooks_rpc.rs:87` 中使用（用于用户配置，非技能）
- `tui/src/chatwidget.rs:1760` — `refresh_skills_for_current_cwd(force_reload: bool)` 内部调用

---

### 3.3 PermissionDenied hook（v2.1.89）
**状态：已实现**
- HookEvent::PermissionDenied 是完整变体

---

### 3.4 PreCompact hook（v2.1.105）
**状态：已实现**
- HookEvent::PreCompact 是完整变体

---

### 3.5 Hook continueOnBlock（v2.1.139）
**状态：缺失**
- PostToolUse hook 结果中无 `continueOnBlock` 字段

**参考实现 — codex: `hooks/src/engine/output_parser.rs:3`**
- `UniversalOutput.continue_processing: bool` — 通用"处理是否继续"标志
- `hooks/src/schema.rs:87-96` — `HookUniversalOutputWire.r#continue: bool`
- **无 `continueOnBlock` 字段**
- 使用 `should_block` 字段进行块决策，`continue_processing` 用于继续/停止

**参考实现 — claude-code-bun: 未找到**

---

### 3.6 Hook terminalSequence（v2.1.141）
**状态：缺失**
- 代码库中完全不存在

**参考实现 — 两者都未找到**

---

### 3.7 Hook args: string[] 执行形式（v2.1.139）
**状态：缺失**
- HookEntry 变体使用单一 `command: String` 传递给 `bash -c`
- 无 `args: Vec<String>` 字段

**参考实现 — codex: `hooks/src/engine/mod.rs:36-38`**
```rust
pub(crate) struct CommandShell {
    pub program: String,
    pub args: Vec<String>,
}
```
- `hooks/src/engine/mod.rs:42-52` — `ConfiguredHandler` 有 `pub command: String`
- `hooks/src/registry.rs:38` — `pub shell_args: Vec<String>`
- `hooks/src/registry.rs:225` — `pub fn command_from_argv(argv: &[String]) -> Option<Command>`
- `config/src/hook_config.rs:142-144` — `HookHandlerConfig::Command { command: String, command_windows: Option<String> }`

**参考实现 — claude-code-bun: `src/utils/hooks.ts:1023-1118`**
- Bash hooks: `spawn(sandboxedCommand, [], { shell, ... })` — 单字符串，无 args 数组
- PowerShell hooks: `spawn(pwshPath, buildPowerShellArgs(finalCommand), ...)` — 固定包装器，非用户提供 args
- MCP add 命令使用 args 数组模式（`addCommand.ts:267`），但非 hook 系统

**实现要点:**
- codex 有 `CommandShell` 结构体带 `program` + `args`，但 `ConfiguredHandler` 仍用 `command` 字符串
- claude-code-bun 完全使用单字符串传递给 shell

---

### 3.8 PostToolUse updatedToolOutput（v2.1.121）
**状态：缺失**
- HookOutput 中有 `updated_input` 但无 `updated_tool_output`

**参考实现 — codex: `hooks/src/schema.rs:233`**
- `pub updated_mcp_tool_output: Option<Value>`（序列化为 `updatedMCPToolOutput`）
- **但运行时拒绝**（`output_parser.rs:424-432`）:
  ```rust
  "PostToolUse hook returned unsupported updatedMCPToolOutput"
  ```
- PreToolUse 的 `updated_input` 已完全实现（`events/pre_tool_use.rs:43-243`）

**参考实现 — claude-code-bun: 未找到**
- `updatedToolUseContext` 存在（`query.ts:1636-1988`），但非 same

**实现要点:**
- codex: `updated_mcp_tool_output` 在 schema 中定义但运行时未支持（预留未来）
- 模式: 序列化支持就先到位，运行时节流

---

### 3.9 disallowed-tools 前端设置（v2.1.152）
**状态：已实现**
- 智能体定义解析器支持 `disallowedTools` / `disallowed_tools` 前端键

---

### 3.10 Hook 条件 if 字段（v2.1.85）
**状态：已实现**
- 所有四个 HookEntry 变体都有 `if_condition: Option<String>` 字段

---

### 3.11 PreToolUse hook 满足 AskUserQuestion（v2.1.85）
**状态：缺失**
- PreToolUse hook 输出无法直接回答用户问题

**参考实现 — codex: `hooks/src/schema.rs:252-259`**
```rust
pub(crate) enum PreToolUsePermissionDecisionWire {
    Allow,
    Deny,
    Ask,  // <-- "ask" 用户变体
}
```
- **运行时拒绝 `Ask`**: `output_parser.rs:451-453`
  ```rust
  Some(PreToolUsePermissionDecisionWire::Ask) => {
      Some("PreToolUse hook returned unsupported permissionDecision:ask".to_string())
  }
  ```
- 测试确认 `Ask` 产生错误（`events/pre_tool_use.rs:537-557`）

**实现要点:**
- codex: `Ask` 在 schema 级别预留但未运行时实现
- 与 `updated_mcp_tool_output` 相同模式: schema first，实现延迟

---

### 3.12 Elicitation / ElicitationResult hooks（v2.1.76）
**状态：已实现**
- HookEvent 枚举中两个都是完整变体

---

## 四、MCP 与工具系统功能差距

### 4.1 alwaysLoad MCP 服务器配置（v2.1.121）
**状态：缺失**
- McpServerConfig 中无 `alwaysLoad` 字段

**参考实现 — claude-code-bun: `src/Tool.ts:464-470`**
```typescript
readonly alwaysLoad?: boolean
```
- 来源: MCP 服务器 `_meta['anthropic/alwaysLoad']`（`client.ts:1797`）
- `isDeferredTool()` 对 `alwaysLoad: true` 返回 `false`（`constants/tools.ts:132`）
- SDK 类型: `entrypoints/agentSdkTypes.ts:86` — `alwaysLoad?: boolean`

**参考实现 — codex: 未找到**
- 无 `alwaysLoad` 概念

**实现要点:**
- claude-code-bun 支持的完整实现: MCP 服务器元数据 → `Tool.alwaysLoad` → 从不延迟加载
- allthecodes 可在 `McpServerConfig` 添加类似字段

---

### 4.2 MCP workspace 保留名称（v2.1.128）
**状态：缺失**
- MCP 子系统使用用户配置的服务器名称，无保留名称

**参考实现 — claude-code-bun: `src/services/mcp/config.ts:635-648`**
- 保留名称: `claude-in-chrome`（`utils/claudeInChrome/common.ts:12`）+ `computer-use`（`utils/computerUse/common.ts:4`）
- 错误: `"Cannot add MCP server \"<name>\": this name is reserved."`
- 服务器名称限制: `[a-zA-Z0-9_-]` 仅

**参考实现 — codex: `app-server/src/request_processors/thread_processor.rs:270-298`**
- 动态工具保留命名空间验证:
  ```rust
  "dynamic tool name is reserved: {name}"
  "dynamic tool namespace is reserved for {name}: {namespace}"
  "dynamic tool namespace collides with a reserved Responses API namespace for {name}: {namespace}""
  ```
- 测试: `thread_processor_tests.rs:271` — `validate_dynamic_tools_rejects_reserved_namespace()`

**实现要点:**
- claude-code-bun: 硬编码保留名称（claude-in-chrome, computer-use）
- codex: 动态工具保留命名空间验证（更全面的模式）

---

### 4.3 流式工具执行始终启用（v2.1.154）
**状态：默认未启用**
- 流式工具执行存在但门控在 `ALLTHECODES_STREAMING_TOOL_EXECUTION` 环境变量后，默认 false

**参考实现 — claude-code-bun: `src/services/tools/StreamingToolExecutor.ts`**
- 完整实现（561 行），门控在 `tengu_streaming_tool_execution2` Statsig 后
- 设计:
  - `TrackedTool` 队列（第 42 行）
  - `addTool()` — 立即开始执行（第 90 行）
  - 并发控制: 并发安全的工具并行，非并发需要独占（第 156-161 行）
  - `executeTool()` — 通过 `runToolUse()` 启动（第 292-432 行）
  - `getCompletedResults()` — 同步返回有序结果（第 439-467 行）
  - `discard()` — 流降级时中止所有正在运行的 tools（第 73 行）
- query.ts 集成: 第 744-1291 行

**参考实现 — codex: 未找到**
- 无流式工具执行概念

**实现要点:**
- claude-code-bun 有最完整实现（Statsig 门控）
- allthecodes 的 env var 门控可以保持，但应默认启用对标 claude-code-bun

---

### 4.4 MCP 工具结果持久化覆盖（v2.1.91）
**状态：部分实现**
- 存在: `applyToolResultBudget` 管道持久化过大工具结果
- 缺失: 无每个 MCP 服务器的配置覆盖

**参考实现 — claude-code-bun: `src/services/mcp/client.ts:2615-2846`**
- 二进制 blob: `persistBlobToTextBlock()`（第 2620 行）
- 大结果: `processMCPResult()`（第 2745 行）
  - 小 → 返回原样
  - 大 + `ENABLE_MCP_LARGE_OUTPUT_FILES` 禁用 → 截断
  - 大 + 功能启用 → `persistToolResult()` 到磁盘
- 文件 ID 格式: `mcp-{serverName}-{toolName}-{timestamp}`
- `src/utils/toolResultStorage.ts:272` — `maybePersistLargeToolResult()`（每工具 `maxResultSizeChars` 阈值）

**参考实现 — codex: `app-server-protocol/src/protocol/thread_history.rs:2074`**
- `reconstructs_mcp_tool_result_meta_from_persisted_completion_events` 测试
- MCP 工具结果元数据从持久化完成事件重构

**实现要点:**
- claude-code-bun 有完整 `persistBlobToTextBlock` + `processMCPResult` + `maybePersistLargeToolResult`
- allthecodes 可添加每 MCP 服务器配置覆盖

---

### 4.5 Slack 消息工具紧凑头部（v2.1.94）
**状态：缺失**
- SlackChannelCompletionProvider 存在但无紧凑头部渲染

**参考实现 — claude-code-bun: `packages/builtin-tools/src/tools/MCPTool/UI.tsx:324-356`**
- `SLACK_ARCHIVES_RE` 正则: `https://[a-z0-9-]+.slack.com/archives/([A-Z0-9]+)/p\d+`
- `trySlackSendCompact()`（第 333-356 行）:
  - 检测 Slack 发送消息结果
  - 返回紧凑 `{channel, url}` 对
  - 渲染: `"Sent a message to <channel>"` 带超链接（非详细模式）
  - 渠道标签回退: `slack`
- 匹配托管 (claude.ai Slack) 和社区 MCP 服务器形状

**参考实现 — codex: `tui/src/history_cell/mcp.rs:140-151`**
- `compact_spans`, `compact_header`, `reserved` 宽度跟踪
- 通用 MCP 紧凑头部渲染（非 Slack 特定）

**实现要点:**
- claude-code-bun: Slack 特定紧凑渲染（正则匹配 + 渠道提取 + 超链接）
- codex: 通用 MCP 紧凑头部

---

### 4.6 Read 工具截断首页 PARTIAL 视图（v2.1.145）
**状态：缺失**
- FileReadTool 有分页但无 PARTIAL 视图通知

**参考实现 — claude-code-bun: `src/utils/fileStateCache.ts:11-14`**
- `isPartialView?: boolean` — 表示模型仅看到文件的局部视图

**参考实现 — claude-code-bun: `src/utils/attachments.ts:1792-1802`**
- 加载记忆文件时设置 `isPartialView: true`
- 标志告诉 Edit/Write 在编辑前需要真实文件读取

**参考实现 — claude-code-bun: `src/services/compact/prompt.ts:145-293`**
- `PARTIAL_COMPACT_PROMPT` — 仅总结最近消息
- `PARTIAL_COMPACT_UP_TO_PROMPT` — 总结到某一点
- `getPartialCompactPrompt()` — 带方向的紧凑提示
- **这些是关于紧凑部分提示，非 Read 工具 PARTIAL 视图**

**实现要点:**
- claude-code-bun 使用 `isPartialView` 用于文件状态跟踪（记忆文件）
- 无正式 `PARTIAL` API 响应类型用于 Read 工具截断
- allthecodes 可添加 Read 工具 PARTIAL 通知

---

## 五、UI/显示功能差距

### 5.1 自定义主题 themes/ 目录支持和 JSON 编辑（v2.1.118）
**状态：部分实现**
- 存在: 命名主题 (dark/light/auto/solarized/monokai/nord)
- 缺失: 插件中无 `themes/` 目录，无 JSON 文件编辑

**参考实现 — codex: 自定义 `.tmTheme` 支持 (`tui/src/render/highlight.rs`)**
- `custom_theme_path()`（第 175 行）— `{CODEX_HOME}/themes/`
- `load_custom_theme()`（第 180 行）— 加载自定义 `.tmTheme` 文件
- `list_available_themes()`（第 366 行）— 发现捆绑 + 自定义主题（去重）
- `theme_picker.rs:284` — `let themes_dir = codex_home.map(|home| home.join("themes"));`
- 配置: `config/src/types.rs:705-710` — `theme: Option<String>` 字段

**参考实现 — claude-code-bun: `src/utils/theme.ts`**
- 6 个内置主题: `dark`, `light`, `light-daltonized`, `dark-daltonized`, `light-ansi`, `dark-ansi`
- `ThemePicker` 组件（`src/components/ThemePicker.tsx`）— 交互式选择器 + 预览
- 自动主题检测: `src/utils/systemTheme.ts` — OSC 11 解析

**实现要点:**
- codex: 支持 `{CODEX_HOME}/themes/` 自定义目录 + `.tmTheme` 文件
- claude-code-bun: 仅内置主题，无自定义目录
- allthecodes 可添加 `$ALLTHECODES_HOME/themes/` 目录支持

---

### 5.2 autoScrollEnabled 配置（v2.1.110）
**状态：缺失**
- 代码库中完全不存在

**参考实现 — 两者都未找到用户可设置配置**
- claude-code-bun: 仅有拖拽自动滚动计时器（`ScrollKeybindingHandler.tsx:349-359`），非用户配置
- codex: 无

---

### 5.3 努力滑块标签 "Faster"/"Smarter"（v2.1.154）
**状态：缺失**
- 努力标签使用 "low"/"medium"/"high"/"auto"/"max"

**参考实现 — claude-code-bun: `src/commands/effort/effort.tsx:158`**
- 有效级别: `low`, `medium`, `high`, `xhigh`, `max`, `auto`
- 描述: quick/straightforward, balanced, comprehensive, extended reasoning, maximum capability, default

**参考实现 — codex: `tui/src/chatwidget/model_popups.rs:512-521`**
```rust
pub(super) fn reasoning_effort_label(effort: ReasoningEffortConfig) -> &'static str {
    match effort {
        ReasoningEffortConfig::None => "None",
        ReasoningEffortConfig::Minimal => "Minimal",
        ReasoningEffortConfig::Low => "Low",
        ReasoningEffortConfig::Medium => "Medium",
        ReasoningEffortConfig::High => "High",
        ReasoningEffortConfig::XHigh => "Extra high",
    }
}
```
- 标签: `None`, `Minimal`, `Low`, `Medium`, `High`, `Extra high`
- **两者都无 "Faster"/"Smarter" 标签**

---

### 5.4 /diff 详情视图键盘滚动（v2.1.149）
**状态：部分实现**
- diff 详情视图存在
- 缺失: 无显式键盘滚动处理（PgUp/PgDn, j/k）

**参考实现 — codex: `cloud-tasks/src/scrollable_diff.rs:21-34`**
```rust
pub struct ScrollableDiff { /* ... */ }
impl ScrollableDiff {
    pub fn scroll_by(&mut self, delta: isize) { /* ... */ }
    pub fn scroll_to_top(&mut self) { /* ... */ }
    pub fn scroll_to_bottom(&mut self) { /* ... */ }
}
```
- 键盘处理: `cloud-tasks/src/lib.rs:1696-1712`
  ```rust
  ov.sd.scroll_by(/*delta*/ 1)  // 下
  ov.sd.scroll_by(/*delta*/ -1) // 上
  KeyCode::Home => ov.sd.scroll_to_top()
  KeyCode::End => ov.sd.scroll_to_bottom()
  ```
- UI: `cloud-tasks/src/ui.rs:312-441` — `draw_diff_overlay()`

**参考实现 — claude-code-bun: 通用键盘滚动**
- PgUp/PgDn 键绑定: `pageup: 'scroll:pageUp'`, `pagedown: 'scroll:pageDown'`（`defaultBindings.ts:209-210`）
- 应用于全屏消息滚动，非 diff 特定
- `StructuredDiff` 组件渲染所有行，无自身滚动

**实现要点:**
- codex: diff 特定 `ScrollableDiff` + Home/End 支持
- claude-code-bun: 通用 PgUp/PgDn，非 diff 特定

---

### 5.5 /model 保存选择为默认（v2.1.153）
**状态：部分实现**
- 模型选择器存在
- 缺失：无独立 `/model` 命令持久化选择为默认

**参考实现 — claude-code-bun: `src/commands/model/model.tsx`**
- `/model` 命令存在（local-jsx），处理内联名称和交互式选择器
- `mainLoopModel` 存储在 `AppState` 中
- 跨会话持久化: 设置模型通过 `/model` 持久化（隐式，非显式"另存为默认"切换）

**参考实现 — codex: `tui/src/app_event.rs:631`**
- `PersistModelSelection { model, effort }` 事件
- 事件派发: `event_dispatch.rs:1293` — 保存模型配置
- `model_popups.rs:232` — 选择模型时发送 `PersistModelSelection`
- `config_lock.rs:93` — 检查 `config_lock_save_fields_resolved_from_model_catalog`
- `/model` 不是命令；模型通过 `/status` 菜单 + 弹出窗口选择

**实现要点:**
- claude-code-bun: 隐式持久化模型（选择即保存）
- codex: 显式 `PersistModelSelection` 事件 + 配置锁
- allthecodes 可添加显式"保存为默认"操作

---

### 5.6 /model 选择器显示快速模式定价（v2.1.153）
**状态：部分实现**
- 快速模式标志和定价模块存在
- 缺失：模型选择器中无显式"快速模式定价"行

**参考实现 — codex: 无**
- codex 无快速模式概念

**参考实现 — claude-code-bun: 未找到选择器中的定价显示**
- 快速模式支持存在（`isFastMode()` 等），但选择器 UI 中无定价行

---

### 5.7 /usage 按类别细分（v2.1.149）
**状态：部分实现**
- `/context` 显示按类别细分
- 缺失：无独立 `/usage` 命令

**参考实现 — claude-code-bun: 上下文用法细分**
- `src/entrypoints/sdk/controlSchemas.ts:180-191` — `getContextUsage` SDK 请求 + `ContextCategorySchema`
- `src/commands/context/context-noninteractive.ts:159-179` — 按类别 markdown 表输出
- `src/components/ContextVisualization.tsx:179-199` — 类别可视化（符号 + 名称 + token 计数 + 百分比）
- `src/utils/analyzeContext.ts:169-609` — 上下文用量分析（技能、工具、总 token）

**参考实现 — codex: `protocol/v2/thread.rs:1280-1313`**
- `TokenUsageBreakdown` 结构体
- `core/src/context_manager/history.rs:54` — `TotalTokenUsageBreakdown`

**实现要点:**
- codex: `TokenUsageBreakdown` 协议类型
- claude-code-bun: 完整类别可视化 + SDK 支持
- allthecodes 已有 `/context` 显示类别细分

---

### 5.8 GPU/CJK 文本渲染改进
**状态：部分实现**
- 存在: 广泛使用 `unicode_width` crate 处理 CJK
- 缺失: 无 GPU 加速渲染

**参考实现 — codex: CJK 处理（非 GPU）**
- `tui/src/markdown_stream.rs:551` — "Emoji (wide), CJK, control char"
- `tui/src/live_wrap.rs:230-236` — CJK 字符和宽度计算测试
- `tui/src/diff_render.rs:951,1410` — CJK 和 tab 处理
- `tui/src/bottom_pane/textarea.rs:2083,3364-3400` — CJK 单词导航

**参考实现 — claude-code-bun: Ink 渲染器中的 CJK**
- `packages/@ant/ink/src/core/wrap-text.ts:8` — 宽字符处理（CJK 边界跨度）
- `packages/@ant/ink/src/core/log-update.ts:316-669` — 宽字符列推进 + 边缘交叉预防
- `packages/@ant/ink/src/core/searchHighlight.ts:15-46` — CJK/emoji 列映射
- `src/services/skillSearch/localSearch.ts:158-164` — CJK bigram 分词
- `src/components/permissions/.../PreviewBox.tsx:87,115` — unicode/emoji/CJK 视觉宽度计算

**实现要点:**
- 两者都处理 CJK 但无 GPU 加速（都是终端应用）

---

### 5.9 CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN（v2.1.132）
**状态：部分实现**
- 存在: `ALLTHECODES_NO_FLICKER` 环境变量控制同步更新
- 缺失: 无 `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN` 环境变量

**参考实现 — claude-code-bun: `src/utils/fullscreen.ts`**
- `isFullscreenEnvEnabled()`（第 112 行）— `CLAUDE_CODE_NO_FLICKER` 控制
- `isMouseTrackingEnabled()`（第 140 行）— `CLAUDE_CODE_DISABLE_MOUSE`

**参考实现 — codex: `protocol/src/config_types.rs:561+`**
- `AltScreenMode` 枚举: `Auto`, `Always`, `Never`
- `config/src/types.rs:682` — `alternate_screen: AltScreenMode` 配置字段
- `tui/src/cli.rs` — `--no-alt-screen` CLI 标志

**实现要点:**
- codex: 三态枚举 + CLI 标志 + 配置
- claude-code-bun: env var 控制
- allthecodes 可添加 `ALLTHECODES_DISABLE_ALTERNATE_SCREEN` env var

---

### 5.10 粘贴 "!command" 进入 bash 模式（v2.1.89）
**状态：缺失**
- 输入处理中无 `!` 前缀检测

**参考实现 — claude-code-bun: `src/components/PromptInput/inputModes.ts`**
- `prependModeCharacterToInput()`, `getModeFromInput()`, `getValueFromInput()`（第 4-23 行）
- `PromptInput.tsx:1379-1385` — `!cmd` 粘贴到空输入 → 自动进入 bash 模式
- `src/utils/processUserInput/processBashCommand.tsx:118-141` — bash 输入 → `<bash-stdout>` / `<bash-stderr>` 包装消息
- 也支持 `/paste !command`

**参考实现 — codex: `tui/src/bottom_pane/chat_composer.rs:1371`**
```rust
if let Some(stripped) = text.strip_prefix('!') {
    self.draft.is_bash_mode = true;
}
```
- `keymap.rs:1777` — `("fixed.shell_command", KeyCode::Char('!'))` 键提示
- `footer.rs:1129` — 状态栏键提示
- `paste_burst.rs:119+` — 粘贴突发检测 + bash 模式集成
- 测试: `chat_composer.rs:10755-10795`

**实现要点:**
- 两者都实现 `!` 前缀检测 → bash 模式
- claude-code-bun: `PromptInput.tsx` 集成
- codex: `chat_composer.rs` 集成 + 完整测试

---

### 5.11 语法高亮 Cedar 策略文件（v2.1.97）
**状态：缺失**
- 支持的语言列表中无 Cedar/`.cedar` 条目

**参考实现 — 两者都未找到**
- codex: "Cedar" 仅作为 `RealtimeVoice::Cedar` 变体存在（`protocol.rs:224`），非策略语言
- claude-code-bun: 使用 highlight.js（`packages/color-diff-napi/src/index.ts`）+ tree-sitter 用于 bash 解析
- 无 Cedar 语法高亮条目

---

### 5.12 Edit 工具对通过 Bash 查看的文件有效（v2.1.89）
**状态：缺失**
- 编辑工具与 Bash 输出之间无连接

**参考实现 — 两者都未找到特定功能**
- claude-code-bun: `MessageSelector.tsx:445` — `"Rewinding does not affect files edited manually or via bash."`
- 编辑工具（FileEditTool）独立于 bash 输出
- codex: `external_editor.rs` 用于组成文本的外部编辑器，非 bash 查看的文件

---

### 5.13 空闲返回时提示 /clear（v2.1.84）
**状态：缺失**
- 无空闲检测或 /clear 提示系统

**参考实现 — 两者都未找到**
- claude-code-bun: `sessionActivity.ts`（空闲日志记录）+ `idleTimeout.ts`（SDK 模式退出），但无 `/clear` 提示
- codex: `streaming/mod.rs` 有 `is_idle()`，但未连接 `/clear`

---

### 5.14 /context 显示技能的插件名称（v2.1.139）
**状态：部分实现**
- 技能可以有 `SkillSource::Plugin(plugin_id)` 来源
- 缺失: `/context` 输出中技能类别未按插件名称细分

**参考实现 — codex: `tui/src/skills_helpers.rs:17-21`**
```rust
if let Some((plugin_name, skill_name)) = skill.name.split_once(':')
    && !plugin_name.is_empty()
{
    return format!("{skill_name} ({plugin_name})");
}
```
- `app_event.rs:460-513` — `plugin_display_name`, `plugin_name` 字段在技能相关事件中

**参考实现 — claude-code-bun: `src/utils/analyzeContext.ts:598`**
```typescript
source: (skill.type === 'prompt' ? skill.source : 'plugin')
```
- `src/utils/plugins/loadPluginCommands.ts:726` — `pluginName:skillName` 命名约定
- `src/commands.ts:660` — `loadedFrom: 'skills' | 'plugin' | 'bundled'`

**实现要点:**
- codex: 格式化 `skill_name (plugin_name)`
- claude-code-bun: `loadedFrom` 追踪来源类型

---

### 5.15 提示历史去重（v2.1.147）
**状态：缺失**
- 无提示历史去重机制

**参考实现 — codex: 多种去重**
- `keymap_setup.rs:479,550-555` — `dedup_bindings()`
- `render/highlight.rs:375` — "Discover custom themes on disk, deduplicating against builtins."
- `chatwidget/protocol.rs:252` — "User-message dedupe only suppresses the app-server echo."
- `core/tests/suite/client.rs:3073` — `history_dedupes_streamed_and_final_messages_across_turns`
- `chatwidget/mcp_startup.rs:238-240` — MCP 启动去重

**参考实现 — claude-code-bun: 多种去重**
- `services/mcp/config.ts` — `dedupPluginMcpServers()`, `dedupClaudeAiMcpServers()`
- `history.ts:158` — "Current-project history for the ctrl+r picker: deduped by display text."
- `services/vcr.ts:250` — "sessionStorage.ts deduplicates messages by UUID."
- `services/api/claude.ts:3253-3274` — `deduplicateEdits()` 用于缓存编辑块
- `bridge/bridgeMessaging.ts:267` — 防御性去重

**实现要点:**
- codex: 用户消息回波去重（应用服务器回波压制）
- claude-code-bun: UUID 级去重 + 缓存编辑去重

---

## 六、权限与安全功能差距

### 6.1 Sandbox 设置（多个版本）

| 功能 | 版本 | codex | claude-code-bun | allthecodes 状态 |
|------|------|-------|------------------|------------------|
| sandbox.bwrapPath/socatPath | v2.1.133 | ✅ **已实现**（`linux-sandbox/src/launcher.rs:19-21` — `BubblewrapLauncher` 枚举，系统/捆绑/bwrap 路径解析） | ❌ 未找到（仅注释） | **缺失** |
| sandbox.failIfUnavailable | v2.1.83 | ❌ 未找到 | ✅ **已实现**（`sandboxTypes.ts:95-103` Zod schema + `sandbox-adapter.ts:483` 读取） | **已实现** |
| sandbox.network.deniedDomains | v2.1.113 | ✅ **已实现**（`network-proxy/src/state.rs:35` + `config.rs:174` 方法链） | ✅ **已实现**（`sandbox-adapter.ts:179,218,362`） | **缺失** |
| 沙箱权限规则 | v2.1.128+ | ✅ 完整系统 | ✅ 完整系统 | 部分实现 |
| autoAllowBashIfSandboxed | v2.1.139 | ❌ 未找到 | ✅ **已实现**（`sandboxTypes.ts:108-112` + `sandbox-adapter.ts:471` 默认 true） | **缺失** |
| rm -rf $HOME 阻止 | v2.1.154 | ✅ Shell 危险模式检测 | ✅ Shell 危险模式检测 | **已实现** |

**codex bwrapPath 参考 — `linux-sandbox/src/launcher.rs:19-21`**
```rust
pub(crate) enum BubblewrapLauncher {
    System(SystemBwrapLauncher),
    Bundled(BundledBwrapLauncher),
    Unavailable,
}
```
- `preferred_bwrap_launcher()`（第 51 行）— 解析系统 vs 捆绑 bwrap
- `system_bwrap_launcher_for_path(path)`（第 69 行）— 从系统路径创建启动器
- `system_bwrap_capabilities(path)`（第 108 行）— 探测能力
- `sandboxing/src/bwrap.rs:45` — `find_system_bwrap_in_path()`
- `BAZEL_BWRAP_ENV_VAR = "CARGO_BIN_EXE_bwrap"`（`bazel_bwrap.rs:8`）

**codex deniedDomains 参考 — `network-proxy/src/state.rs:35`**
- `denied_domains: Option<Vec<String>>` 字段
- 用于过滤和验证（第 70-73, 121-128, 314-352 行）
- 运行时执行: `runtime.rs:306,733,739,798-799`

**claude-code-bun 沙箱整体实现:**
- `sandboxTypes.ts` — 完整 Zod 模式（157 行，覆盖 ~20 个设置）
- `sandbox-adapter.ts` — 适配器层（900+ 行，包装 `@anthropic-ai/sandbox-runtime`）
- `permissions.ts:1207` — 沙箱自动允许逻辑
- `Shell.ts:204-299` — 沙箱 tmp 目录 + shell 配置

---

### 6.2 环境变量支持

| 功能 | 版本 | codex | claude-code-bun | allthecodes 状态 |
|------|------|-------|------------------|------------------|
| SUBPROCESS_ENV_SCRUB | v2.1.83 | ❌ 未找到 | ✅ **已实现**（`subprocessEnv.ts:1-99` — `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` env var，清除敏感 env） | **缺失** |
| SCRIPT_CAPS | v2.1.98 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| PERFORCE_MODE | v2.1.98 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| FORK_SUBAGENT | v2.1.117 | ❌ 未找到 | ✅ **已实现**（`commands.ts:149` — `feature('FORK_SUBAGENT')` 编译时标志） | **缺失** |
| ENABLE_PROMPT_CACHING_1H | v2.1.108 | ✅ 提示缓存作为 API 概念存在（`codex-api/src/common.rs:37` — `prompt_cache_key`），**非 env var** | ✅ **已实现**（`claude.ts:393` — `ENABLE_PROMPT_CACHING_1H_BEDROCK` env var） | 部分实现 (TTL 存在, env var 名称不同) |
| PLUGIN_PREFER_HTTPS | v2.1.141 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| CLAUDE_CODE_SESSION_ID | v2.1.132 | ✅ 会话 ID 存在（`tui/src/cli.rs:30` — `resume_session_id`），**非 env var** | ✅ **已实现**（`state.ts:425` — `getSessionId()`，`Shell.ts:325` — 设置子进程 env） | **缺失** |
| CLAUDE_CODE_EXTRA_BODY | v2.1.113 | ❌ 未找到 | ✅ **已实现**（`claude.ts:275-308` — `getExtraBodyParams()` 解析 JSON 并合并 beta headers） | **缺失** |
| CLAUDE_CODE_USE_MANTLE | v2.1.94 | ❌ 未找到 | ❌ 未找到 | **缺失** |

**claude-code-bun SUBPROCESS_ENV_SCRUB 参考: `src/utils/subprocessEnv.ts:1-99`**
- 门控: `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` env var（第 60, 86 行）
- 清除: ANTHROPIC_API_KEY, OTLP headers, AWS/Azure/GCP 凭证, GitHub Actions tokens

**claude-code-bun CLAUDE_CODE_EXTRA_BODY 参考: `src/services/api/claude.ts:275-308`**
- 解析 `CLAUDE_CODE_EXTRA_BODY` JSON 并合并到 API 请求中
- 与提示缓存断检测集成（`promptCacheBreakDetection.ts:59-423` — `extraBodyHash` 和 `extraBodyChanged`）

---

### 6.3 CLI 标志

| 标志 | 版本 | codex | claude-code-bun | allthecodes 状态 |
|------|------|-------|------------------|------------------|
| --bare | v2.1.81 | ❌ 未找到 | ✅ **已实现**（`envUtils.ts:50-65` — `isBareMode()` 检查 `CLAUDE_CODE_SIMPLE` env var 或 `--bare` argv），~30+ 门控 | **缺失** |
| --channels 权限转发 | v2.1.81 | ❌ 未找到 | ✅ **已实现**（`channelNotification.ts:15-236` — `--channels` 会话解析 + `gateChannelServer()`） | **缺失** |
| --dangerously-skip-permissions | v2.1.126 | ✅ **不同名称**: `--dangerously-bypass-approvals-and-sandbox`（`cli/src/main.rs:2621`）+ `--dangerously-bypass-hook-trust`（第 2794 行） | ✅ **已实现**（`permissionSetup.ts:691-725` — `initialPermissionModeFromCLI()`） | **缺失** |

---

### 6.4 Bash 工具权限

| 功能 | 版本 | codex | claude-code-bun | allthecodes 状态 |
|------|------|-------|------------------|------------------|
| env 变量前缀自动批准 | v2.1.97 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| Find vnode 耗尽修复 | v2.1.149 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| PowerShell 自动批准 | v2.1.119 | ✅ `disable_powershell_profile_for_elevated_windows_sandbox()`（`core/src/tools/runtimes/mod.rs:113`） | ✅ `powershellProvider.ts`（完整文件）+ `tools.ts:164-268`（`getPowerShellTool()`） | 部分实现 |
| DENY 规则处理复合命令 | v2.1.97+ | ❌ 未找到 | ✅ `powershell/parser.ts:605` — 将 deny 降级为 ask 用于复合命令 | 部分实现 |

---

### 6.5 工作树设置

| 设置 | 版本 | codex | claude-code-bun | allthecodes 状态 |
|------|------|-------|------------------|------------------|
| worktree.baseRef | v2.1.133 | ❌ 未找到 | ❌ 未找到（工作树配置存在: `types.ts:438-457` — `symlinkDirectories`, `sparsePaths`，但无 `baseRef`） | **缺失** |
| worktree.bgIsolation | v2.1.143 | ❌ 未找到 | ❌ 未找到 | **缺失** |

---

### 6.6 企业/托管设置

| 设置 | 版本 | codex | claude-code-bun | allthecodes 状态 |
|------|------|-------|------------------|------------------|
| ParentSettingsBehavior admin-tier | v2.1.133 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| allowAllClaudeAiMcps | v2.1.149 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| forceRemoteSettingsRefresh | v2.1.92 | ✅ `forceRemoteSync` 存在于测试 fixture（`protocol/v2/tests.rs:2834-3307`） | ❌ 未找到 | **缺失** |
| Managed-settings.d/ 碎片目录 | v2.1.83 | ❌ 未找到 | ✅ **完整服务**: `services/remoteManagedSettings/`（`index.ts:1-639` — 获取/缓存/轮询/安全检查）、`syncCache.ts`, `securityCheck.tsx` | **缺失** |
| SkillOverrides 设置 | v2.1.129 | ❌ 未找到 | ❌ 未找到 | **缺失** |
| prUrlTemplate | v2.1.119 | ❌ 未找到 | ❌ 未找到（`prUrl` 字段存在于 `types/logs.ts:48,132`，但非 template） | **缺失** |

**claude-code-bun remoteManagedSettings 参考: `src/services/remoteManagedSettings/index.ts`**
- 完整企业设置分发服务（639 行）
- 功能: checksum-based HTTP 缓存 (ETag), SHA-256 校验和, OAuth + API 密钥认证, 每小时轮询
- 安全检查: 危险设置变更检测
- 失败处理: 获取失败时继续使用现有设置（fail-open）

---

## 七、已实现功能确认

以下功能已检查并确认在 allthecodes 中实现：

- ✅ `/team-onboarding` 命令
- ✅ `/recap` 会话回顾
- ✅ 焦点视图 (Ctrl+O)
- ✅ GFM 任务列表复选框渲染
- ✅ Markdown 块引用连续左侧条
- ✅ 转录模式搜索 (Ctrl+O → /)
- ✅ NO_FLICKER 模式 (`ALLTHECODES_NO_FLICKER`)
- ✅ `/config` 菜单搜索
- ✅ 流式工具执行
- ✅ 状态行接收 COLUMNS/LINES
- ✅ 智能体自动完成斜杠命令
- ✅ `/mcp` 显示工具计数和 0 工具警告
- ✅ Claude in Chrome / 浏览器选择
- ✅ 紧凑行号格式
- ✅ Token 计数显示为 1.5M
- ✅ 推送通知工具
- ✅ Monitor 工具
- ✅ Shell 危险模式检测 (rm -rf 阻止)
- ✅ PermissionDenied hook
- ✅ PreCompact hook
- ✅ disallowed-tools 前端元数据
- ✅ Hook 条件 if 字段
- ✅ Elicitation / ElicitationResult hooks
- ✅ 自动(匹配终端) 主题选项
- ✅ Verbose 模式持久化
- ✅ 插件依赖解析器
- ✅ 自动模式分类器（基础版）
- ✅ 提示缓存 TTL=1h 支持

---

## 八、建议优先级路线

### P0 — 核心功能差距
1. **Workflow 系统** — 动态多智能体编排引擎
2. **/goal 命令** — 跨轮次目标追踪（工具存在，命令缺失）
   - 参考: codex `SlashCommand::Goal` + `ThreadGoal` + `GoalStatusIndicator`

### P1 — 重要用户体验
3. **/feedback 命令** — 用户反馈渠道
   - 参考: codex `SlashCommand::Feedback` + `feedback` crate（日志捕获 → 分类 → 附件 → 上传）
   - 参考: claude-code-bun `src/commands/feedback/`（local-jsx 命令 + React 组件）
4. **/usage 统一** — 合并 /cost + /extra-usage
   - 参考: claude-code-bun `src/commands/usage/`（已统一，别名 `cost`, `stats`）
5. **/undo → /rewind 别名** — 简单别名注册
   - 参考: claude-code-bun `/rewind` 已有 `checkpoint` 别名
6. **/proactive → /loop 别名** — 简单别名注册
   - 参考: claude-code-bun `/proactive` 独立于 `/loop`，功能不同
7. **/scroll-speed 命令** — 将 env 设置包装为命令
   - 参考: claude-code-bun `CLAUDE_CODE_SCROLL_SPEED` env var（可包装为命令）
8. **无闪烁 /tui fullscreen** — 渲染模式切换
   - 参考: codex `AltScreenMode::Auto/Always/Never` + CLI `--no-alt-screen` 标志
   - 参考: claude-code-bun `CLAUDE_CODE_NO_FLICKER` env var
9. **Read 工具 PARTIAL 视图** — 截断提示
   - 参考: claude-code-bun `isPartialView` 文件状态缓存概念

### P2 — 生态完善
10. **/reload-skills 命令** — 添加命令或别名
    - 参考: claude-code-bun `/reload-plugins` + codex `ListSkills(force_reload)`
11. **/code-review --fix** — 为 /review 添加 --fix
    - 参考: claude-code-bun `/simplify` bundled skill（AgentTool 并行化修复）
12. **claude agents --json** — JSON 输出选项
13. **Plugin defaultEnabled** — 插件配置字段
    - 参考: claude-code-bun `BuiltinPluginDefinition.defaultEnabled?: boolean`
    - 参考: codex `FeatureSpec.default_enabled: bool`
14. **Plugin 依赖强制传播** — enable/disable 依赖链
    - 参考: codex `normalize_dependencies()` 用于功能扇出
15. **Hook args: string[]** — 执行形式支持
    - 参考: codex `CommandShell { program, args }` 结构体
16. **autoScrollEnabled** — 全屏滚动配置
17. **/model 保存默认值** — 模型选择器持久化
    - 参考: codex `PersistModelSelection` 事件 + 配置锁
18. **SessionStart reloadSkills** — hook 配置参数
19. **MessageDisplay hook** — 新 hook 事件变体

### P3 — 锦上添花
20. **/less-permission-prompts skill** — 技能定义
21. **/powerup 交互式教学** — 功能教程
22. **/buddy 愚人节彩蛋** — 彩蛋功能
    - 参考: claude-code-bun 完整 `src/buddy/` 系统（18 物种 + 反应 + 闪光）
    - 参考: codex `src/pets/` 系统（7 宠物 + Kitty/Sixel 协议）
23. **/color 无参随机** — 命令实现
    - 参考: claude-code-bun `/color` 命令（提示栏颜色选择器）
24. **/release-notes** — 版本选择器
    - 参考: claude-code-bun `src/commands/release-notes/`（获取 + 缓存 + 格式化）
25. **Cedar 语法高亮** — 语言别名条目
26. **Sandbox bwrapPath/socatPath** — 设置字段
    - 参考: codex `BubblewrapLauncher` 枚举（系统/捆绑/不可用）
27. **Sandbox deniedDomains** — 网络策略字段
    - 参考: codex `network-proxy/src/state.rs` denied_domains 字段
    - 参考: claude-code-bun `sandbox-adapter.ts` deniedDomains
28. **工作树 baseRef / bgIsolation** — 设置
29. **空闲返回 /clear 提示** — 通知
30. **企业远程管理设置** — 托管设置分发
    - 参考: claude-code-bun `services/remoteManagedSettings/index.ts`（1-639 行，完整企业功能）

---

## 九、在 codex 和 claude-code-bun 中发现的额外功能

以下功能在两个参考代码库中发现但未在差距分析中覆盖：

### codex 特有功能（OpenAI Codex）
- **宠物系统**: `/pets` 命令 + 7 个内置宠物 + Kitty/Sixel 图像协议支持
- **审查系统**: Guardian 审批 + 自动审查拒绝（`GuardianApproval` 功能标志）
- **诊断反馈**: `feedback` crate 含日志环回缓冲区捕获 + 诊断收集 + GitHub/Slack 跟进链接
- **`.tmTheme` 自定义主题**: `{CODEX_HOME}/themes/` 目录加载
- **动态工具保留命名空间**: 验证动态工具名称不冲突

### claude-code-bun 特有功能（Claude Code Bun）
- **完整插件市场系统**: 浏览/添加/删除/发现市场 + 官方 Anthropic 市场
- **`/ultraplan` + `/ultrareview`**: 远程 CCR 会话 + GrowthBook 门控 + Extra Usage 计费
- **`/buddy` 彩蛋**: 18 物种 + 5 稀有度 + 反应系统 + 闪光变体 + 预告
- **远程托管设置**: 企业设置分发（ETag 缓存 + SHA-256 + 每小时轮询 + 安全检查）
- **Slack 紧凑头部**: 检测 Slack 消息结果并渲染紧凑 `{channel, url}` 对
- **`alwaysLoad` MCP 元数据**: MCP 服务器控制工具加载优先级
- **流式工具执行器**: `StreamingToolExecutor` 并发工具执行 + Statsig 门控
- **通道系统**: `--channels` 通过 Telegram/iMessage/Discord 的权限转发

### 两者共同功能
- `defaultEnabled` 插件/功能配置（codex: `FeatureSpec`; claude-code-bun: `BuiltinPluginDefinition`）
- 插件市场系统（两者都有完整实现）
- CJK 文本渲染支持（两者都有，使用不同的 CJK 处理库）
- `!` 前缀 bash 模式（两者都有，相似的 `starts_with('!')` 检测）

---

## 附录：审计方法

本差距分析使用八个专门的子代理进行（2026-06-01 更新）：

1. **codex 命令审计** — 搜索 `codex-rs/` 和 `codex-cli/` 中的所有命令相关功能
2. **claude-code-bun 命令审计** — 搜索 `claude-code-bun/src/commands/` 中的所有命令
3. **codex 插件/Hook 审计** — 搜索 codex 中的插件和 hook 系统
4. **claude-code-bun 插件/Hook/MCP 审计** — 搜索 claude-code-bun 中的插件/hook/MCP 系统
5. **codex MCP/UI 审计** — 搜索 codex 中的 MCP、UI 和显示功能
6. **claude-code-bun UI 审计** — 搜索 claude-code-bun 中的 UI 和显示功能
7. **codex 安全审计** — 搜索 codex 中的权限和安全功能
8. **claude-code-bun 安全审计** — 搜索 claude-code-bun 中的权限和安全功能

每个功能对照实际的源代码验证，包括：
- grep 搜索实现/功能名称（跨 Rust `.rs` 和 TypeScript `.ts/.tsx` 文件）
- 检查注册命令、枚举变体、配置结构
- 读取关键源文件以确认实现状态和具体方法
- 交叉引用跨两个参考代码库的实现差异

> 本文件将作为跟踪 allthecodes vs Claude Code 功能差距的活文档。
> 下次更新建议在 2026-06-15 前后。
> 参考代码库: `codex/` (OpenAI) 和 `claude-code-bun/` (Anthropic Claude Code Bun)
