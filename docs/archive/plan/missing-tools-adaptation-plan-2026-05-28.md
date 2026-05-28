# allthecodes 缺失工具更新适配计划

> 生成日期: 2026-05-28
> 范围: 基于 `docs/reference/tool-comparison-3projects.md` gap 分析，规划 claude-code-bun / codex 已有但 allthecodes (rust) 尚未实现的工具
> 目标: 分优先级补齐工具生态，覆盖权限注册、系统提示词、Tool trait 实现、前端渲染四个层面

---

## 1. 当前工具生态快照

| 来源 | 工具数量 | 说明 |
|------|----------|------|
| **claude-code-bun** (upstream) | ~62 | 最全，含 cron/MCP-resource/通知等特色工具 |
| **allthecodes** (rust，当前) | ~47+ | 基础工具齐全，缺少中级/高级工具 |
| **codex** (OpenAI) | ~31 | 独特的有 apply_patch、goal 管理、multi-agent v2 等 |

### 当前 allthecodes 已实现的工具概览

**allthecodes-tools 自有工具** (25+):
Read / Write / Edit / Glob / Grep / SafeWrite / Bash / PowerShell / REPL / Sleep /
AskUserQuestion / Config / StructuredOutput / SendUserMessage / WebFetch / WebSearch /
EnterPlanMode / ExitPlanMode / Brief / SystemStatus / ToolSearch / TodoWrite /
Task / TaskList / TaskUpdate / TaskOutput / TaskStop

**外部 provider 注册工具** (~15):
Agent / TaskAgent / Skill / Lsp / SendMessage / TeamSpawn /
SubscribePrActivity / UnsubscribePrActivity /
EnterWorktree / ExitWorktree /
mcp__computer-use__screenshot / mcp__computer-use__left_click / mcp__computer-use__right_click /
mcp__computer-use__middle_click / mcp__computer-use__double_click /
mcp__computer-use__type_text / mcp__computer-use__key /
mcp__computer-use__scroll / mcp__computer-use__mouse_move /
mcp__computer-use__cursor_position

**与 bun 等效但实现不同**:
- `subscribe_pr_activity` / `unsubscribe_pr_activity` (bun: `SubscribePR`)
- `ToolSearch` (BM25-based, bun: `SearchExtraTools`)

---

## 2. 缺失工具清单与分级

### P0 — 核心体验缺口（影响基础交互完整性）

| # | 工具 | bun | codex | 缺失影响 |
|---|------|-----|-------|----------|
| 1 | **NotebookEdit** | ✅ | — | Jupyter 用户无法在会话中编辑 notebook cell |
| 2 | **ListMcpResources / ReadMcpResource** | ✅ | list_mcp_resources / read_mcp_resource | MCP 资源探知能力缺失；allthecodes-mcp 已有 Resource 数据结构但未暴露为工具 |

### P1 — 中优先级（提升自动化/调度/协作能力）

| # | 工具 | bun | codex | 缺失影响 |
|---|------|-----|-------|----------|
| 3 | **CronCreate / CronDelete / CronList** | ✅ | — | 无法定时执行任务/轮询 |
| 4 | **WebBrowser** | ✅ | — | 缺少浏览器内内容获取能力，WebFetch 不够应对 JS 渲染页 |
| 5 | **Monitor** | ✅ | — | 长时后台任务监控缺失 |
| 6 | **SendUserFile** | ✅ | — | 缺少直接向用户发送文件的能力 |

### P2 — 低优先级（产品体验增强）

| # | 工具 | bun | codex | 缺失影响 |
|---|------|-----|-------|----------|
| 7 | **SubscribePR** (完整版) | ✅ | — | 当前 `subscribe_pr_activity` 功能不全 |
| 8 | **TerminalCapture** | ✅ | — | 终端输出捕获 |
| 9 | **ReviewArtifact** | ✅ | — | 审查工作制品 |
| 10 | **Snip** | ✅ | — | 历史消息压缩 |
| 11 | **CtxInspect** | ✅ | — | 上下文窗口检视 |
| 12 | **RemoteTrigger** | ✅ | — | 远程触发其他 Claude Code 实例 |
| 13 | **ListPeers** | ✅ | — | 本地会话发现 |

