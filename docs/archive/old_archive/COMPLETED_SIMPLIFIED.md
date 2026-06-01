# 已完成模块 — 大幅简化实现（进入全量构建后视作待补齐清单）

> **阶段切换 (2026-04-22)**：本仓库已从 "rust-lite 精简版" 切换到**全量构建 (Full Build)**。
> 原先本文登记的"已接受的缩减实现"一律**重新解读为"全量构建阶段待补齐的 TODO"**。
> 不要再把本文里的 "TS 独有（未移植）" 列表当作"不做"的白名单——它是补齐工单的参考清单。
> 规则与补齐流程见 [`../../CLAUDE.md`](../../CLAUDE.md) 顶部"当前阶段"说明，以及 [`../IMPLEMENTATION_GAPS.md`](../IMPLEMENTATION_GAPS.md) §2 与 §7。
>
> 最后更新: 2026-04-02 (Phase 14B-3/14C/14D: 30 新命令 + 9 新工具 + path_validation + config validation)
> 此文档原意是记录已标记 ✅ 但相比 TypeScript 原版有显著功能缩减的模块；在全量构建阶段，缩减部分应被逐项补齐。
> 完整实现见 [`COMPLETED_FULL.md`](COMPLETED_FULL.md)。
> 详细简化率分析见 [`MODULE_SIMPLIFICATION.md`](MODULE_SIMPLIFICATION.md)。
> 剩余工作见 [`REWRITE_PLAN.md`](../REWRITE_PLAN.md)。

---

## 简化分级

| 级别 | 缩减率 | 含义 |
|------|--------|------|
| **S** | >80% | 仅保留核心骨架，大量功能未移植 |
| **A** | 50-80% | 核心功能完整，高级功能缺失 |
| **B** | 30-50% | 较完整，部分边缘 case 省略 |

---

## 1. S 级简化 (>80% 缩减)

### 1.1 BashTool — 仍有 Full Build 差距 (输出截断 / heredoc / Git 跟踪 / 进程树终止 / 危险命令拒绝列表 / security validator 高风险规则与 AST 启发式批次 / script block securityPatterns 子集 / 明显 parse-error fail-closed / sandbox FS preflight / fail-closed 用户面已补全)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 12,411 (18 文件) | 830 + 934 + 563 + 193 + 557 (+security gate) |
| 文件 | — | `tools/exec/bash.rs` + `crates/cc-utils/src/bash.rs` + `crates/cc-utils/src/git_operation_tracking.rs` + `tools/exec/process_control.rs` + `crates/cc-permissions/src/dangerous.rs` + `crates/cc-sandbox/src/runner.rs` |

