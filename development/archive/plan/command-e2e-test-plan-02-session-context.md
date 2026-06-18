# 命令 E2E 测试计划 02：会话与上下文命令


> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_session.rs`
> 离线和在线测试混合。在线测试标记为 `#[ignore]`。

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 模式 |
|---|---|---|---|
| `/session` | -- | `Output` | 离线 |
| `/resume` | `/sessions`、`/preview` | `Output`/`SwitchSession` | 在线 |
| `/rename` | -- | `Output` | 离线 |
| `/rewind` | -- | `Output` | 在线 |
| `/insights` | -- | `Output` | 在线 |
| `/branch` | `/br` | `Output`/`SwitchSession` | 在线 |
| `/export` | `/markdown-export` | `Output` | 离线 |
| `/audit-export` | `/audit` | `Output` | 离线 |
| `/session-export` | `/sexport`、`/structured-export` | `Output` | 离线 |
| `/add-dir` | -- | `Output` | 离线 |
| `/init` | -- | `Output` | 离线 |

## 测试用例

### T01：`/session` 显示当前会话
```rust
fn session_command_shows_info() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("session")
    // 3. Wait(2s)
    // 4. AssertScreenContains("session")  // 应显示会话 ID 或信息
    // 5. Snapshot("session_info")
    // 断言：屏幕包含会话相关信息
}
```

### T02：`/session list`
```rust
fn session_list_command() {
    // 步骤：
    // 1. Command("session list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic，可能显示 "no sessions" 或列表
}
```

### T03：`/session list all`
```rust
fn session_list_all() {
    // 步骤：
    // 1. Command("session list all")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic
}
```

### T04：`/rename` 设置会话标题
```rust
fn rename_command_sets_title() {
    // 步骤：
    // 1. Command("rename My Test Session")
    // 2. Wait(2s)
    // 3. AssertScreenContains("My Test Session") 或 AssertScreenContains("rename")
    // 断言：重命名操作已确认
}
```

### T05：`/export` 导出到默认路径
```rust
fn export_command_default() {
    // 步骤：
    // 1. Command("export")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 4. Snapshot("export_result")
    // 断言：无 panic，显示导出路径或错误
}
```

### T06：`/export list`
```rust
fn export_list() {
    // 步骤：
    // 1. Command("export list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic
}
```

### T07：`/audit-export` 默认
```rust
fn audit_export_command() {
    // 步骤：
    // 1. Command("audit-export")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：无 panic
}
```