### P3 — 暂缓（需要基础设施或不属于当前核心目标）

| # | 工具 | bun | codex | 说明 |
|---|------|-----|-------|------|
| 14 | LocalMemoryRecall | ✅ | — | 需要跨会话内存存储层 |
| 15 | VaultHttpFetch | ✅ | — | 需要加密凭据存储 |
| 16 | PushNotification | ✅ | — | 需要移动端基础设施 |
| 17 | DiscoverSkills | ✅ | — | Skill 发现，优先级低 |
| 18 | VerifyPlanExecution | ✅ | — | 计划验证工作流 |
| 19 | workflow | ✅ | — | 工作流脚本引擎 |
| 20 | ExecuteExtraTool | ✅ | — | 延迟工具执行 |
| 21 | **apply_patch** | — | ✅ | Tree-sitter AST 感知的语义化 patch，需重大前端投入 |
| 22 | **Goal 管理** (get_goal / create_goal / update_goal) | — | ✅ | 目标管理系统 |
| 23 | **Multi-agent v2** (send_message / followup_task / list_agents / close_agent / wait_agent) | — | ✅ | 增强型多 agent 通信 |
| 24 | view_image | — | ✅ | 已可通过 computer-use 截图覆盖 |

---

## 3. 各工具适配方案

### 3.1 NotebookEdit (P0)

**状态**: 权限配置已预留 (`allthecodes-permissions/src/rules.rs` 已有 `NotebookEdit`)，UI permission router 已注册 `PermissionRouteKind::NotebookEdit`，但无实际工具实现。

**实现方案**:
- 新工具文件: `allthecodes-tools/src/notebook_edit.rs`
- 依赖: `serde_json` 解析 cell 标识 + 内容；本地 `.ipynb` 文件读写复用现有 `Read`/`Write` 的文件操作能力
- 核心能力:
  - 读取 notebook 文件并解析 cell 结构
  - 替换指定 cell 的源代码
  - 支持插入新 cell、删除 cell
  - 支持 code/markdown cell 类型切换
- 注册: `allthecodes-tools` 的 `fs::tools()` 中追加，或独立模块

**估算**: ~400 行 Rust，复用 `allthecodes-tools` 的 tool trait / result / 错误处理

### 3.2 ListMcpResources / ReadMcpResource (P0)

**状态**: `allthecodes-mcp` 已有 `ResourceDefinition` 数据结构和 `resources/list` 发现能力。UI 层已有 MCP resource 渲染 (`render_mcp_resource`)。但未暴露为 agent 可用工具。

**实现方案**:
- 两个工具均可放在 `allthecodes-mcp` crate 中作为外部 provider 注册
- `ListMcpResources`: 查询所有已连接 MCP server 的 resource 列表，返回名称/URI/MIME 类型
- `ReadMcpResource`: 按 URI 读取具体 resource 内容
- 需要扩展 `McpManager` 暴露 resource 枚举和读取接口

**估算**: ~300 行 Rust，主要工作量在 `McpManager` 接口扩展

### 3.3 Cron 套件 (P1)

**状态**: bun 版本基于 `node-cron`，Rust 需要纯 Rust cron 解析器。

**实现方案**:
- 新 crate `allthecodes-cron` 或直接放入 `allthecodes-tools/src/cron/`
- 依赖: `cron` crate (Rust 版 cron 解析)
- 三个工具:
  - `CronCreate`: 传入 cron 表达式 + prompt + (可选) recurring 标志
  - `CronDelete`: 按 job ID 删除
  - `CronList`: 列出所有活跃 cron job
- 存储: 内存 + `~/.allthecodes/cron_jobs.json` 持久化
- 注意: 与现有 `allthecodes-daemon` 的集成，确保 cron 在 daemon 模式下存活

**估算**: ~600 行 Rust

### 3.4 WebBrowser (P1)

**状态**: 当前 `WebFetch` 只能处理静态 HTML，无法执行 JS。