**Rust 保留：**
- 基础进程执行 (`tokio::process::Command`)
- timeout 支持
- stdout/stderr 输出捕获
- ✅ **输出截断策略** (head 200 行 + tail 100 行 + 中间省略, 行边界感知)
- ✅ **heredoc 校验** (未闭合 delimiter、quoted delimiter、`<<-`、多 heredoc、quoted text / arithmetic shift 规避)
- ✅ **Git 操作跟踪** (shell-agnostic 检测 commit/amend/cherry-pick、push branch、merge/rebase、`gh pr`、`glab mr create`、curl PR endpoint；Bash/PowerShell 成功结果附带 `git_operations`)
- ✅ **进程树终止 / 取消语义** (Unix process group、Windows `taskkill /T /F`，超时和 abort signal 终止进程树并返回 `termination` 元数据)
- ✅ **命令拒绝列表与危险命令分析子项** (force-with-lease、`git clean` dry-run 例外、stash drop/clear、SQL drop/truncate、PowerShell destructive cmdlet/alias)
- ✅ **PowerShell security validator 高风险与 AST 启发式规则** (`Invoke-Expression`、嵌套 PowerShell、download cradle、`Add-Type`、COM object、`Start-Process` 提权/再拉 PowerShell、WMI/CIM 任意方法调用 fail-closed；standalone download utilities、script file execution、`ForEach-Object -MemberName`、`Invoke-Item`、scheduled task、env/module/runtime-state mutation；动态 IEX、危险 script block、stop-parsing、明显危险 static method；一般 dynamic command name、dot-sourced dynamic command、subexpression、expandable string、splatting、member/static member invocation、非 CLM allowlist type literal、`nameType=application` command-name guard 子集)
- ✅ **PowerShell 参数绑定安全规则子项** (`Start-Process -Verb:RunAs` 冒号/quote/backtick 形式、`Start-Job`/`Start-ThreadJob` 位置脚本文件参数、`ForEach-Object`/`%` 位置 `MemberName` 参数)
- ✅ **PowerShell `New-Object` TypeName CLM 子项** (`New-Object` 的 `-TypeName` / `-t:` / 位置 TypeName 参数按上游 CLM allowlist 校验)
- ✅ **PowerShell `securityPatterns.hasScriptBlocks` 子集** (非安全消费者的 script block fail-closed，仅允许 Where/Sort/Select/Group/Format 过滤与输出消费者)
- ✅ **PowerShell parser-invalid fail-closed 子项** (`PowerShellTool::validate_input()` 调用原生 `Parser.ParseInput()` 拒绝完整 parser errors；权限库保留未闭合 quote/paren/brace/type literal 与 mismatched delimiter 轻量 fallback)
- ✅ **PowerShell 原生 AST metadata 安全门** (`PowerShellTool::validate_input()` 解析原生 AST metadata，复核 `elementTypes`、colon-bound `children`、raw command name / `nameType` 语义与 statement `securityPatterns`，拒绝动态/path-like command name、colon-bound 表达式、splatting、stop-parsing、`using` / `#Requires` 与非安全 script block)
- ✅ **sandbox 文件系统 preflight 子项** (shell redirection、常见 Bash 写命令、PowerShell 写 cmdlet 按 read-only/workspace/allowWrite/denyWrite 执行 Rust 级拒绝)
- ✅ **sandbox fail-closed 用户面** (`/sandbox require` / `/sandbox optional` 切换 `sandbox.failIfUnavailable`，缺少 OS-level primitive 时可明确硬失败或 best-effort fallback)
- ✅ **Windows sandbox OS-level primitive 决策** (Restricted Token / Job Object 不自研；上游 sandbox-runtime/PowerShell UI 当前不支持 Windows sandbox，cc-rust 保留 Rust-level preflight 与 fail-closed 用户面，详见 `IMPLEMENTATION_GAPS.md` §7)

**TS 独有（未移植）：**
- PowerShell 分支 (8,959 行的 PowerShellTool)
- 复杂后台任务 / auto-background 超时逻辑
- 终端大小感知

---

### 1.2 AgentTool — 87% 缩减 (worktree 隔离已补全)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 6,072 (14 文件) | 789 (1 文件) |
| 文件 | — | `tools/agent.rs` |

**Rust 保留：**
- 子 QueryEngine 派生
- 递归深度限制
- agent_id/agent_type 传递
- 基础输入验证 (prompt 必填)
- 模型覆盖 (model alias 解析)
- ✅ **worktree 隔离执行** (`isolation: "worktree"`, 创建临时 worktree → 运行 agent → 变更检测 → 自动清理/保留, fail-closed 安全)
- ✅ **工具白名单过滤与定义去重** (child `QueryEngineConfig` 按 `subagent_type` 解析内置/用户/项目 agent 定义，应用 `tools` / `disallowedTools`，按工具名去重；Explore/Plan/code-reviewer 不再继承全量工具)
- ✅ **团队上下文继承** (`AgentContext` 携带父会话 `team_context`，child `QueryEngine` 初始化时恢复到 AppState；子 agent 内的 `SendMessage` 可继续使用当前团队上下文)
- ✅ **AgentTool 多 agent 调度入口** (`Agent` 支持 `name` / `team_name` / `mode`；`name` 触发 in-process `TeamSpawn` teammate，返回 `teammate_spawned` / `teammate_id` / `team_name`，并把 `mode: "plan"` 映射到 teammate plan-mode requirement)

**TS 独有（未移植）：**
- 多后端 spawn 中的 tmux / iTerm2 pane backend
- `spawnMultiAgent.ts` 的 tmux/iTerm2 pane 布局与安装引导细节（§7 Intentional 裁剪）

---