### T08：`/audit-export` 别名 `/audit`
```rust
fn audit_export_alias() {
    // 步骤：
    // 1. Command("audit")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T09：`/session-export` 默认
```rust
fn session_export_command() {
    // 步骤：
    // 1. Command("session-export")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：无 panic
}
```

### T10：`/session-export` 别名 `/sexport`
```rust
fn session_export_alias() {
    // 步骤：
    // 1. Command("sexport")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T11：`/add-dir` 列出当前目录
```rust
fn add_dir_lists_current() {
    // 步骤：
    // 1. Command("add-dir")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示当前附加的工作目录
}
```

### T12：`/add-dir` 带路径
```rust
fn add_dir_with_path() {
    // 步骤：
    // 1. Command("add-dir /tmp")
    // 2. Wait(2s)
    // 3. AssertScreenContains("/tmp") 或 AssertScreenContains("add")
    // 断言：目录已添加或显示错误
}
```

### T13：`/init` 创建配置
```rust
fn init_command() {
    // 步骤：
    // 1. Command("init")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic，显示设置路径
}
```

### T14：`/resume` 无会话时（在线）
```rust
#[ignore = "requires real API key"]
fn resume_no_sessions() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("resume")
    // 3. Wait(3s)
    // 4. AssertNoPanic
    // 断言：无 panic，显示 "no sessions" 或类似信息
}
```

### T15：`/branch` 分叉会话（在线）
```rust
#[ignore = "requires real API key"]
fn branch_command_forks() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Input("Remember the number 7749")
    // 3. WaitForText("7749", API_TIMEOUT)
    // 4. Command("branch test-fork")
    // 5. Wait(3s)
    // 6. Snapshot("after_branch")
    // 7. AssertNoPanic
    // 断言：新会话已创建，无 panic
}
```

### T16：`/rewind` 命令（在线）
```rust
#[ignore = "requires real API key"]
fn rewind_command() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Input("First message")
    // 3. WaitForText("First", API_TIMEOUT)
    // 4. Input("Second message")
    // 5. WaitForText("Second", API_TIMEOUT)
    // 6. Command("rewind")
    // 7. Wait(3s)
    // 8. AssertNoPanic
    // 断言：对话已回退
}
```

### T17：`/insights` 命令（在线）
```rust
#[ignore = "requires real API key"]
fn insights_command() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Input("Hello")
    // 3. WaitForText("hello", API_TIMEOUT)
    // 4. Command("insights")
    // 5. Wait(3s)
    // 6. AssertNoPanic
    // 断言：显示洞察信息
}
```

## 优先级：高
会话管理是核心功能。先执行离线测试，在线测试作为后续补充。

---

## 补充功能说明

以下为各命令的功能细节，供测试断言和用例设计参考。

---

### `/session`

#### 功能描述
显示当前会话信息（ID、标题、工作目录、消息数、模型），并列出当前工作区最近的历史会话。支持 `list` 子命令列出当前工作区会话，`list all` 列出所有工作区会话。`ls` 是 `list` 的别名。

#### 输出示例
```
Current session:

  Session ID:        abc12345-...
  Title (auto):      first prompt text
  Working directory: /path/to/project
  Messages:          42
  Model:             claude-sonnet-4-20250514

Recent workspace sessions (3):

  Session ID                             Msgs  Last Modified         Title / Directory
  ----------                             ----  -------------         -----------------
  def67890-...                              8  2026-05-20 14:30      * My custom title
  ghi11111-...                             15  2026-05-19 09:15        /path/to/project

Use /resume <session_id> to load one of these sessions.
```

无会话时输出 `No previous sessions found for this workspace.`。未知子命令输出 `Unknown session subcommand: '<arg>'`。

#### 源码路径
`crates/cc-commands/src/session.rs`

#### 边界情况
- 空会话（0 条消息）时消息数显示为 0
- 列表最多显示 10 条（无参数）或 20 条（`list`），超出部分显示 `... and N more`
- 自定义标题（`custom_title`）前带 `* ` 标记，截断到 60 字符
- 当前 session 不会出现在历史列表中（被过滤）
- 未知名令子命令返回用法提示

---

### `/resume`

#### 功能描述
恢复之前的会话。无参数时恢复当前工作区最近的会话；传 session ID 时先精确匹配，再前缀匹配；优先匹配当前工作区，再匹配全局。支持 `recent` 关键字作为"无参数"别名。匹配到多个会话时返回候选列表供用户进一步选择。成功恢复时返回 `SwitchSession` 并加载历史消息。

#### 输出示例
```
Loaded history from session abc12345-... (15 messages) into the current conversation.
```

找不到时输出：
```
No previous session found for workspace: /path/to/project
```
或：
```
Session 'xyz' was not found.
Use /session or /session list to see available sessions.
```

前缀匹配多个时：
```
Found 3 sessions matching 'ab'. Please be more specific:

  abc12345-... (15 msgs, 2026-05-20 14:30)
  abc67890-... (8 msgs, 2026-05-19 09:15)
```

空会话恢复时输出 `no saved conversation messages`。

#### 源码路径
`crates/cc-commands/src/resume.rs`

#### 边界情况
- 无参数 + 无历史会话 → 友好提示
- session ID 为当前 session 时被排除，不会恢复自身
- 空消息的 session → 提示无消息，不加载
- `recent` 关键字与无参数行为相同
- 前缀模糊匹配最多显示 10 个候选
- 恢复时如果 session 关联了 team context，会更新 `app_state.team_context`

---

### `/rename`

#### 功能描述
管理当前会话的自定义标题。无参数显示当前标题；传文本设置自定义标题并持久化到 session 文件；`--clear` 移除自定义标题回退到自动派生标题；`--auto` 将自动派生标题锁定为自定义标题。如果 session 文件尚未存在于磁盘，会先触发一次持久化。

#### 输出示例
```
Session title set to: My Test Session
```

显示当前标题：
```
Current title (auto): first prompt text

Use /rename <new title> to pin a custom title,
or /rename --auto to lock in the current auto-derived title.
```

清除时：
```
Custom title cleared. Falling back to: first prompt text
```

无自定义标题时清除：
```
No custom title to clear. The title is already auto-derived.
```

#### 源码路径
`crates/cc-commands/src/rename.rs`

#### 边界情况
- 空字符串标题被拒绝，提示 `Title cannot be empty`
- 自动派生标题取第一个非 meta 用户消息的首行，截断到 80 字符并加 `…`
- 纯空格/空内容的用户消息被跳过
- `--clear` / `-c` / `clear` 均可用
- `--auto` / `-a` / `auto` 均可用
- `--clear` 在没有自定义标题时返回 "already auto-derived" 提示

---

### `/rewind`

#### 功能描述
将对话回退到较早的用户轮次。无参数或 `list` 时显示带编号的用户轮次列表；`<n>` 保留前 n 个用户轮次（含对应 assistant/tool 消息）；`--to <uuid>` 按 UUID 前缀回退到指定轮次。回退时同步截断内存消息和磁盘 session 文件，并创建 `*.rewind-<ts>.json` 备份。

#### 输出示例
```
Conversation turns (3):

    1. [a1b2c3d4] 14:30:05 — first prompt text
    2. [e5f6g7h8] 14:31:10 — second prompt text
    3. [i9j0k1l2] 14:32:20 — third prompt text

Use /rewind <n> or /rewind --to <uuid-prefix> to rewind.
A backup of the pre-rewind session is written next to the session file.
```

回退成功：
```
Rewound to turn 2 of 3 — kept 4 messages (was 6). (disk session truncated, backup saved)
```

#### 源码路径
`crates/cc-commands/src/rewind.rs`

#### 边界情况
- 空对话 → `No user turns to rewind — the conversation is empty.`
- `0` 清空所有消息并截断磁盘 session
- n 超出范围 → `Turn N is out of range — there are only M turns.`
- 回退到最后一个轮次 → `already the last turn — nothing to rewind.`
- tool_use_result 用户消息和 meta 用户消息不计入轮次
- `--to` 无参数时返回用法提示
- `--to <prefix>` 匹配多个时列出歧义结果
- 未识别参数返回用法帮助

---

### `/insights`

#### 功能描述
跨会话的历史分析与统计报告。无参数时分析所有工作区的会话；`this` / `workspace` / `ws` 限定当前工作区；`recent [days]` 筛选最近 N 天（默认 30 天）；`fast` 跳过 session body 扫描仅读元数据。报告包含会话数、消息数、token 用量、估算成本、工作区分布和最大会话排名。

#### 输出示例
```
Session insights (all workspaces)

  Sessions included:   12
  Sessions filtered:   3
  Total messages:      450
  User turns:          120
  Assistant turns:     150
  Activity span:       2026-04-01 10:00 -> 2026-05-20 14:30

  Token usage:
    Input:             1,234,567
    Output:            234,567
    Cache read:        500,000
    Cache creation:    100,000
  Estimated cost:      $12.34

  Sessions by workspace:
    /path/to/project1                  8
    /path/to/project2                  4

  Largest sessions:
    [abc12345]  200 msgs  2026-05-20  Implement feature X
```

无匹配会话时：`No sessions match the current filter. Try /insights or /session list.`

#### 源码路径
`crates/cc-commands/src/insights.rs`

#### 边界情况
- 无历史会话 → 友好提示
- `recent abc` 非数字参数 → 返回错误
- `recent 0` 或负数 → `must be a positive integer`
- `fast` 模式下 token 用量和成本不显示，显示 `Usage stats skipped` 提示
- 消息数不足 2 的 session 被过滤（`min_messages: 2`）
- 工作区分布最多显示 8 个
- 最大会话排名最多显示 5 个
- 未知子命令显示用法帮助

---

### `/branch`

#### 功能描述
将当前对话 transcript fork 到一个新 session，立即切换到新 session。不同于 `/gbranch`（git 分支操作），`/branch` 是会话级别的分叉。新 session 保留所有消息内容，写入 `forked_from` 元数据，返回 `SwitchSession` 使运行时切换到新会话。不接受参数（传参时提示是否想用 `/gbranch`）。

#### 输出示例
```
Branched conversation. You are now in the new branch (session abc12345-...).
Use /resume def67890-... to return to the original conversation.
From a terminal, run: cc-rust -r def67890-...
```

传参时：
```
/branch takes no arguments and forks the current conversation.
Did you mean `/gbranch feature/foo` (git branch wrapper)?
```

#### 源码路径
`crates/cc-commands/src/branch.rs`

#### 边界情况
- 空对话也能 fork（0 条消息的 session）
- 传参数时返回 `Output` 提示 `/gbranch` 而非报错
- 返回类型为 `SwitchSession`，包含新 session_id 和完整消息副本
- 父 session 的 transcript 文件不受影响（完整保留）
- 新 session 的 session_header 包含 `forked_from` 和 `forked_at_uuid` 元数据

---

### `/export`

#### 功能描述
将对话导出为 Markdown 格式。无参数导出当前 session 到默认目录 `~/.cc-rust/exports/`；`list` 列出已有导出文件；参数包含 `/`、`\` 或以 `.md` 结尾时视为路径；否则视为 session ID 进行精确或前缀匹配。

#### 输出示例
```
Session exported to: /home/user/.cc-rust/exports/session_abc12345.md
```

列出：
```
Exported sessions (2):
  session_abc12345.md
  session_def67890.md
```

无导出时：`No exports found. Use /export to export the current session.`

找不到 session：`No session found matching 'xyz'. Use /session list to see available sessions.`

#### 源码路径
`crates/cc-commands/src/export.rs`

#### 边界情况
- 空消息 session 也能导出（生成空 markdown）
- 路径检测启发式：`/`、`\` → 文件路径；`.md` 后缀 → 文件路径；其他 → session ID
- session ID 前缀匹配支持精确优先
- `list` 在无导出文件时返回友好提示而非报错

---

### `/audit-export`

#### 功能描述
导出可校验的审计记录（JSON 格式），或校验已有审计文件的完整性。无参数导出当前 session；`list` 列出审计文件；`verify <path>` 校验审计文件完整性并报告 PASS/FAIL；参数包含 `/`、`\` 或以 `.json` 结尾时视为路径；否则视为 session ID。

#### 输出示例
```
Audit record exported to: /home/user/.cc-rust/audits/session_abc12345.audit.json
Use `/audit-export verify /home/user/.cc-rust/audits/session_abc12345.audit.json` to verify integrity.
```

校验结果：
```
[PASS] Integrity verified.
  Entries: 42
```

或：
```
[FAIL] Chain broken at entry 5.
  Entries: 42
  First broken at: entry #5
```

#### 源码路径
`crates/cc-commands/src/audit_export.rs`

#### 边界情况
- `verify` 无路径参数 → 返回 `Usage: /audit-export verify <path>`
- 路径检测启发式与 `/export` 类似但后缀为 `.json`
- session ID 匹配失败返回友好提示
- 别名 `/audit` 可用

---

### `/session-export`

#### 功能描述
导出结构化 session JSON 数据包，包含原始 transcript、API 请求记录、工具调用、压缩历史、token 使用摘要和成本信息。支持子命令 `list`、路径导出和 session ID 导出，参数识别规则与 `/audit-export` 相同（包含 `/`、`\` 或 `.json` 后缀视为路径）。

#### 输出示例
```
Session exported to: /home/user/.cc-rust/exports/session_abc12345.json

Messages: 42
  User: 15, Assistant: 18, System: 1
Tool calls: 8
  Tools used: Read, Edit, Bash
API requests: 20
Compactions: 2
Tokens: ~50000 / 200000 (25.0%)
Cost: $1.2345
```

#### 源码路径
`crates/cc-commands/src/session_export.rs`

#### 边界情况
- 摘要中成本为 0 时不显示 `Cost` 行
- token 利用率百分比按 `tokens / window * 100` 计算
- 无工具调用时 `Tools used` 行不显示
- session ID 匹配失败返回友好提示
- 别名 `/sexport`、`/structured-export` 均可用

---

### `/add-dir`

#### 功能描述
管理额外工作目录，使工具可访问主工作目录之外的文件。无参数列出当前已添加的目录；传路径时解析（支持相对路径和 `~` 扩展）、验证目录存在性、检查是否已在 CWD 内或已添加，然后加入 `additional_working_directories`。

#### 输出示例
```
Added working directory: other-repo (/home/user/projects/other-repo)
```

列出：
```
Additional working directories:
  other-repo → /home/user/projects/other-repo
```

无目录时：`No additional working directories.`

#### 源码路径
`crates/cc-commands/src/add_dir.rs`

#### 边界情况
- 路径不存在 → `Directory not found: '<path>'`
- 路径是文件而非目录 → `'<path>' is not a directory.`
- 目录已在 CWD 内 → `is already within the current working directory`
- 目录已添加 → `is already added`
- `~` 扩展依赖 `dirs::home_dir()`
- 新增目录默认不是只读（`read_only: false`）
- 目录名从路径最后一段派生

---

### `/init`

#### 功能描述
在当前工作目录下初始化项目配置。创建 `.cc-rust/settings.json`（内容为 `{"model": null, "theme": null}`）和 `CLAUDE.md` 模板文件。如果 `CLAUDE.md` 已存在则完全跳过（不覆盖）；如果 `settings.json` 已存在则跳过该文件但继续创建其他文件。

#### 输出示例
```
Project initialization complete.
Created: /path/to/project/.cc-rust/settings.json, /path/to/project/CLAUDE.md
```

已存在时：
```
CLAUDE.md already exists here. Skipping /init to avoid overwriting it.
```

部分存在时：
```
Project initialization complete.
Already present: /path/to/project/.cc-rust/settings.json
```

#### 源码路径
`crates/cc-commands/src/init.rs`

#### 边界情况
- `CLAUDE.md` 已存在 → 整个命令跳过，输出 "already exists" 提示，不创建/覆盖任何文件
- `settings.json` 已存在但 `CLAUDE.md` 不存在 → 跳过 settings，创建 CLAUDE.md
- `settings.json` 已存在且 `CLAUDE.md` 也存在 → 完全跳过
- CLAUDE.md 模板内容包含 "cc-rust" 和 "Project instructions"
- 无参数（`_args` 参数被忽略）