**实现方案**:
- 方案 A (推荐): 利用现有 `allthecodes-browser` cmar 的 Chrome/CDP 集成能力
- 方案 B: 借助 `headless_chrome` crate（如需要全功能浏览器）
- 核心功能: 打开 URL → 等待渲染 → 提取文本/截图 → 返回结构化内容
- 权限: 需要单独的浏览器访问许可（安全敏感）

**估算**: ~500 行 Rust，依赖 `allthecodes-browser` 能力

### 3.5 Monitor (P1)

**实现方案**:
- 新工具: `allthecodes-tools/src/monitor.rs`
- 功能: 在后台启动一个命令/脚本，定期检查输出，通过 tool result 返回状态更新
- 类似于 bun 版的长时运行 `TerminalCapture` + 循环检查
- 存储: 使用 `allthecodes-tasks` 的 task store 追踪 monitor 状态

**估算**: ~350 行 Rust

### 3.6 SendUserFile (P1)

**实现方案**:
- 新工具: `allthecodes-tools/src/send_user_file.rs`
- 功能: 读取指定文件并通过消息系统以 attachment 形式发送给用户
- 复用 `Read` 工具的文件读取逻辑 + `Message::Attachment` 消息构造
- UI 侧: 需要文件类型的 attachment 渲染支持（多数已有）

**估算**: ~150 行 Rust（较轻量）

### 3.7 其余 P2 工具

| 工具 | 建议方案 | 估算行数 |
|------|---------|---------|
| **TerminalCapture** | 新建 `allthecodes-tools/src/terminal_capture.rs`，启动 PTY 后台进程捕获输出 | ~400 |
| **ReviewArtifact** | UI/UX 定义先行；后端为结构化 artifact + annotation 渲染 | ~300 |
| **Snip** | 利用现有 `allthecodes-compact` 的 compaction 能力 | ~200 |
| **CtxInspect** | 读取当前 `QueryEngine` 的上下文统计并格式化输出 | ~150 |
| **RemoteTrigger** | 需要先定义 CCR API 客户端；可先做 stub + 配置驱动的 HTTP 调用 | ~300 |
| **ListPeers** | 基于 UDS socket 扫描；已有 IPC transport 为基础 | ~250 |
| **SubscribePR (完整)** | 扩展现有 `subscribe_pr_activity`，补齐 bun 版订阅能力 | ~200 |

---

## 4. 架构改动影响

### 4.1 新增文件/模块

```
crates/allthecodes-tools/src/
├── notebook_edit.rs          # P0: NotebookEdit 工具
├── cron/
│   ├── mod.rs                # P1: Cron 工具模块
│   ├── create.rs
│   ├── delete.rs
│   └── list.rs
├── web_browser.rs            # P1: WebBrowser 工具
├── monitor.rs                # P1: Monitor 工具
├── send_user_file.rs         # P1: SendUserFile 工具
├── ctx_inspect.rs            # P2: CtxInspect 工具
├── terminal_capture.rs       # P2: TerminalCapture 工具
├── review_artifact.rs        # P2: ReviewArtifact 工具
└── snip.rs                   # P2: Snip 工具

crates/allthecodes-mcp/src/
├── resources.rs              # P0: ListMcpResources / ReadMcpResource 实现
└── resource_tools.rs         # P0: Tool trait 适配器

crates/allthecodes-cron/      # P1: 可选独立 crate
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── job.rs
│   ├── scheduler.rs
│   └── storage.rs
```

### 4.2 需修改的现有文件

| 文件 | 改动 | 优先级 |
|------|------|--------|
| `crates/allthecodes-tools/src/fs/mod.rs` | 追加 NotebookEdit 到 `tools()` | P0 |
| `crates/allthecodes-tools/src/registry.rs` | 追加新工具的注册 | P0-P2 |
| `crates/allthecodes-startup/src/tool_registry.rs` | 追加 MCP resource 工具 provider | P0 |
| `crates/allthecodes-mcp/src/lib.rs` | 暴露 Resource 查询接口 | P0 |
| `crates/allthecodes-mcp/src/manager.rs` | 新增 resource 路由/缓存 | P0 |
| `crates/allthecodes-permissions/src/rules.rs` | 注册新工具的权限规则（部分已有） | P0-P2 |
| `crates/allthecodes/src/ui/permissions/permission_request_router.rs` | 追加新工具 UI 权限路由（部分已有） | P0-P2 |