### 1.3 FileEditTool — 缩减实现 (fuzzy + 安全/历史/缩进/transcript 子项已补全)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 1,812 (6 文件) | 1,001 (1 文件) |
| 文件 | — | `tools/file_edit.rs` |

**Rust 保留：**
- 精确字符串匹配替换
- `replace_all` 选项
- 路径验证
- ✅ **Fuzzy 匹配** (基于 `similar::TextDiff` 的滑动窗口相似度搜索, >60% 阈值建议最佳匹配, 显示行号范围和相似百分比)
- ✅ **读后冲突检测** (`Read` 完整读取登记共享 `FileStateCache`; `Edit` 拒绝未读或读后被外部修改的文件，并在成功编辑后刷新缓存)
- ✅ **文件锁/readonly 写前检查** (`Edit` 写入前尝试读写打开目标文件，提前拒绝 readonly、PermissionDenied、WouldBlock 与 Windows sharing violation)
- ✅ **编辑历史备份** (`Edit` 复用 `safe_write_text()`，覆盖前创建恢复备份并在结果 / hook payload 暴露 `edit_history.backup_path`)
- ✅ **自动缩进修正** (`Edit` 精确匹配失败时查找唯一缩进等价块，并把 `new_string` leading whitespace 映射到文件实际缩进；歧义候选拒绝)
- ✅ **live transcript FileEdit 预览** (`Edit` 结果分离 concise model content 与 UI-only `display_preview`；SDK replay/headless IPC/Rust TUI 保留 `tool_use_result` 并渲染结构化 diff)

**TS 独有（未移植）：**
- 完整 session-level file rewind UI / snapshot 管线

---

### 1.4 FileReadTool — 已补齐主要 Full Build 差距 (原 54% 缩减)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 1,602 (5 文件) | 1,214 (1 文件) |
| 文件 | — | `tools/file_read.rs` |

**Rust 保留：**
- 文本文件读取 + 行号
- 二进制文件检测
- offset/limit 分页
- ✅ **图片文件** (.png/.jpg/.gif/.bmp/.webp → base64 编码, .svg → 文本读取)
- ✅ **PDF 文件** (pages 参数, 通过 `pdftotext` 子进程提取文本)
- ✅ **Jupyter notebook** (.ipynb JSON 解析, cell 类型/源码/输出提取)
- ✅ **符号链接解析** (canonicalize + `resolved_path` / `symlink_resolved` 元数据)
- ✅ **大文件智能分页** (默认窗口 + `next_offset` 续读提示)
- ✅ **文件编码检测** (UTF-8/UTF-8 BOM、UTF-16 LE/BE BOM、UTF-16 无 BOM 启发式、UTF-8 lossy fallback)

**TS 独有（未移植）：**
- （主要读取路径已对齐；后续差异按 `IMPLEMENTATION_GAPS.md` 新条目登记）

---

### 1.5 GrepTool — 53% 缩减 (ripgrep + multiline 已补全)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 795 (3 文件) | 371 (1 文件) |
| 文件 | — | `tools/grep.rs` |

**Rust 保留：**
- 正则搜索 (regex crate 内置 + rg 子进程)
- 上下文行 (-A/-B/-C)
- output_mode: content/files_with_matches/count
- ✅ **ripgrep 子进程调用** (自动检测 `rg`, 构建完整参数, 失败时回退内置 regex)
- ✅ **多行匹配模式** (`multiline: true` → rg `-U --multiline-dotall`)
- ✅ **offset 分页** (跳过前 N 条结果)

**TS 独有（未移植）：**
- (核心功能已基本对齐)

---

### 1.6 FileWriteTool — 已补齐 Full Build 差距 (原 82% 缩减)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 856 (3 文件) | 429 + 683 (2 文件) |
| 文件 | — | `tools/file_write.rs` + `tools/fs/safe_write.rs` |

**Rust 保留：**
- 路径验证 + 内容写入
- 父目录自动创建
- ✅ 安全文件写入 (先写临时 → rename)
- ✅ 文件备份/恢复
- ✅ 二进制内容检查
- ✅ 文件大小限制
- ✅ 权限保持

**TS 独有（未移植）：**
- （主要写入安全路径已对齐；后续差异按 `IMPLEMENTATION_GAPS.md` 新条目登记）

