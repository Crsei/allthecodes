# Rust TUI 大粘贴、命令高亮、会话 WARN、状态行与底部布局修复计划

日期：2026-07-18  
状态：审阅修复已完成；完整 PTY 保留 2 个独立既有等待窗口失败（2026-07-19）
范围：仅 Rust TUI（`crates/allthecodes/src/ui/`）及为 TUI 提供结构化数据所必需的 startup / engine / shared DTO 边界

## 1. 背景与目标

本计划统一处理以下六个用户可见问题：

1. 粘贴超长文本时，输入框没有显示紧凑的 `[Pasted Content n chars]` 引用。
2. 输入以 `/` 开头的有效命令时，命令文本没有高亮。
3. 启动阶段出现的用户可操作 `WARN` 只写入 stderr / tracing，进入 alternate screen 后没有出现在 session 会话框。
4. 模型名称同时出现在 context layer 和默认 footer，输入框下方重复显示。
5. 任务运行时，底部缺少统一的 `model / agent / context 占用百分比` 状态。
6. 短会话或任务刚启动后，输入框跟随内容停在终端中上部，而不是固定在终端底部。

目标是建立五个稳定契约：结构化 prompt 内容、命令语义高亮、结构化启动诊断、单一底部状态所有者、bottom-anchored 主布局。不能只改显示字符串或移动一行坐标。

## 2. 非目标

- 不把任意 tracing 日志或所有 `WARN` 无差别复制到会话历史。
- 不把大粘贴内容丢弃、截断后提交，或只保留可见占位符。
- 不高亮未知命令、普通路径（如 `/usr/bin`）或无法执行的动态命令。
- 不以全会话累计 token 近似当前上下文窗口占用。
- 不删除 scriptable `statusLine.command`、通知行、多 agent footer、命令面板或已有 overlay。
- 不在本计划中重新实施已经完成的 mouse selection、IME caret、多行输入和 UTF-8 压缩修复；本计划以
  `development/tui/2026-07-18-tui-mouse-selection-ime-preedit-fix-plan.md` 的当前实现为基线。

## 3. 当前代码证据

### 3.1 大粘贴只有独立 notice，没有内联引用模型

`crates/allthecodes/src/ui/prompt_input.rs::paste_text()` 当前把规范化后的完整文本直接插入 `input`。字符数不少于 512
或行数不少于 4 时，只额外保存：

```text
Pasted {char_count} chars across {line_count} lines; the full text remains editable.
```

`crates/allthecodes/src/ui/app/render.rs::render_paste_notice()` 再在输入框上方占一整行渲染 `paste ...`。这不是用户期望的
内联 `[Pasted Content n chars]`，也没有建立“可见引用 -> 完整原文”的结构化映射。

上游完整版使用可展开的 pasted-content 引用，并在执行即时命令前展开引用。Rust 端应对齐这种“显示紧凑、提交完整”
的行为边界，但格式按本任务固定为 `[Pasted Content n chars]`。

### 3.2 PromptInput 统一样式渲染，命令注册表没有进入渲染上下文

`PromptInput::render_with_context()` 当前把每条可视行的输入文本整体渲染为 `Style::default()`。命令面板通过
`allthecodes_commands::commands::get_all_commands()` 获取 built-in / dynamic commands，但有效命令范围没有传给
PromptInput。

上游 `PromptInput.tsx` 先用 `findSlashCommandPositions()` 找出词法范围，再通过 `hasCommand()` 只保留真实注册命令。
Rust 端也必须复用 canonical command registry / resolver，不能维护第二份命令名称列表。

### 3.3 启动 WARN 发生在 TUI 建立之前

模型回退、未配置认证、plugin reconciliation、MCP discovery / connect 和 dashboard companion 等警告发生在
`startup::*` 中。当前 `RuntimeReady` 不携带诊断，`mode_router.rs` 调用 `run_tui(engine, initial_prompt, model, token)`
时也没有 warning 参数。`run_tui()` 进入 alternate screen 后才创建 `App`，因此此前的 stderr / tracing 内容不会进入
`Message::System`。

运行期 subsystem warning 已有 `InfoLevel::Warning` -> `Message::System` 的通道，本任务应复用其会话渲染语义，
但启动期需要独立、显式的数据通道。