### 4.3 系统提示词改动

新工具需要加入系统提示词，引导模型在合适场景调用：
- NotebookEdit → 在检测到 `.ipynb` 文件编辑时优先选择
- ListMcpResources / ReadMcpResource → 在需要访问 MCP server 数据时激活
- Cron 套件 → 在需要定时/周期性任务时调用
- WebBrowser → 在 WebFetch 无法获取渲染后内容时建议使用

---

## 5. 执行路线图

### Phase 0 — 基线冻结与实现边界 (P0 准备, 2-3 天)

Phase 0 不做运行时行为改动，目标是把 P0 工具的上游契约、当前 Rust 接线点、测试基线和命名边界冻结下来，避免 Phase 1 一边实现一边重新判定范围。

#### 0A. 上游契约冻结

参考路径：
- `claude-code-bun/packages/builtin-tools/src/tools/NotebookEditTool/NotebookEditTool.ts`
- `claude-code-bun/packages/builtin-tools/src/tools/ListMcpResourcesTool/ListMcpResourcesTool.ts`
- `claude-code-bun/packages/builtin-tools/src/tools/ReadMcpResourceTool/ReadMcpResourceTool.ts`
- `claude-code-bun/src/services/mcp/client.ts`

输出物：
- 记录上游参考 commit、allthecodes 当前 commit、对照日期。
- 固定 `NotebookEdit` 输入字段：`notebook_path`、`cell_id`、`new_source`、`cell_type`、`edit_mode`。
- 固定 `NotebookEdit` 输出字段：`new_source`、`cell_id`、`cell_type`、`language`、`edit_mode`、`error`、`notebook_path`、`original_file`、`updated_file`。
- 固定 MCP resource 工具输入/输出结构：`server` 可选过滤、`uri` 必填读取、`contents[]` 支持 text 与 blob。
- 明确工具命名策略：Phase 1 默认采用 allthecodes 当前 CamelCase 内置工具风格；若选择与 bun 完全兼容，应显式采用 `ListMcpResourcesTool` / `ReadMcpResourceTool`，否则在本计划中记录保留差异并说明映射关系。

#### 0B. 当前 Rust 接线点清单

确认并记录以下当前状态：
- `crates/allthecodes-permissions/src/rules.rs` 已把 `NotebookEdit` 纳入 edit/AcceptEdits 规则。
- `crates/allthecodes/src/ui/permissions/permission_request_router.rs` 已有 `PermissionRouteKind::NotebookEdit`、`Monitor`、`ReviewArtifact` 路由。
- `crates/allthecodes-mcp/src/client/mod.rs` 已实现 `list_resources()` 与 `read_resource()`。
- `crates/allthecodes-mcp/src/manager.rs` 只有 `all_resources()` 快照，缺少按 server/URI 定位和读取的 manager API。
- `crates/allthecodes-startup/src/tool_registry.rs` 目前只注册 root-owned 工具、plugin 工具；MCP resource helper tools 还没有 provider 接线。

输出物：
- 在本文件追加一段 “Phase 0 Baseline Snapshot”，列出上述状态与缺口。
- 若发现本计划中路径使用旧写法（例如缺少 `crates/` 前缀），在 Phase 0 同步修正文档路径，避免后续任务误改旧路径。

#### 0C. 行为边界与非目标

Phase 1 必做：
- `NotebookEdit` 必须要求 Read-before-edit，并复用 `FileStateCache` 防止 stale edit。
- `.ipynb` 必须保留除目标 cell 外的 notebook metadata、cell metadata、nbformat/nbformat_minor。
- code cell 修改或插入时必须清空 `outputs` 并把 `execution_count` 置为 `null`。
- `source` 必须兼容 notebook 中的 string 与 string array 表示；写回策略以 Phase 0 冻结记录为准。
- MCP resource 工具必须只读取当前已连接 MCP server，不隐式启动未配置 server。
- MCP resource 工具必须 read-only、concurrency-safe，不进入 edit 权限路径。