---

## 2. A 级简化 (50-80% 缩减)

### 2.1 SkillTool — 核心已补齐 (原 69% 缩减)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 1,477 (4 文件) | 454 + 2,125 (4 文件) |
| 文件 | — | `tools/skill.rs` + `crates/cc-skills/src/{lib,loader,bundled}.rs` |

**Rust 保留：**
- 技能查找 (registry 查询)
- 参数替换 ($ARGUMENTS, ${NAME})
- new_messages 注入 (inline 上下文)
- fork 上下文 fallback
- ✅ 技能依赖解析
- ✅ 技能热重载 (`/skills reload`)
- ✅ 技能版本管理 / 冲突检测 / app 兼容版本
- ✅ frontmatter 诊断与关键字段校验

**TS 独有（未移植）：**
- MCP skill builder (如后续需要，按插件/脚手架能力单独立项)

---

### 2.2 TaskTools — 58% 缩减

| | TypeScript | Rust |
|---|---|---|
| 行数 | 1,561 (15 文件, 6 工具) | 2,604 (1 文件) |
| 文件 | — | `tools/tasks.rs` |

**Rust 保留：**
- TaskStore（持久化 task records + 进程内索引）
- 6 个操作: Create/Get/Update/List/Stop/Output
- ✅ 磁盘持久化、输出 sidecar 与重启 interruption recovery
- ✅ 依赖字段、agent/supervisor/isolation 元数据
- ✅ 后台 local-agent runtime cancellation token
- ✅ `TaskOutput` `block` / `timeout` 与 `retrieval_status` (`success` / `timeout` / `not_ready`)
- ✅ 上游 task type taxonomy：`local_bash` / `local_agent` / `remote_agent` / `in_process_teammate` / `local_workflow` / `monitor_mcp` / `dream`，并迁移历史 alias
- ✅ remote/multi-type supervisor 元数据底座：`tool_use_id`、`remote_task_type`、`remote_session_id`、`remote_task_metadata`、`poll_started_at`
- ✅ remote restart recovery marker：重启后携带 remote identity 的未完成任务恢复为 `recoverable`，并由 `TaskOutput` 保持等待/未就绪语义
- ✅ remote restore poll timer reset：恢复 remote task 时重置 `poll_started_at`，避免 remote review 离线后立即超时
- ✅ remote review timeout guard：读取/list 时刷新 `ultrareview` / `isRemoteReview` active 任务，超过 `poll_started_at + 30min` 后持久化为 `failed`

**TS 独有（未移植）：**
- 远程/多类型后台任务 poller/reconnect runtime parity
- 复杂后台任务 auto-background 细节

---

### 2.3 ToolSearchTool — 57% 缩减

| | TypeScript | Rust |
|---|---|---|
| 行数 | 593 (3 文件) | 254 (1 文件) |
| 文件 | — | `tools/tool_search.rs` |

**Rust 保留：**
- `select:` 精确选择模式
- 关键字模糊搜索
- 结果限制 (max_results)

**TS 独有（未移植）：**
- TF-IDF 排名算法
- 工具描述全文索引
- deferred tool schema 加载

---

### 2.4 PlanMode — 缩减实现（计划持久化 / 实现关联 / 团队审批子项已补）

| | TypeScript | Rust |
|---|---|---|
| 行数 | 934 (8 文件) | `tools/plan_mode.rs` + `plan_workflow.rs` + `cc-types::plan_workflow` + IPC/daemon/tasks 接线 |
| 文件 | — | `tools/plan_mode.rs`, `plan_workflow.rs`, `tools/tasks.rs`, `crates/cc-types/src/plan_workflow.rs` |

**Rust 保留：**
- 完整状态转换 (save/restore pre_plan_mode)
- agent 上下文阻止
- 重复进入检测
- 用户确认退出
- 保守 classifier gate（IPC / daemon 用户入口的显式关键词触发）
- `.cc-rust/current-plan-workflow.json` 持久化 `PlanWorkflowRecord`
- approval lifecycle、trace 与 plan_text 记录
- approved/implementing 计划会在 `TaskCreate` 后关联 task id，并推进到 implementing
- 团队审批 mailbox flow：`plan_approval_request` / `plan_approval_response` 会更新 teammate `awaiting_plan_approval`、`permission_mode`，并把审批结果注入下一轮
- Plan mode 专用计划文件写入白名单：仅当前解析出的 plan 文件可由 `Write` / `Edit` / `FileWrite` / `FileEdit` 维护，其余非只读工具仍被 Stage 3c 拒绝