### 3.4 model 与 context 的所有权重复，context 数值口径错误

`App::refresh_context_layer()` 当前写入 `repo`、`model`、`agent` 和 `ctx`；`render_status_bar()` 又把
`session_ui.model_name` 添加到默认 footer，导致 model 重复。

当前 `context_usage_summary()` 把全会话累计的 input、output、cache-read、cache-creation 相加后显示为 `{n}t`。
这些计数会跨 API call 单调累加，不等于当前模型请求占用的上下文，也没有有效 context-window denominator，不能据此
计算百分比。

配置层已有 `ModelCapabilitySettings.context_window`、`max_context_window` 和
`effective_context_window_percent`；compact pipeline 也已有请求级 token estimate / exact count 边界，但 TUI 当前没有收到
“当前请求 used / effective capacity”快照。

### 3.5 主布局把弹性空白放在输入框之后

`App::render()` 当前按以下顺序分割纵向空间：

```text
Length(content_height)
Length(message_bottom_gap_height)
Length(bottom_height)
Min(0)
```

短内容时，剩余空间落在 bottom pane 之后，输入框因此紧跟 welcome / conversation 内容。目标布局应把弹性空间放在
message content 与 bottom pane 之间，使 prompt、context、agent footer 和 status 作为一个整体固定到底部。

当前 `app/tests.rs` 中 `render_places_prompt_after_compact_welcome` 和
`render_places_prompt_after_short_chat_content` 明确断言 prompt 不贴底；实施时必须有意更新这些旧行为测试，不能让它们
继续固定错误契约。

## 4. 目标行为契约

### 4.1 大粘贴引用

1. 达到阈值的 paste 在输入框中显示为一个原子引用：`[Pasted Content {char_count} chars]`。
2. 阈值沿用当前首版值（`chars >= 512 || lines >= 4`），但提取为命名策略并用 boundary tests 固定；后续调整不能散落在渲染代码中。
3. 提交给命令处理器、engine 和 session history 的值必须是完整原文，不是占位符字符串。
4. 引用必须是结构化 segment / range，不允许依靠扫描显示字符串反向恢复内容；用户手动输入同样的方括号文本不能被误展开。
5. 光标左右移动把引用视为一个原子单位；紧邻引用的 Backspace / Delete 删除整个引用。若选择“展开后编辑”，必须显式转换为普通文本并移除旧映射，不能产生悬空 payload。
6. 多次 paste 使用稳定的内部 id，删除一个引用不影响其它引用；复制、undo/history、队列消息和外部编辑器路径不得串错 payload。
7. 外部编辑器、持久化 history 和最终消息保存 expanded 原文；同一草稿若保留紧凑显示，必须同时保存结构化 metadata。任何时候都不能持久化一个无法恢复的裸引用。
8. 小 paste 和少量多行文本继续按普通可编辑文本显示。

建议的数据边界（最终命名可调整）：

```text
PromptDocument
  segments: [Text(...), LargePaste { id, original, char_count, line_count }, ...]
  display_projection(): text + source map
  expanded_text(): exact submission value
```

如果为了兼容现有 completion / vim / byte cursor 暂时保留 `String input`，也必须增加不可伪造的
`LargePasteRange` 集合和双向 source map；每次 insert / delete / history restore 后验证 range 有序、不重叠且位于合法 UTF-8
边界。不得只把 `large_paste_notice: Option<String>` 改成另一个字符串。

### 4.2 slash command 高亮

1. 只高亮 canonical registry 中当前可执行的 command name 或 alias；未知 `/foo` 保持普通输入样式。
2. 首字符 `/command` 必须识别；行内命令仅在 slash 前为行首或空白时识别，`/usr/bin`、URL 和普通文件路径不得误判。
3. 命令范围在空白、换行或参数开始处结束，参数继续使用普通文本样式。
4. built-in、project、user、plugin、skill 动态命令与命令面板使用同一注册表快照和可见性规则。
5. 高亮 range 使用 UTF-8 byte boundary，并与 `PromptInputVisualLine.start..end` 求交后生成 spans；CJK 参数、软折行、硬换行和水平/垂直 viewport 不得破坏字符。
6. 使用现有 theme 的 suggestion / info accent（最终颜色由 theme token 决定），不在 PromptInput 中硬编码新的 RGB 命令色。
7. command palette 是否打开不影响已输入有效命令的语义高亮。