Phase 1 非目标：
- 不实现 Cron / WebBrowser / Monitor / SendUserFile。
- 不引入新的 MCP transport。
- 不重构 MCP tool wrapper 的既有命名规则。
- 不做完整 binary blob 持久化目录设计；若 ReadMcpResource 遇到 blob，Phase 1 只允许采用已有 allthecodes 数据目录下的最小安全落盘方案，或返回明确的 “binary unsupported in Phase 1” 错误。

#### 0D. 验证基线

Phase 0 结束前运行并记录结果：

```
cargo test -p allthecodes-startup tool_registry -- --nocapture
cargo test -p allthecodes-mcp --lib -- --nocapture
cargo test -p allthecodes-tools fs:: -- --nocapture
cargo test -p allthecodes-permissions rules -- --nocapture
cargo test -p allthecodes ui::permissions::permission_request_router -- --nocapture
```

如果某个命令已有非本任务失败，Phase 0 要记录失败用例、首个错误和是否阻塞 Phase 1；不要在 Phase 1 中把无关历史失败混入 P0 工具实现。

#### Phase 0 退出条件

- [x] 上游契约、Rust 接线点、命名策略已记录。
- [x] Phase 1 文件写入范围明确。
- [x] Phase 1 非目标明确，未把 P1/P2 工具提前混入。
- [x] 当前测试基线已运行或记录无法运行原因。
- [x] 没有运行时行为变更；Phase 0 只产生文档变更。

#### Phase 0 Implementation Snapshot (2026-05-29)

参考版本：
- allthecodes: `e131e6e8180cd41763d09c4ccf5efcd6aab9fef8`
- claude-code-bun: `2cc9a7daef652286ba8ad27f3489d713004059b7`

冻结契约：
- `NotebookEdit` 使用 `notebook_path`、`cell_id`、`new_source`、`cell_type`、`edit_mode`。
- `NotebookEdit` 返回 `new_source`、`cell_id`、`cell_type`、`language`、`edit_mode`、`error`、`notebook_path`、`original_file`、`updated_file`。
- MCP resource 工具采用 allthecodes CamelCase 名称：`ListMcpResources`、`ReadMcpResource`。
- `ListMcpResources` 输入为可选 `server`，输出 resource 列表并包含 `server`、`uri`、`name`、`description`、`mimeType`。
- `ReadMcpResource` 输入为必填 `server` 与 `uri`，输出 `contents[]`；binary blob 不直接写入模型上下文。

基线结果：
- `cargo test -p allthecodes-startup tool_registry -- --nocapture`: 通过，7 passed。
- `cargo test -p allthecodes-tools fs:: -- --nocapture`: 通过，72 passed。
- `cargo test -p allthecodes-permissions rules -- --nocapture`: 通过，52 passed。
- `cargo test -p allthecodes ui::permissions::permission_request_router -- --nocapture`: 通过，8 passed。
- `cargo test -p allthecodes-mcp --lib -- --nocapture`: sandbox 内因 loopback/OAuth socket 权限失败；按批准的 escalated 命令重跑通过，60 passed。

### Phase 1 — P0 工具快速补齐 (1-2 周)

Phase 1 只交付 `NotebookEdit` 与 MCP resource helper tools。建议拆成 5 个可独立 review 的切片，按顺序执行；前两个 NotebookEdit 切片可并行做只读调研，但代码落地应串行，避免同一 fs 模块冲突。

#### 1A. NotebookEdit 数据模型与纯函数

写入范围：
- `crates/allthecodes-tools/src/fs/notebook_edit.rs`
- `crates/allthecodes-tools/src/fs/mod.rs`
- 必要时补充 `crates/allthecodes-tools/Cargo.toml`，但优先使用已有 `serde_json`、`uuid`、`chrono`、`similar`、`tokio`。