**TS 独有（未移植）：**
- full auto-mode LLM classifier parity

---

### 2.5 LSP 工具 + 服务 — 已补齐 Full Build 核心差距 (原 56% 缩减)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 2,005 (6 文件) | 752 + 3,024 (5 文件) |
| 文件 | — | `tools/lsp.rs` + `lsp_service/{mod,transport,client,conversions}.rs` |

**Rust 保留：**
- 9 种 LSP 操作（全部实现，通过 JSON-RPC over stdio 与语言服务器通信）
- JSON-RPC 传输层（Content-Length 帧协议）
- LSP 客户端生命周期管理（初始化握手、请求路由、文件同步）
- `lsp-types` 0.97 协议类型 → 内部类型转换
- 6 种语言服务器支持（rust-analyzer, typescript-language-server, pylsp, gopls, clangd, jdtls）
- 按需懒启动 + 崩溃自动重启
- ✅ 增量文档同步 (`textDocument/didChange`)
- ✅ 被动诊断反馈 (`publishDiagnostics`)
- ✅ 补全建议 (`textDocument/completion`)

**TS 独有（未移植）：**
- 插件侧 LSP 配置整合如后续需要，按插件配置入口单独登记

---

### 2.6 WebFetchTool — 缩减实现（redirect policy 子项已补）

| | TypeScript | Rust |
|---|---|---|
| 行数 | 1,131 (5 文件) | 975 (1 文件) |
| 文件 | — | `tools/web_fetch.rs` |

**Rust 保留：**
- HTTP GET 请求 (reqwest)
- HTML → text 转换
- URL 验证
- 响应截断
- 缓存
- ✅ redirect budget / cross-host diagnostic：最多 10 跳，仅同源/`www.` 变体自动跟随；跨站 redirect 返回目标 URL 诊断
- ✅ Content-Type 基础分发：HTML 提取文本、JSON pretty-print、文本型 MIME 直出、二进制 MIME 拒绝进入模型上下文
- ✅ 环境代理支持：`HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` 及 `NO_PROXY` 基础绕过规则
- ✅ Cookie/credential 安全边界：对齐上游未支持 cookie/credential URL 的边界，拒绝 embedded username/password

**TS 独有（未移植）：**
- JavaScript 渲染 (headless browser)

---

## 3. B 级简化 (30-50% 缩减)

### 3.1 终端 UI — 94% 缩减 (框架差异)

| | TypeScript | Rust |
|---|---|---|
| 行数 | 54,049 (components + ink + hooks) | 3,165 (ui/) |

> 注: 大部分缩减来自 React/Ink → ratatui 的框架差异，非功能遗漏。

**Rust 保留：**
- app.rs (409) — 主应用循环
- keybindings.rs (425) — 键绑定
- vim.rs (847) — Vim 模式
- messages.rs (404) — 消息渲染
- markdown.rs (259) — Markdown 渲染
- prompt_input.rs (250) — 输入框
- permissions.rs (244) — 权限对话框
- theme/diff/spinner

**TS 独有（框架差异，无法直接对比）：**
- React 组件树 (113 个组件)
- Ink 终端渲染引擎 (44 文件)
- React Hooks (83 个)
- 任务面板 UI
- MCP 审批对话框
- 团队管理面板
- 设置向导

---

### 3.2 认证 — 部分实现

| | TypeScript | Rust |
|---|---|---|
| 文件 | — | `auth/mod.rs` + `api_key.rs` + `token.rs` |

**Rust 保留 (活跃路径)：**
- API Key 验证/存储/加载 (`ANTHROPIC_API_KEY`)
- External Token (`ANTHROPIC_AUTH_TOKEN`)
- keyring 集成 (feature-gated)

**接口保留但未实现 (`#[allow(dead_code)]`, `bail!`)：**
- OAuth 登录流程
- Token 刷新
- Token 持久化