### 4.3 WARN 进入 session 会话框

新增结构化启动诊断 DTO，例如：

```text
StartupDiagnostic {
  stable_id,
  severity: Warning | Error,
  source: Model | Auth | Plugin | Mcp | Dashboard | History | Other,
  summary,
  detail,
  user_action,
}
```

契约如下：

1. 只有显式标记为 user-visible 的启动诊断进入会话；debug 噪声、瞬态内部重试和高频相同错误不进入。
2. 原有 tracing / stderr 仍保留，结构化会话消息是额外通道，不替代日志。
3. `RuntimeComposition` 聚合启动诊断，`RuntimeReady` 携带诊断，`ModeRouter` 在调用 `run_tui()` 时传入；dashboard 等在
   composition 完成后、TUI 前发生的警告也追加到同一集合。
4. `run_tui()` 创建 App 后、处理 initial prompt 前，把诊断转换为
   `Message::System(SystemSubtype::Informational { level: Warning })`，使其出现在 session 会话框并参与 transcript/export。
5. 同一 `stable_id + normalized detail` 在一次启动中只显示一次；多服务器失败可聚合成一条摘要和受限 detail。
6. token、Authorization header、URL userinfo、环境变量值、绝对凭据路径和任意 secret 必须在进入 DTO 前脱敏。
7. 非 TUI 模式继续只使用日志/对应机器协议，不能把 TUI system message 注入 `--print`、JSON、headless 或 ACP 输出。
8. TUI 建立后发生的 warning 继续走已有 AppEvent / subsystem event 通道，不再回写 startup buffer。

首批应覆盖：startup model fallback / unavailable requested model、认证缺失、MCP discovery/connect partial failure、plugin
reconciliation failure、dashboard companion failure。对每个现有 `warn!` 逐项分类，而不是通过 tracing subscriber 全局抓取。

### 4.4 底部状态的单一所有者

以 context layer 的一行 compact status 作为 `model / agent / context` 的唯一 prompt-adjacent 所有者，默认 footer 不再重复
追加 model。多 agent footer 只展示 worker 列表和状态，不再承担主 agent / model / context 摘要。

宽度充足且任务运行时的标准格式：

```text
model: gpt-5.6-sol | agent: Primary running | context: 16.4k/272k (6%)
```

规则：

1. model 只出现一次；显示 effective model，而不是失效的用户请求别名。
2. agent 使用当前 thread 的 nickname / role 与真实 runtime status。主线程没有显式昵称时显示 `Primary`；不能把“有 worker”误当成当前 agent。
3. context numerator 来自最新/当前模型请求的上下文快照，不能使用 `UsageTracking` 的全会话累计总和。
4. denominator 优先使用 effective settings 对当前 model 的 context capacity，并应用
   `effective_context_window_percent`；必要时才回退 shared model capability。未知 capacity 显示 `context: unknown`，不得猜测百分比。
5. 百分比使用 `used / effective_capacity`，四舍五入规则统一并 clamp 可视进度到 100%；若原始 used 超限，应仍显示
   `>100%` 或 warning tone，不能静默伪装成 100%。
6. exact provider count 可用时标记为 exact；否则使用 compact pipeline 与实际 request 相同消息集合的 estimate。不能在 TUI
   独立重新估算另一份消息。
7. compact 成功后快照下降；model 切换后立即切换 denominator，并在新快照到达前显示 `context: calculating` / `unknown`，
   不能沿用旧模型百分比。
8. streaming 中展示最近可用快照；任务结束后保留最后一次值。完全没有请求时可只显示 model / agent。
9. 窄终端按 `context -> agent status detail -> model display decoration` 的顺序降级，但 model id、agent 和百分比三者不能在
   有空间时重复或互相覆盖。截断必须按 Unicode display width。
10. 自定义 `statusLine.command` 继续拥有其当前区域；本任务不擅自拼接 built-in compact status 到用户脚本输出。

建议新增共享事件：

```text
ContextWindowSnapshot {
  model_id,
  used_tokens,
  effective_capacity,
  source: Exact | Estimated,
  phase: Preparing | Streaming | Completed | Compacted,
}
```