实现要点：
- 定义 `NotebookEditTool`，工具名为 `NotebookEdit`。
- 定义内部 `NotebookDocument` / `NotebookCell` serde 表示，保留未知字段，避免丢弃上游或 Jupyter 扩展 metadata。
- 支持 `edit_mode`: `replace`、`insert`、`delete`，默认 `replace`。
- 支持 `cell_type`: `code`、`markdown`；`insert` 时必填或按上游兼容策略默认 `code`，具体以 Phase 0 冻结结果为准。
- cell 定位顺序与 bun 对齐：先按真实 cell `id` 查找，再支持 `cell-N` 数字索引格式。
- `replace` 指向末尾后一格时按上游行为转为 `insert`。
- 修改 code cell 时清空 `outputs`，`execution_count` 写为 `null`。
- 新增 nbformat 4.5+ cell 时生成 cell `id`；低版本 notebook 不强制写入 `id`。
- 用纯函数覆盖 JSON parse、cell 查找、cell 修改、写回 JSON 构造，降低工具 I/O 测试成本。

测试要求：
- 有真实 `.ipynb` fixture 或 inline JSON fixture。
- 覆盖 replace by id、replace by `cell-N`、insert after cell、insert at beginning、delete、cell type switch。
- 覆盖 code cell 输出清理。
- 覆盖 string source 与 array source 输入。
- 覆盖 invalid JSON、非 `.ipynb`、cell 不存在、非法 edit_mode。

#### 1B. NotebookEdit 文件 I/O、权限与结果渲染数据

写入范围：
- `crates/allthecodes-tools/src/fs/notebook_edit.rs`
- `crates/allthecodes-tools/src/fs/mod.rs`
- 如 TUI 现有 NotebookEdit permission preview 需要字段补齐，仅修改 `crates/allthecodes/src/ui/permissions/notebook_edit_permission_request/`

实现要点：
- `input_json_schema()` 与 Phase 0 冻结契约一致。
- `validate_input()` 执行路径校验、扩展名校验、read-before-edit、stale read 校验、notebook JSON 校验、cell 定位校验。
- `get_path()` 返回 `notebook_path`，`is_destructive()` 返回 true，`is_read_only()` 返回 false，`is_concurrency_safe()` 返回 false。
- `check_permissions()` 复用现有文件写权限决策路径；不要新增独立权限语义，除非 Phase 0 证明现有 API 不可复用。
- `call()` 使用 `safe_write_text()` 原子写回，更新 `FileStateCache`，并复用 `edited_text_file_message()` 产生 edited attachment。
- 触发 `FileChanged` hook，operation 建议为 `notebook_edit`，payload 至少包含 `file_path`、`edit_mode`、`cell_id`、`cell_type`、`safe_write`。
- `ToolResult.data` 返回冻结输出字段；`model_content` 返回简洁文字，不把完整 notebook 重复塞入模型结果。
- `display_preview` 提供 TUI 可消费的 JSON，至少含 `kind: "notebook_edit"`、`path`、`edit_mode`、`cell_id`、`hunk_lines` 或摘要。

测试要求：
- Read → NotebookEdit → Read 不返回 stale 内容。
- 未 Read 直接编辑失败。
- Read 后文件被外部修改时失败。
- safe write 失败时返回 tool error data，不 panic。
- registry 测试能发现 `NotebookEdit` 且 schema 合法。

#### 1C. MCP manager resource API

写入范围：
- `crates/allthecodes-mcp/src/manager.rs`
- `crates/allthecodes-mcp/src/lib.rs`
- `crates/allthecodes-mcp/src/client/client_tests.rs` 或 manager 专用测试模块

实现要点：
- `list_resources(server: Option<&str>) -> Result<Vec<McpResourceWithServer>>`，结果包含 `server`、`uri`、`name`、`description`、`mime_type`。
- `read_resource(server: &str, uri: &str) -> Result<ReadResourceResult>`，按 server 精确定位，区分 “server 不存在”、“server 未连接”、“server 不支持 resources”、“resource read failed”。
- 可选增加 `find_client_for_resource(server, uri)` 或 `client_for_server(server)`；不要暴露可变 client 引用到不必要的外层。
- 如果已有 `resources/list_changed` 通知路径，应记录是否会刷新 `client.resources`；Phase 1 至少保证手动 list 后结果不 stale。
- manager API 保持 read-only 语义，不影响现有 `all_tools()` / MCP tool wrapper。

测试要求：
- 多 server 聚合列表。
- 按 server 过滤列表。
- server 不存在错误包含可用 server 列表。
- 读取 resource 时调用正确 server。
- 单 server 失败不影响其他 server list，或明确记录 Phase 1 是否 fail-fast。