---

### 3.3 API 提供商 — 部分实现

**活跃实现：**
- Anthropic Direct (完整)

**接口保留但未实现 (`unimplemented!`)：**
- AWS Bedrock
- GCP Vertex AI

---

### 3.4 遥测/远程 — 仅接口

| 模块 | 文件 | 行数 | 状态 |
|---|---|---|---|
| 遥测 | `analytics/mod.rs` | 54 | 本地日志接口，无网络发送 |
| 远程会话 | `remote/session.rs` | 27 | 接口存根 (`bail!`) |
| LSP 服务 | `lsp_service/{mod,transport,client,conversions}.rs` | ~1,200 | **完整实现** — JSON-RPC 传输 + 9 操作 + 6 语言服务器 |

---

## 4. 简化总结

### 按缩减率排序

| 模块 | TS 行数 | Rust 行数 | 缩减率 | 级别 |
|------|---------|-----------|--------|------|
| state (→ types) | ~58,000 | 832 | 99% | S |
| skills/ | ~43,000 | 989 | 98% | S |
| BashTool | 12,411 | 3,077 + sandbox runner 973 | 75% | S (截断 / heredoc 校验 / Git 操作跟踪 / 进程树终止 / 危险命令拒绝列表 / security validator 高风险与 AST 启发式批次 / 参数绑定安全规则 / New-Object TypeName CLM / sandbox FS preflight / fail-closed 用户面已补全) |
| utils/ | 90,813 | 2,857 | 97% | S |
| AgentTool | 6,072 | 789 | 87% | S (worktree 已补全) |
| UI (全部) | 54,049 | 3,165 | 94% | S (框架) |
| permissions/ | 9,409 | 959 | 90% | S |
| FileEditTool | 1,812 | 1,001+ | 45% | A (fuzzy + 安全/历史/缩进/transcript 子项已补全) |
| FileReadTool | 1,602 | 1,214 | 24% | Full-build 差距已补齐 |
| FileWriteTool | 856 | 1,112 | +30% | Full-build 差距已补齐 |
| GrepTool | 795 | 371 | 53% | A (rg+multiline 已补全) |
| SkillTool | 1,477 | 2,579 | +75% | 核心已补齐 |
| TaskTools | 1,561 | 648 | 58% | A |
| ToolSearchTool | 593 | 254 | 57% | A |
| LSP | 2,005 | 3,776 | +88% | Full-build 核心差距已补齐 |
| WebFetchTool | 1,131 | 553 | 51% | A |
| PlanMode | 934 | 432 | 54% | A |

### 主要简化原因分布

| 原因 | 涉及模块 | 影响 |
|------|---------|------|
| **框架差异** (React→无框架) | UI, state, hooks | ~110,000 行差异 |
| **平台功能未移植** | PowerShell, sandbox, native installer | ~14,000 行 |
| **网络功能降优先** | bridge, voice, telemetry, remote | ~20,000 行 |
| **高级特性省略** | JS render, OAuth (fuzzy match 已补全) | ~10,000 行 |
| **边缘 case 精简** | 各工具的错误恢复/重试/UI | 分散 |

### 优先补全建议

~~以下简化项已补全：~~ (2026-04-01)

1. ~~**FileEditTool fuzzy 匹配**~~ ✅ 已补全 — 230→386 行, `similar::TextDiff` 滑动窗口
   - 2026-05-05 继续补齐读后冲突检测 — `Read` / `Edit` 共享文件快照，防止覆盖未读或读后外部修改的文件
   - 2026-05-05 继续补齐文件锁/readonly 写前检查 — `Edit` 提前拒绝锁定或不可写目标
   - 2026-05-05 继续补齐编辑历史备份 — `Edit` 覆盖前创建恢复备份并返回 `edit_history.backup_path`
   - 2026-05-05 继续补齐自动缩进修正 — `Edit` 精确匹配失败时可映射唯一缩进等价块，歧义候选拒绝