快照应在 engine 已确定实际 request message set 后产生，通过现有 SDK / EngineEvent 通道送达 App。若这会扩展公共 SDK DTO，
必须同步 adapter、headless/web 序列化兼容测试；也可以先新增 TUI 内部 callback，但不得复制 context 计算算法。

### 4.5 输入框固定在终端底部

主布局目标顺序：

```text
Length(content_height)
Min(0)                         # flexible spacer / unused message capacity
Length(message_bottom_gap)
Length(bottom_pane_height)     # prompt + notices + context + agent footer + status
```

契约如下：

1. welcome、短会话、运行中任务和任务完成后，bottom pane 的底边都与 frame 底边一致。
2. 输入框不是必须位于最后一物理行；输入框下方的 notification / context / agent footer / status 仍属于 bottom pane，但整个
   bottom pane 必须贴底。
3. 长会话继续占满可用 message area 并可滚动，新增 spacer 不得改变 `max_scroll` 或吞掉最后一条消息。
4. 多行输入动态增高时向上扩展，不把状态行推到 frame 外；达到 `MAX_VISIBLE_INPUT_LINES` 后在 prompt 内滚动。
5. command surface、completion、permission/question overlay 的底边继续位于 `render_layout.prompt_area` 上方，不被贴底改动遮挡。
6. welcome、workspace trust gate、transcript/focus mode 保留各自专用布局；只修改 normal prompt-mode layout。
7. 终端高度不足时按明确定义的优先级压缩：弹性 spacer -> message body -> optional hints / agent overflow；prompt 和当前可操作
   overlay 不得重叠，也不能出现 `u16` underflow。

## 5. 分阶段实施

### 阶段 A：先建立失败测试与真实 PTY 证据

- [ ] 为大 paste 建立 PromptDocument / source-map 单元测试，先证明当前实现只有 notice、没有内联引用。
- [ ] 为 `/model arg`、alias、未知 `/foo`、`/usr/bin`、CJK 参数和软折行建立 span/style 测试。
- [ ] 为 startup diagnostics 聚合、去重、脱敏、TUI-only 注入建立纯单元测试。
- [ ] 建立 context 快照口径测试：两次 API call 后不得显示累计和；model switch / compact 后必须更新。
- [ ] 更新两个“prompt should not be pinned”旧测试为坐标契约，并增加 80x24、120x40、短高度和多行 prompt 用例。
- [ ] 增加 PTY 基线，记录当前 model 重复、prompt y 坐标和 warning 不可见；断言必须检查真实文本及坐标，不能只检查“不 panic”。

### 阶段 B：实现结构化大粘贴引用

- [ ] 提取阈值策略与 `LargePaste` payload，替换单值 `large_paste_notice`。
- [ ] 建立 display projection / expanded submission 双视图和 UTF-8-safe source map。
- [ ] 让 render、caret、delete、history、queue、external editor、command dispatch 都通过统一 PromptDocument API。
- [ ] 删除独立 `render_paste_notice()` 行及 `BottomPaneHeights.paste_notice`，避免引用显示后仍重复占一行。
- [ ] 增加多 paste、删除、跨引用移动、碰撞文本、CJK/emoji 和持久化 round-trip 测试。

### 阶段 C：实现 slash command 语义高亮

- [ ] 从 canonical command registry 生成当前可执行 command / alias 视图。
- [ ] 新增无副作用 range finder，并在 registry 版本或动态命令变化时刷新，避免每帧重复昂贵发现。
- [ ] 扩展 `PromptInputRenderContext` 接收 typed highlight ranges；按 visual line 切分 styled spans。
- [ ] 使用 theme token，覆盖 command palette 打开/关闭、多行、viewport 和未知命令回归。

### 阶段 D：建立结构化 startup diagnostic 通道

- [ ] 新增 DTO、redaction、dedup/aggregation helper。
- [ ] 各 startup builder 显式返回或写入 diagnostic sink；保留原 tracing。
- [ ] 在 `RuntimeReady` / `ModeRouter` / `run_tui` 边界传递诊断。
- [ ] App 初始化后按 Warning system message 注入 session，并固定 initial prompt 与 warning 的顺序。
- [ ] 审核 print/json/headless/ACP 不受影响，并测试秘密字段不会进入 snapshot/transcript。

### 阶段 E：收口底部状态与 context 百分比