#### 1D. MCP resource Tool adapter 与注册

推荐实现位置：
- `crates/allthecodes-engine/src/mcp_resource_tools.rs`，如果需要访问运行时 MCP manager。
- 或 `crates/allthecodes-mcp/src/resource_tools.rs`，前提是不会造成 `allthecodes-mcp -> allthecodes-tools` 依赖环。
- 注册点：`crates/allthecodes-startup/src/tool_registry.rs`

实现要点：
- 实现 `ListMcpResources` 与 `ReadMcpResource` 两个 `Tool`。
- 二者 `is_read_only()`、`is_concurrency_safe()` 返回 true，`is_destructive()` 返回 false。
- `ListMcpResources` 输入：`server?: string`。
- `ReadMcpResource` 输入：`server: string`、`uri: string`。
- `ToolResult.data` 对齐 Phase 0 冻结契约。
- 文本 resource 进入 `model_content`；binary blob 不直接把 base64 放入上下文。Phase 1 若未实现安全落盘，应返回明确错误并在本计划记录剩余项。
- provider 注册后，`get_all_tools()` 包含两个工具且不与 MCP server 动态工具重名。
- 如果最终工具名选择 bun 兼容形式，所有测试和文档统一使用 `ListMcpResourcesTool` / `ReadMcpResourceTool`，不要混用。

测试要求：
- `test_all_tools_have_unique_names` 通过。
- `test_all_tools_have_schema` 通过。
- read-only/concurrency-safe 断言。
- 空 resource list 返回 “No resources found” 类似提示。
- server 过滤和 read 错误路径有稳定消息。

#### 1E. 系统提示词、ToolSearch、TUI/headless 验证

写入范围：
- `crates/allthecodes-engine/src/system_prompt/` 下现有动态/静态提示词文件。
- `crates/allthecodes-tools/src/tool_search.rs` 仅在新工具没有被通用索引自然覆盖时修改。
- `crates/allthecodes/src/ui/messages/` 或 permission preview 文件仅在现有渲染无法展示新结果时修改。

实现要点：
- 系统提示词说明：编辑 `.ipynb` 优先用 `NotebookEdit`，不要用 `Edit` 直接改 JSON，除非 NotebookEdit 不可用。
- 系统提示词说明：需要 MCP server 资源时先 list，再 read；不要猜 URI。
- ToolSearch 能用 “jupyter / notebook / ipynb / mcp resources” 找到对应工具。
- TUI permission router 已有 NotebookEdit route；Phase 1 只补缺失字段，不重写权限 UI。
- headless JSONL 输出包含新工具 result，不破坏现有 attachment/message schema。

验证命令：

```
cargo test -p allthecodes-tools notebook_edit -- --nocapture
cargo test -p allthecodes-mcp --lib -- --nocapture
cargo test -p allthecodes-startup tool_registry -- --nocapture
cargo test -p allthecodes-engine mcp_resource -- --nocapture
cargo test -p allthecodes ui::permissions::permission_request_router -- --nocapture
cargo build --workspace --release
git diff --check
```

### Phase 2 — 调度与自动化 (P1, 3-4 周)

```
Week 3-4:
  [Cron 套件]
    - 实现 cron 解析、调度器、持久化
    - 注册三个工具
    - 集成 daemon 生命周期

  [WebBrowser]
    - 对接 allthecodes-browser 的 CDP 能力
    - 权限模型（安全敏感: 需要单独授权）
    - 前端渲染（全页面/截图/文本提取）
```

### Phase 3 — 产品体验 (P1-P2, 5-7 周)

```
Week 5:
  [Monitor + SendUserFile]
    - 后台监控 + 文件发送

Week 6-7:
  [TerminalCapture + Snip + CtxInspect + ReviewArtifact]
    - 终端捕获、历史压缩、上下文检视、制品审查
    - 按依赖关系排序：CtxInspect 最简单（纯读取），Snip 需要 compaction 层
```

### Phase 4 — 协作与远程 (P2, 8-9 周)

```
Week 8-9:
  [RemoteTrigger + ListPeers + SubscribePR 补齐]
    - 远程触发需要先定义 CCR API 协议
    - ListPeers 基于已有 IPC transport
    - SubscribePR 补齐 bun 版功能
```