2. ~~**BashTool 输出截断**~~ ✅ 已补全 — 199→430 行, head+tail 行级截断
   - 2026-05-05 继续补齐 heredoc 校验 — BashTool + cc-utils bash helper 共 1,730 行
   - 2026-05-05 继续补齐 Git 操作跟踪 — 新增 `cc-utils/src/git_operation_tracking.rs`，Bash/PowerShell 结果附带 `git_operations`
   - 2026-05-05 继续补齐进程树终止 / 取消语义 — 新增 `tools/exec/process_control.rs`，Bash/PowerShell 共享超时和 abort 终止路径
   - 2026-05-05 继续补齐危险命令拒绝列表 — `cc-permissions/src/dangerous.rs` 增加上游 destructive warning 覆盖面，并让 PowerShell 执行安全门调用专用检测
   - 2026-05-05 继续补齐 PowerShell security validator 高风险规则 — 覆盖 eval、nested PowerShell、download cradle、Add-Type、COM、Start-Process；WMI/CIM 对 `Invoke-WmiMethod`/`iwmi`/`Invoke-CimMethod` 任意调用 fail-closed，避免动态 Class/MethodName 绕过
   - 2026-05-05 继续补齐 PowerShell security validator 高风险规则（二）— 覆盖 standalone download utilities、script file execution、ForEach-Object MemberName、Invoke-Item、scheduled task、env/module/runtime-state mutation
   - 2026-05-05 继续补齐 PowerShell security validator 目标语法规则 — 覆盖动态 IEX、危险 script block、ForEach script block、stop-parsing、明显危险 static method
   - 2026-05-05 继续补齐 PowerShell security validator AST 启发式规则 — 覆盖一般 dynamic command name、dot-sourced dynamic command、subexpression、expandable string、splatting、member/static member invocation、非 CLM allowlist type literal
   - 2026-05-05 继续补齐 PowerShell `nameType=application` 命令名 guard 子集 — 拒绝直接执行本地脚本/路径应用（如 `.\payload.ps1`、`scripts\Out-Null.ps1`、路径限定 `.exe`）
   - 2026-05-05 继续补齐 PowerShell 参数绑定安全规则子项 — 覆盖 Start-Process 冒号绑定 RunAs、位置脚本文件参数与 ForEach-Object 位置 MemberName
   - 2026-05-05 继续补齐 PowerShell New-Object TypeName CLM 子项 — 覆盖 `-TypeName` / `-t:` / 位置 TypeName 的 CLM allowlist 校验
   - 2026-05-05 继续补齐 PowerShell 原生 parser-invalid fail-closed — `PowerShellTool::validate_input()` 调用 `Parser.ParseInput()` 在执行前拒绝完整 parser errors
   - 2026-05-05 继续补齐 PowerShell 原生 AST metadata 安全门 — `PowerShellTool::validate_input()` 解析 `elementTypes`、colon-bound `children`、raw command name / `nameType` 语义与 statement `securityPatterns`
   - 2026-05-05 继续补齐 sandbox 文件系统 preflight — `cc-sandbox/src/runner.rs` 对 shell 显式写目标执行 read-only/workspace/allowWrite/denyWrite 检查
   - 2026-05-05 继续补齐 sandbox fail-closed 用户面 — `/sandbox require` / `/sandbox optional` 暴露 `sandbox.failIfUnavailable` 会话切换
   - 2026-05-05 重评 Windows Restricted Token / Job Object — 上游 sandbox-runtime/PowerShell UI 当前不支持 Windows sandbox，移入 `IMPLEMENTATION_GAPS.md` §7 Intentional 裁剪；保留 Rust-level FS/network preflight 与 fail-closed 用户面
3. ~~**FileReadTool PDF/图片**~~ ✅ 已补全 — 236→743 行, 图片 base64 + PDF pdftotext + ipynb JSON
   - 2026-05-05 继续补齐 symlink 解析、编码检测、大文件分页 — 743→1,214 行
4. ~~**GrepTool ripgrep 调用**~~ ✅ 已补全 — 185→371 行, rg 子进程 + multiline + offset
5. ~~**AgentTool worktree 隔离**~~ ✅ 已补全 — 322→789 行, 临时 worktree + 变更检测 + 自动清理

**剩余补全建议 (低优先级)：**

1. **ToolSearchTool** — TF-IDF 排名、全文索引
2. **API 提供商** — Bedrock/Vertex 剩余 provider parity
3. **认证** — OAuth 登录流程