- [ ] 设计并产出 request-level `ContextWindowSnapshot`，接通 exact / estimated 数据来源。
- [ ] 建立 effective model capacity resolver，覆盖 profile capability、百分比、未知模型与 model switch。
- [ ] App 单独保存 `SessionUsageSnapshot`（成本/统计）和 `ContextWindowSnapshot`（窗口占用），禁止混用。
- [ ] context layer 统一渲染 model / current agent / context；从默认 footer 删除 model。
- [ ] 定义 streaming、idle、compact、error、agent switch 与窄宽度降级行为。
- [ ] 保留 multi-agent footer，但消除主 agent 摘要重复。

### 阶段 F：实现 bottom-anchored layout

- [ ] 把 `Constraint::Min(0)` 移到 content 与 bottom pane 之间。
- [ ] 重新核对 message area、scrollbar、prompt area、cursor position 和所有 prompt-adjacent overlay 坐标。
- [ ] 更新 welcome / short chat 的旧断言和 snapshots。
- [ ] 覆盖内容从短到长、多行 prompt、notification、agent footer、command surface 与 resize。

### 阶段 G：集成与文档收口

- [ ] 将用户可见问题追加到 `docs/KNOWN_ISSUES.md` 或当前权威归档入口，并在修复后标记验证证据。
- [ ] 更新 PTY README / fixtures 中关于 status row、model 和 prompt 坐标的过时说明。
- [ ] 生成本任务要求的 worktree HTML artifact，记录截图/PTY 坐标、commit 与验证结果。
- [ ] 只在所有阶段完成后执行 release build；不能用 release 编译成功替代行为测试。

## 6. 预计修改路径

核心 TUI：

- `crates/allthecodes/src/ui/prompt_input.rs`
- `crates/allthecodes/src/ui/app/input.rs`
- `crates/allthecodes/src/ui/app/render.rs`
- `crates/allthecodes/src/ui/app/status.rs`
- `crates/allthecodes/src/ui/app.rs`
- `crates/allthecodes/src/ui/context_layer.rs`
- `crates/allthecodes/src/ui/components/bottom_pane.rs`
- `crates/allthecodes/src/ui/command_palette/*`
- `crates/allthecodes/src/ui/tui.rs`
- `crates/allthecodes/src/ui/tui/engine_events.rs`
- `crates/allthecodes/src/ui/tui/subsystem_events.rs`

启动诊断边界：

- `crates/allthecodes/src/startup/runtime_composition.rs`
- `crates/allthecodes/src/startup/mode_router.rs`
- `crates/allthecodes/src/startup/model_runtime.rs`
- `crates/allthecodes/src/startup/mcp_runtime.rs`
- `crates/allthecodes/src/startup/plugin_runtime.rs`
- 其它经逐项分类后确认为 user-visible 的 startup builder

上下文数据边界（以最小公共 API 改动为准）：

- `crates/allthecodes-engine/src/lifecycle/deps/model_call.rs`
- `crates/allthecodes-engine/src/lifecycle/deps/autocompact.rs`
- `crates/allthecodes-types/src/sdk.rs` 或等价 TUI 内部事件 DTO
- 受公共 DTO 影响的 adapter / protocol tests

测试：

- `crates/allthecodes/src/ui/app/tests.rs`
- `crates/allthecodes/src/ui/prompt_input.rs` 内单元测试或拆出的 prompt document tests
- `crates/allthecodes/src/ui/command_palette/tests.rs`
- `crates/allthecodes/src/ui/tui/tests.rs`
- `crates/allthecodes/tests/pty_tui_e2e/conversation.rs`
- `crates/allthecodes/tests/pty_tui_e2e/status.rs`
- `crates/allthecodes/tests/pty_tui_e2e/welcome.rs`
- 必要时新增 `paste_input.rs` / `startup_warning.rs`，并在 PTY `main.rs` 注册

## 7. 测试矩阵