### Phase 5 — 评估 (P3, 持续)

```
持续:
  - LocalMemoryRecall → 等跨会话存储基础设施就绪
  - VaultHttpFetch → 等凭据管理基础设施就绪
  - PushNotification → 等移动端推送通道
  - apply_patch → 评估 Tree-sitter AST 集成成本
  - Goal 管理 → 评估是否需要独立于 Task 系统的目标管理层
  - Multi-agent v2 → 评估当前 teams/coordinator 是否可扩展
```

---

## 6. 依赖与阻塞

| 工具 | 外部依赖 | 内部依赖 | 风险 |
|------|---------|---------|------|
| NotebookEdit | `serde_json` (已有) | 无 | 低 |
| ListMcpResources | 无 | allthecodes-mcp resource 接口暴露 | 低 |
| ReadMcpResource | 无 | allthecodes-mcp resource 接口暴露 | 低 |
| Cron | `cron` crate | daemon 生命周期 | 中（daemon 集成） |
| WebBrowser | `allthecodes-browser` / `headless_chrome` | 浏览器检测、权限系统 | 中（安全/平台差异） |
| Monitor | 无 | task store、daemon | 低 |
| RemoteTrigger | 无 | CCR API 协议定义 | 中（协议未定） |
| TerminalCapture | 无 | PTY/进程管理 | 低 |
| SubscribePR | GitHub API | teams 基础设施 | 低（已有基础） |
| CtxInspect | 无 | QueryEngine 上下文统计 API | 低 |
| Snip | 无 | `allthecodes-compact` | 低 |

---

## 7. 验收标准

### Phase 0 验收

- [ ] 已记录上游参考 commit、allthecodes 当前 commit、对照日期
- [ ] 已冻结 `NotebookEdit`、`ListMcpResources`、`ReadMcpResource` 的输入/输出契约
- [ ] 已确认 P0 工具命名策略，且文档内没有混用未说明的别名
- [ ] 已记录 Rust 现有接线点、缺失接口、Phase 1 写入范围
- [ ] 已运行 Phase 0 基线命令，或记录每个无法运行命令的原因和首个错误
- [ ] Phase 0 未引入运行时行为变更

### Phase 1 验收

- [x] NotebookEdit 可编辑 `.ipynb` 文件中的任意 cell
- [x] NotebookEdit 支持插入/删除 cell 和切换 cell 类型
- [x] NotebookEdit 强制 Read-before-edit，并能拒绝 stale edit
- [x] NotebookEdit 修改 code cell 时清空 outputs 且重置 execution_count
- [x] ListMcpResources 列出所有已连接 MCP server 的资源
- [x] ReadMcpResource 按 URI 读取 resource 内容
- [x] MCP resource 工具为 read-only、concurrency-safe，且不进入 edit 权限路径
- [x] 所有 P0 工具在 `allthecodes_tools_base_tools()` 或 provider 中注册
- [x] 所有 P0 工具有单元测试
- [x] Phase 1 验证命令通过，或文档记录非本任务失败的首个错误与剩余风险

### Phase 2 验收

- [ ] Cron 支持标准 5 字段表达式
- [ ] Cron job 在 daemon 模式下正确触发
- [ ] Cron job 持久化跨进程重启存活
- [ ] WebBrowser 能获取 JS 渲染后的页面内容
- [ ] WebBrowser 有独立的权限提示（安全模型）

### Phase 3 验收

- [ ] Monitor 后台运行并返回定期状态更新
- [ ] SendUserFile 正确发送文件内容给用户
- [ ] TerminalCapture 捕获 PTY 输出
- [ ] Snip 对历史消息执行 compaction
- [ ] CtxInspect 返回有意义的上下文统计

### 通用

- [ ] 所有新工具通过 `test_all_tools_have_unique_names` 测试
- [ ] 所有新工具通过 `test_all_tools_have_schema` 测试
- [ ] 所有新工具有完整的 JSON schema
- [ ] 权限系统为每个新工具注册相应规则
- [ ] TUI 和 headless 前端正确渲染新工具的输出