| 类别 | 必测场景 | 关键断言 |
| --- | --- | --- |
| 大 paste | 511/512 chars、3/4 lines、多次 paste、CJK/emoji | 阈值准确；显示引用；提交全文逐字相等 |
| 引用编辑 | 左右移动、Backspace/Delete、普通文本碰撞、history、queue、editor | 无悬空 id；不丢内容；不误展开用户文本 |
| command | `/model`、alias、dynamic command、`/foo`、`/usr/bin`、CJK 参数 | 仅有效 command name 范围有 accent style |
| startup WARN | model fallback、auth、MCP、plugin、重复错误、含 secret detail | session 可见；日志保留；去重；脱敏 |
| model/status | idle、streaming、agent switch、worker、custom statusline | model 只出现一次；agent 对应当前 thread |
| context | 首次请求、连续请求、cache、compact、model switch、未知窗口 | 非累计；百分比正确；compact 后下降；未知不猜测 |
| layout | 80x24、120x40、极短终端、welcome、短/长会话、多行输入 | bottom pane 贴底；消息可滚动；无 overlay 遮挡 |
| PTY | 启动、输入命令、paste、运行任务、WARN fixture、resize | 检查屏幕文本、样式可见替代证据和 prompt/status 坐标 |

说明：普通 PTY screen text 通常不能可靠读取颜色。命令颜色由 `TestBackend` cell style 精确断言，PTY 负责确认文本范围、
布局和真实终端输出未被 ANSI 切坏。

## 8. 分层验证顺序

遵循仓库 SOP，全部代码应用完成后再统一测试，不在每个小改动后反复跑完整 PTY：

1. `cargo fmt --all --check`
2. 相关单元测试：PromptDocument、PromptInput、command range、startup diagnostics、context snapshot、App layout
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace --exclude allthecodes --lib`
5. 相关 PTY test filters；首次 snapshot 变化一次性 batch 更新并人工审阅
6. `cargo test -p allthecodes --test pty_tui_e2e -- --test-threads=1`
7. `git diff --check`
8. `cargo build --workspace --release`

所有 cargo 命令使用仓库 `AGENTS.md` 指定的 `CARGO_HOME`、`RUSTUP_HOME`、`PATH` 和独立 worktree
`CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-<task-slug>`。

## 9. 验收标准

- [ ] 超长 paste 在输入框内显示 `[Pasted Content n chars]`，提交、queue、history 和 editor 获得完整原文。
- [ ] 只有已注册且当前可执行的 `/command` / alias 高亮，未知命令与路径不高亮。
- [ ] 明确分类的启动 WARN 同时保留日志并以 Warning system message 出现在 session 会话框。
- [ ] warning 内容经过脱敏和去重，非 TUI 输出协议不受污染。
- [ ] normal prompt-mode 中 model 只显示一次。
- [ ] 任务运行时显示 effective model、当前 agent/status、真实 request-level context used/capacity 与百分比。
- [ ] 连续请求不会把全会话累计 token 当作当前上下文；compact 与 model switch 后数值正确更新。
- [ ] welcome、短会话、任务运行/完成后 bottom pane 固定在终端底部。
- [ ] 80x24、120x40、短高度、长会话、多行输入下消息、prompt、overlay 和状态行不重叠。
- [ ] TestBackend 精确样式/坐标断言、目标 PTY、完整 PTY、clippy 和 workspace release build 全部 exit 0，且无新增 warning。

## 10. 实施工作流与提交拆分

按 `development/workflow/2026-07-16-per-session-worktree-workflow-plan.md` 执行：

1. 本计划文件先在主分支单独提交；不得把当前工作树中其它未提交的 welcome / render / snapshot 修改混入该计划提交。
2. 从包含计划 commit 的主分支 HEAD 创建：
   `worktree/tui-paste-command-warning-status-bottom-layout`。
3. 所有产品代码、测试、KNOWN_ISSUES 更新和 artifact 只在独立 worktree 中完成。
4. artifact 路径：
   `development/worktree-workflow-artifacts/2026-07-18-tui-paste-command-warning-status-bottom-layout.html`。
5. 建议按以下边界提交，便于审阅与回滚：
   - prompt document + large paste reference
   - slash command semantic highlighting
   - structured startup diagnostics
   - request-level context snapshot + bottom status ownership
   - bottom-anchored layout + PTY/snapshots/docs/artifact
6. 全部验证通过后 rebase（如需要）、`git merge --ff-only` 回主分支，再删除 worktree 和 task branch。

## 11. 风险与回滚边界

- PromptInput 从裸字符串转为结构化 document 会影响 completion、vim、history、queue 和 editor；必须先建立 source-map
  invariant tests，再替换调用方，不能以字符串占位符快速绕过。
- 公共 SDK 增加 context event 可能影响 JSON/headless/web consumers；若不能保持向后兼容，应改用 TUI 内部 callback / event，
  但 context 计算仍必须在 engine 的实际 request 边界完成。
- startup warning 注入若无分类和脱敏会泄漏配置或把 session 淹没；任何全局 tracing capture 方案直接判为不合格。
- layout 修改会影响 command surfaces 和短终端；回滚时可只回滚 constraint 顺序，不应连带回滚 prompt document 或 diagnostics。
- status 所有权调整可以独立回滚到 context layer，但不得恢复 model 双重显示或累计 token 百分比。

## 12. 2026-07-19 实现审阅后的修复批次

提交 `3e51a518` 已落地大粘贴引用、slash command 高亮、startup diagnostic、context 状态与
bottom-anchored layout 的首版实现，但审阅确认以下边界仍未满足本计划原始契约。本批次在同一计划中继续收口，
不新建平行计划文件。

### 12.1 必须修复

- [x] startup diagnostic 在 DTO 入库前覆盖常见 API key、云凭据、JWT、通用敏感环境变量赋值和 MCP 错误文本；新增
  table-driven redaction tests，确保 session/transcript 不出现原始 secret。
- [x] 删除 TUI 从 session 累计 `UsageTracking` 推导“request-level context”的做法；在 engine 已确定实际请求消息集或收到
  单次 provider usage 的边界产出结构化 `ContextWindowSnapshot`，只把最新请求快照送入 TUI。
- [x] context capacity resolver 使用当前 effective `context_window`，仅在明确启用扩展窗口时使用
  `max_context_window`；覆盖 `gpt-5.4` 的 272k / 1m 差异和 `effective_context_window_percent`。
- [x] slash command highlight 使用可失效的 cwd/registry snapshot cache；普通 render 不重复扫描
  `.allthecodes/workflows`，动态命令或 cwd 变化后仍能刷新。
- [x] 为大 paste、startup warning、单一 model/context status 和 bottom anchor 增加真实 PTY 文本/坐标用例；不得只复用与本任务
  无关的既有 PTY 用例作为完成证据。

### 12.2 文档与验收收口

- [x] 更新 UI-014：在上述问题和定向 PTY 未通过前不得保持无条件 `Fixed`。
- [x] 更新既有 workflow artifact，明确 follow-up commit、测试命令、通过项和任何未通过边界。
- [x] 完成相关 unit、targeted PTY、fmt、clippy、非 PTY workspace lib tests、`git diff --check` 和 workspace release build；
  若完整 PTY 仍有独立失败，必须逐项记录且不能把本计划状态标为全部完成。

### 12.3 本批次实现与验证结果

实现提交：`c29a329f`（`fix(tui): close paste status review gaps`）。定向 unit 覆盖 startup diagnostic 脱敏、动态 registry
revision/cache 失效、单次 Assistant request usage 覆盖前次 context snapshot，以及 `gpt-5.4` 的 272k effective capacity。
新增真实 PTY 用例覆盖 512 字符 bracketed paste、真实 startup model fallback warning、单一 model owner，以及 100x24 下
prompt/context/footer 的 20/22/23 行坐标，2/2 通过。

最终验证：

- `cargo fmt --all --check`：通过。
- 相关 unit：dynamic registry 12/12、startup diagnostics 4/4、context status 5/5、cache 与 SDK event 回归各 1/1。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace --exclude allthecodes --lib`：通过，本次未复现旧 artifact 记录的 gateway 并发偶发失败。
- `cargo test -p allthecodes --test pty_tui_e2e paste_status_followup -- --test-threads=1 --nocapture`：2/2 通过。
- `cargo test -p allthecodes --test pty_tui_e2e -- --test-threads=10`：221 passed、36 ignored、2 failed；失败仍为
  `effort_set_high_reports_profile_result` 与 `surface_mcp_remove_action_updates_project_settings` 的既有等待窗口问题，后者磁盘
  settings 删除断言通过。本任务新增用例和相关行为均通过，因此不把完整 PTY 标为全绿。
- `git diff --check`：通过。
- `cargo build --workspace --release`：通过，独立 target cold release 用时 8m26s，无 warning。
