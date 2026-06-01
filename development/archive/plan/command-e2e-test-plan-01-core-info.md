# 命令 E2E 测试计划 01：核心与信息命令（离线）

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_core_info.rs`
> 所有测试均为**离线**（无需 API 密钥），使用 `--permission-mode bypass`。
>
> 已隐藏命令（不在此计划中测试）：`/extra-usage`、`/rate-limit-options`、`/config set model`

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 备注 |
|---|---|---|---|
| `/exit` | `/quit`、`/q` | `Exit` | 触发 REPL 退出 |
| `/version` | `/v` | `Output` | 已测试，包含别名测试 |
| `/config` | `/settings` | `Output` | `show`、`set`、`reset` 子命令 |
| `/debug` | -- | `Output` | 调试信息转储 |
| `/effort` | -- | `Output` | 无参数显示当前值；`low/medium/high` 设置 |
| `/fast` | -- | `Output` | 切换，`on/off/status` |
| `/context` | `/ctx` | `Output` | 上下文 token 估算 |
| `/files` | -- | `Output` | 列出引用的文件 |
| `/copy` | `/cp` | `Output` | 复制最后一条助手消息 |
| `/compact` | -- | `Output` | 压缩对话 |
| `/keybindings` | `/keys`、`/shortcuts` | `Output` | 快捷键显示 |
| `/statusline` | `/status-line` | `Output` | 状态栏配置 |
| `/terminal-setup` | `/term-setup`、`/terminal` | `Output` | 终端诊断 |
| `/doctor` | `/diagnostics`、`/diag` | `Output` | 聚合诊断 |
| `/experimental` | `/experiments`、`/exp` | `Output` | 功能门控检查 |

## 测试用例

### T01：`/exit` 触发 REPL 退出
```rust
fn exit_command_quits_repl() {
    // 步骤：
    // 1. 使用 default_args() 启动 PTY
    // 2. skip_trust_gate
    // 3. Command("exit")
    // 4. Wait(2s)
    // 5. AssertNoPanic
    // 6. finish() — 进程应正常退出
    // 断言：输出中不包含 "panicked"，进程退出
}
```

### T02：`/exit` 别名 `/q` 和 `/quit`
```rust
fn exit_aliases_work() {
    // 分别在独立启动中测试每个别名：
    // - "/q" 触发退出
    // - "/quit" 触发退出
    // 断言：两者均正常退出且无 panic
}
```

### T03：`/config` 显示（无参数）
```rust
fn config_show_displays_settings() {
    // 步骤：
    // 1. 启动 PTY
    // 2. skip_trust_gate
    // 3. Command("config")
    // 4. Wait(2s)
    // 5. AssertScreenContains("model")  // 设置显示应展示模型
    // 6. Snapshot("config_show")
    // 断言：屏幕包含配置相关文本（model、backend、theme）
}
```

### T04：`/config reset`
```rust
fn config_reset_restores_defaults() {
    // 步骤：
    // 1. 启动 PTY
    // 2. skip_trust_gate
    // 3. Command("config set model dummy")
    // 4. Wait(1s)
    // 5. Command("config reset")
    // 6. Wait(2s)
    // 7. Snapshot("after_reset")
    // 断言：无 panic，屏幕显示重置确认或默认值
}
```

### T05：`/config` 别名 `/settings`
```rust
fn config_alias_settings() {
    // 步骤：
    // 1. Command("settings")
    // 2. Wait(2s)
    // 3. AssertScreenContains("model")
    // 断言：别名效果与 /config 相同
}
```

### T06：`/debug` 显示调试信息
```rust
fn debug_command_shows_info() {
    // 步骤：
    // 1. Command("debug")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Snapshot("debug_output")
    // 断言：屏幕非空，无 panic
}
```

### T07：`/effort` 显示当前 effort
```rust
fn effort_shows_current() {
    // 步骤：
    // 1. Command("effort")
    // 2. Wait(2s)
    // 3. AssertScreenContains("effort")
    // 断言：显示当前 effort 级别
}
```

### T08：`/effort high`
```rust
fn effort_set_high() {
    // 步骤：
    // 1. Command("effort high")
    // 2. Wait(2s)
    // 3. AssertScreenContains("high")
    // 断言：effort 级别已变更为 high
}
```

### T09：`/effort` 无效值被拒绝
```rust
fn effort_rejects_invalid() {
    // 步骤：
    // 1. Command("effort extreme")
    // 2. Wait(2s)
    // 3. AssertScreenContains("low") 或 AssertScreenContains("medium") 或 AssertScreenContains("high")
    //    （错误信息应提到有效值）
    // 断言：无效 effort 被拒绝并给出有用的提示信息
}
```

### T10：`/fast` 切换
```rust
fn fast_toggle() {
    // 步骤：
    // 1. Command("fast status")
    // 2. Wait(1s)
    // 3. Snapshot("fast_initial")
    // 4. Command("fast on")
    // 5. Wait(2s)
    // 6. AssertScreenContains("fast")
    // 7. Command("fast off")
    // 8. Wait(2s)
    // 断言：切换操作无 panic
}
```

### T11：`/context` 显示上下文信息
```rust
fn context_command_shows_usage() {
    // 步骤：
    // 1. Command("context")
    // 2. Wait(2s)
    // 3. AssertScreenContains("token") 或 AssertScreenContains("context")
    // 断言：显示上下文用量
}
```

### T12：`/context` 别名 `/ctx`
```rust
fn context_alias_ctx() {
    // 步骤：
    // 1. Command("ctx")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T13：`/files` 列出引用的文件
```rust
fn files_command_works() {
    // 步骤：
    // 1. Command("files")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic（新会话中可能显示 "no files"）
}
```

### T14：`/copy` 在无助手消息时不崩溃
```rust
fn copy_no_crash_fresh_session() {
    // 步骤：
    // 1. Command("copy")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无助手消息时不 panic
}
```

### T15：`/copy` 别名 `/cp`
```rust
fn copy_alias_cp() {
    // 步骤：
    // 1. Command("cp")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T16：`/compact` 在空会话时不崩溃    长对话能否有效压缩(可以在session中插入大量文本测试)
```rust
fn compact_no_crash_empty() {
    // 步骤：
    // 1. Command("compact")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：空对话时不 panic
}
```

### T17：`/keybindings` 显示快捷键
```rust
fn keybindings_command() {
    // 步骤：
    // 1. Command("keybindings")
    // 2. Wait(2s)
    // 3. AssertScreenContains("Ctrl") 或 AssertScreenContains("key")
    // 断言：显示快捷键
}
```

### T18：`/statusline` 命令
```rust
fn statusline_command() {
    // 步骤：
    // 1. Command("statusline")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic
}
```

### T19：`/terminal-setup` 命令
```rust
fn terminal_setup_command() {
    // 步骤：
    // 1. Command("terminal-setup")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic，显示终端信息
}
```

### T20：`/doctor` 诊断
```rust
fn doctor_command() {
    // 步骤：
    // 1. Command("doctor")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 4. Snapshot("doctor")
    // 断言：无 panic，显示诊断信息
}
```

### T21：`/doctor` 别名 `/diagnostics`、`/diag`
```rust
fn doctor_aliases() {
    // 测试 "/diagnostics" 和 "/diag"
    // 断言：两者均可用且无 panic
}
```

### T22：`/experimental` 功能门控
```rust
fn experimental_command() {
    // 步骤：
    // 1. Command("experimental")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无 panic，显示功能门控状态
}
```

### T23：所有别名批量测试
```rust
fn all_core_aliases_batch() {
    // 按顺序执行：/v、/settings、/ctx、/cp、/keys、/diag、/exp
    // 每个执行后 AssertNoPanic
    // 断言：所有别名均可解析且无 panic
}
```

## 优先级：高
这些是最基本的命令，应始终可用。优先实施。

---

## 补充功能说明

以下为本文件涵盖的每条命令的补充功能细节，供编写和审查测试用例时参考。

---

### `/exit` (`/quit`、`/q`)

**功能描述**
退出 REPL。从预定义的告别语列表（"Goodbye!"、"See ya!"、"Bye!"、"Catch you later!"）中随机选取一条，返回 `CommandResult::Exit(message)`，触发 REPL 循环终止。

**输出示例**
```
Goodbye!
```
（或 "See ya!"、"Bye!"、"Catch you later!" 中的任意一条）

**源码路径**
`crates/cc-commands/src/exit.rs` — `ExitHandler`

**边界情况**
- 带参数时仍正常退出，参数被忽略
- 每次执行返回的告别语取决于系统时间纳秒数，不可预测具体是哪一条
- 返回 `Exit` 而非 `Output`，TUI 层需要将此类型映射为进程退出而非文本输出
- 连续调用多次 `/exit` 不应导致 panic（REPL 应在第一次后终止）

---

### `/config` (`/settings`)

**功能描述**
显示、设置或重置配置。无参数或 `show` 子命令时显示当前生效配置及来源；`set <key> <value>` 写入指定键值并持久化到磁盘；`reset` 将内存配置恢复默认值（可选重写文件）。支持 `--user`、`--project`、`--local` 三种写入范围。

**输出示例**
无参数 `/config`：
```
# Settings (effective)
- model: gpt-5.5
- backend: codex
- theme: dark
...
```

`/config set theme dark`：
```
Theme set to: dark
-> persisted to ~/.cc-rust/settings.json (backups kept)
```

`/config reset`：
```
In-memory configuration reset to defaults. Files on disk untouched; pass --user / --project / --local to also rewrite a file.
```

**源码路径**
`crates/cc-commands/src/config_cmd.rs` — `ConfigHandler`，展示逻辑在 `crates/cc-commands/src/config_cmd/show.rs`

**边界情况**
- `model` 和 `effortLevel`、`model_reasoning_effort` 键是只读的，`set` 时返回 "read-only" 提示并引导用户使用 `/login`、`/model`、`/effort`
- `backend` 值会做规范化：只有 `codex` 保留为 `codex`，其他值一律回退为 `native`
- 未知键会被放入 `extra` map，不会报错
- `set` 缺少 `<value>` 时返回 Usage 提示
- `reset` 不带 `--scope` 只重置内存状态，不改磁盘文件
- `set` 的 boolean 类型键只接受 `true`/`false`/`1`/`0`，其他值会报错
- `permissionMode` 设置 `auto` 时需要 `enableAutoMode` 配置许可

---

### `/debug`

**功能描述**
导出调试信息。当前实现仅支持 `snapshot` 子命令，用于在交互式 TUI 中将当前帧导出为带时间戳的文本快照文件到 `target/tui-snapshots/snapshot-*.txt`。无参数时显示用法提示。

**输出示例**
无参数：
```
Usage: /debug snapshot

Exports the current Rust TUI frame to a timestamped target/tui-snapshots/snapshot-*.txt file when run inside the interactive TUI.
```

`/debug snapshot`：
```
TUI debug snapshots are available in the interactive TUI via /debug snapshot or F12.
```

**源码路径**
`crates/cc-commands/src/debug_cmd.rs` — `DebugHandler`

**边界情况**
- 无参数时显示 Usage 而非报错
- 只接受 `snapshot` 作为子命令，其他子命令不匹配
- 在非 TUI 环境（headless、PTY 测试）中，snapshot 子命令仅返回文本提示，不实际执行帧导出
- 不修改任何状态，纯只读命令

---

### `/effort`

**功能描述**
显示或设置模型推理深度（thinking effort）。无参数时显示当前 effort 值和支持的级别列表；有参数时接受 `auto` 或当前模型支持的 reasoning level（如 `low`、`medium`、`high`、`xhigh`）。设置后同时写入 `authProfiles.<active>.modelReasoningEffort` 和 `output_config.effort` 并持久化。

**输出示例**
无参数：
```
Current effort: high (24576 thinking tokens)

Model: gpt-5.5
Usage: /effort <auto|low|medium|high|xhigh>
auto uses the model default: medium.
```

`/effort high`：
```
Effort set to: high (24576 thinking tokens)
-> persisted output_config.effort=high and authProfiles.codex.modelReasoningEffort=high to ~/.cc-rust/settings.json
```

无效值：
```
Invalid effort for gpt-5.5: 'extreme'
Supported levels: auto, low, medium, high, xhigh
```

**源码路径**
`crates/cc-commands/src/effort.rs` — `EffortHandler`

**边界情况**
- 大小写不敏感：`HIGH` 和 `high` 等效
- `auto` 解析为当前模型的 `default_reasoning_level`（codex profile 的 gpt-5.5 默认是 `medium`）
- 数字参数（如 `12000`）不被支持，返回 "Invalid effort"
- `MAX`（全大写）不是有效值，返回 "Invalid effort"（代码中只有 `max` 作为特殊级别但不在此模型的 supported list 中）
- 当前 profile 没有 `model_capabilities` 时，返回 "no configured reasoning levels" 提示
- `output_config.effort` 使用独立的规范化映射：`low` -> `high`，`xhigh` -> `max`

---

### `/fast`

**功能描述**
切换快速模式。无参数时切换 on/off；`on`/`enable` 启用，`off`/`disable` 关闭，`status` 显示状态。启用时如果当前模型不支持 fast mode，会自动切换到 `fastModel`（默认 `MOTA`），并在输出中提示模型切换。

**输出示例**
`/fast status`：
```
Fast mode: disabled
Model: gpt-5.5
Model compatible: true
```

`/fast on`：
```
Fast mode enabled. Output will be generated faster using the same model.
Model switched from 'legacy-model' to 'gpt-5.5' for fast mode.
```

`/fast on`（已启用）：
```
Fast mode is already enabled.
```

**源码路径**
`crates/cc-commands/src/fast.rs` — `FastHandler`

**边界情况**
- 无参数是切换（toggle），不是显示状态
- `on`/`enable` 等效；`off`/`disable` 等效
- 已启用时再 `on` 返回 "already enabled"；已禁用时再 `off` 返回 "already disabled"
- 未知参数（如 `/fast turbo`）返回 "Unknown argument" 和 Usage
- 当前模型和 fast model 都不支持 fast mode 时，返回 "not supported" 错误
- 模型兼容性检查优先查 `settings.model_capabilities`，其次查 `auth_profiles.<active>.model_capabilities`
- 启用后查询循环会设置 `speed: "fast"` 和 `fast-mode-2026-02-01` beta header

---

### `/context` (`/ctx`)

**功能描述**
输出本地启发式的上下文 token 使用估算。运行与实际发送管道相同的 snip + microcompact 变换，将结果分为 messages、system prompt、skills、file cache、tool schemas、hook results、free budget 等类别，以 TUI token 网格或 JSON 格式渲染。支持 `json`/`raw` 子命令获取机器可读输出。

**输出示例**
```
## Estimated Conversation Context

**Model:** gpt-5.5
**Window:** 200K tokens
**Used:**   1.5K / 200K (0.8%)
**Messages:** 2 in -> 2 after pre-send pipeline

### Breakdown

  messages  ████████████████████████   1.2K   0.6%
  free      ░░░░░░░░░░░░░░░░░░░░░░░ 198.8K  99.4%

Note: estimated conversation context only. Snip + microcompact are simulated; the async tool-result-budget pass is skipped.
```

**源码路径**
`crates/cc-commands/src/context.rs` — `ContextHandler`

**边界情况**
- 空会话也能正常渲染，各分类 token 为 0
- `json`/`raw` 子命令输出合法的 JSON，可被程序解析
- 未知子命令（如 `/context wobble`）返回 "Unknown /context subcommand" 提示
- 这是本地估算，不是精确 token 数，测试不应断言精确数值
- token 数值会做格式化：>= 1M 显示为 `1.5M`，>= 1K 显示为 `1.5K`

---

### `/files`

**功能描述**
从当前对话的消息和工具调用中提取引用过的文件路径。扫描 assistant 消息中的 `ToolUse` block，识别 `Read`/`FileRead`、`Write`/`FileWrite`、`Edit`/`FileEdit`、`Grep` 工具的 `file_path`/`path` 参数，以及 `Attachment::EditedTextFile` 中的路径，去重后按字母序输出。

**输出示例**
有文件时：
```
Files in context:
src/main.rs
src/lib.rs
```

无文件时：
```
No files in context
```

**源码路径**
`crates/cc-commands/src/files.rs` — `FilesHandler`

**边界情况**
- 空会话（无消息）返回 "No files in context"
- 只识别特定工具名（Read、Write、Edit、Grep），Bash 工具中的文件不解析
- 文件路径尝试相对于 `cwd` 显示，无法转换时保留绝对路径
- `Grep` 工具使用 `path` 字段而非 `file_path`
- 工具名大小写敏感：识别 `Read` 也识别 `FileRead` 和 `file_read`
- 重复路径自动去重（使用 `BTreeSet`）

---

### `/copy` (`/cp`)

**功能描述**
提取最后一条 assistant 消息中的纯文本内容。如果系统安装了 clipboard adapter，则尝试复制到系统剪贴板；否则回显文本。当前实现中 clipboard adapter 通常未安装，所以实际行为是回显文本并提示 "Failed to copy to clipboard"。

**输出示例**
有 assistant 消息且 clipboard 可用：
```
Copied to clipboard (123 chars):

Hello, world!
```

有 assistant 消息但 clipboard 不可用：
```
Failed to copy to clipboard: clipboard runtime adapter is not installed
Clipboard text (123 chars):

Hello, world!
```

无 assistant 消息：
```
No assistant messages found.
```

**源码路径**
`crates/cc-commands/src/copy.rs` — `CopyHandler`

**边界情况**
- 空会话返回 "No assistant messages found."
- 只提取 `ContentBlock::Text` 类型的内容，忽略 ToolUse、Thinking 等 block
- 如果最后一条 assistant 消息有多个 Text block，用换行符拼接
- 从后向前遍历消息，找到第一条有 Text block 的 assistant 消息就停止
- clipboard provider 通过 `set_clipboard_copy_provider` 注入，TUI 启动时设置

---

### `/compact`

**功能描述**
对当前对话执行本地压缩（compaction）。无参数时执行全量本地压缩；带自由文本参数时将其作为压缩指令；还支持部分压缩语法 `up-to <anchor>` 和 `from <anchor>`，分别压缩锚点之前或之后的消息。压缩运行 snip + microcompact 管道，生成摘要并插入 compact boundary 标记。

**输出示例**
压缩成功：
```
Compacted: ~50000 → ~12000 tokens (38000 tokens freed)
Messages: 205 → 15
```

无需压缩：
```
No compaction needed. Current conversation:
- 2 messages, ~150 estimated tokens
- Token usage is within limits for model 'gpt-5.5'
```

空会话：
```
Nothing to compact -- conversation is empty.
```

**源码路径**
`crates/cc-commands/src/compact.rs` — `CompactHandler`

**边界情况**
- 空会话返回 "Nothing to compact -- conversation is empty."
- 小会话（token 未超限）返回 "No compaction needed" 而非执行压缩
- 自由文本参数（如 `/compact focus on code changes`）会被记录在摘要消息中
- 部分压缩 `up-to` 使用 1-based 可见消息索引，0 会报错 "Message index must be 1-based"
- UUID 前缀匹配多条消息时报 "ambiguous" 错误
- 部分压缩受 `CC_RUST_PARTIAL_COMPACT` feature gate 控制
- 压缩后会插入 `CompactBoundary` 系统消息，包含 preserved segment 和压缩前后 token 数

---

### `/keybindings` (`/keys`、`/shortcuts`)

**功能描述**
查看、编辑或管理快捷键配置。默认行为（无参数）是确保 `~/.cc-rust/keybindings.json` 存在（不存在则从空模板创建）并在用户的 `$VISUAL`/`$EDITOR` 中打开；`status` 显示配置路径和生效绑定数；`list` 列出所有生效快捷键（可选按 context 过滤）；`reload` 强制重新加载；`path` 输出配置文件路径。

**输出示例**
`/keybindings status`：
```
Keybindings
───────────
  Config path: ~/.cc-rust/keybindings.json
  File:        exists
  Effective bindings: 24

Tip: run `/keybindings` to create and edit the file, or `/keybindings list` to see every effective binding.
```

`/keybindings`（默认，无 editor 时）：
```
Created ~/.cc-rust/keybindings.json from template.
```

**源码路径**
`crates/cc-commands/src/keybindings_cmd.rs` — `KeybindingsHandler`

**边界情况**
- 无参数时默认是"打开编辑器"而非"显示状态"，测试应断言不包含 "Effective bindings:" 文本
- `status` 显示 "Effective bindings:"，可作为与默认行为的区分标记
- `list` 可接受 context 过滤参数（如 `Chat`、`Global`），输出包含 `[Chat]` 这样的 context header
- 未知子命令返回 "Unknown /keybindings subcommand" 和 Usage
- 不存在 keybindings 文件时 `status` 显示 "not created yet"
- `reload` 会检查配置文件变更并报告解析问题

---

### `/statusline` (`/status-line`)

**功能描述**
查看、配置、测试或清空可编程状态栏命令。`set <cmd>` 持久化新命令；`clear` 移除命令；`enable`/`disable` 切换启用状态；`test [cmd]` 用合成 payload 运行一次性测试；`payload` 输出当前 JSON payload；`refresh`/`timeout`/`padding` 设置相关参数。持久化到 `~/.cc-rust/settings.json`。

**输出示例**
`/statusline`：
```
Status line
───────────
  type:              command
  command:           echo hello
  enabled:           yes
  padding:           0
  refreshIntervalMs: 5000 (effective: 5000)
  timeoutMs:         1000 (effective: 1000)
  would run now:     yes
  runs:              0
  errors:            0

Config path: ~/.cc-rust/settings.json

Tip: `/statusline set echo 'demo'` to try, `/statusline clear` to remove.
```

`/statusline test echo hello`：
```
Command: echo hello
── stdout ───────────────────────
hello
```

**源码路径**
`crates/cc-commands/src/statusline_cmd.rs` — `StatusLineHandler`

**边界情况**
- 无配置时 `test` 子命令返回 "No statusLine.command configured" 提示
- `test` 使用独立的 runner 实例，不会污染 TUI live runner 的 last output
- `payload` 子命令输出合法 JSON，包含 `hookEventName: "StatusLine"`
- `set` 空命令返回 Usage 提示
- `enable` 已启用或 `disable` 已禁用时状态不变，仍返回确认消息
- 配置同时更新内存中的 `app_state.settings.status_line` 和磁盘文件，当前会话立即生效

---

### `/terminal-setup` (`/term-setup`、`/terminal`)

**功能描述**
诊断当前终端环境并打印配置建议。只读命令，不修改任何用户配置。输出涵盖：检测到的终端程序、shell/复用器状态、`CLAUDE_CODE_*` 环境变量开关状态、各终端的 Shift+Enter 手动配置指南、tmux passthrough 建议、`$VISUAL`/`$EDITOR` 导出说明、鼠标捕获状态。

**输出示例**
```
Terminal setup
--------------
  Detected:   iTerm2
  TERM:       xterm-256color
  TERM_PROGRAM: iTerm.app
  SHELL:      /bin/zsh
  VISUAL:     nvim

CLAUDE_CODE_* toggles (issue #12)
  CLAUDE_CODE_NO_FLICKER:    (unset)
    -> synchronized updates: off
  CLAUDE_CODE_DISABLE_MOUSE: (unset)
    -> mouse capture:        enabled for wheel events; terminal drag selection is captured

Tips
----
Manual setup only: /terminal-setup is read-only and cannot edit terminal, tmux, shell, or editor configuration files.

Manual Shift+Enter setup (iTerm2):
  - iTerm2 -> Settings -> Keys -> Key Bindings -> map Shift+Return to 'Send Escape Sequence' with value `[27;2;13~`.
...
```

**源码路径**
`crates/cc-commands/src/terminal_setup.rs` — `TerminalSetupHandler`

**边界情况**
- `env` 子命令只显示环境变量表，不含 Tips
- `tips` 子命令只显示提示，不含环境变量表
- 未知子命令返回 "Unknown /terminal-setup subcommand" 和 Usage
- 通过 `$TERM_PROGRAM` 和 `$WT_SESSION`、`$VTE_VERSION` 等环境变量检测终端类型
- `VISUAL` 如果包含参数（如 `code --wait`），会提示 "includes arguments" 并说明 transcript export 当前不支持
- tmux 环境下额外显示 tmux 配置建议
- `CLAUDE_CODE_DISABLE_MOUSE` 的运行时支持因平台而异，当前 Rust TUI 可能显示 "not implemented"

---

### `/doctor` (`/diagnostics`、`/diag`)

**功能描述**
聚合诊断命令，遍历配置、认证、MCP、快捷键、沙箱、终端等多个子系统，生成统一健康报告。每行带有严重级别标签（ok/warn/fail/info），方便用户快速定位问题。纯数据提取，不修改任何状态。支持 `summary`（仅计数和重点问题）、`raw`/`json`（机器可读 JSON）子命令。

**输出示例**
```
Doctor
├── Install [ok]
│   ├── data root [ok] ~/.cc-rust
│   └── cc-rust version [info] 0.1.0
├── Auth [ok]
│   └── credential [ok] ANTHROPIC_API_KEY / keychain
├── Settings [ok]
│   └── validation [ok] no validation warnings
...
└── Terminal [ok]
    └── terminal [ok] iTerm2

Counts: 12 ok, 0 warn, 0 fail.
Use `/doctor summary` for just the red flags, or `/doctor raw` for JSON.
```

`/doctor summary`：
```
Doctor summary
──────────────
  ok: 12   warn: 0   fail: 0   info: 0
  All checks passed.
```

**源码路径**
`crates/cc-commands/src/doctor.rs` — `DoctorHandler`

**边界情况**
- 纯只读，不修改任何配置或状态
- 包含 9 个检查区段：Install、Auth、Settings、Permission Rules、Managed Configuration Overlays、MCP、Keybindings、Sandbox、Terminal
- `summary` 子命令只显示 warn/fail 级别的问题
- `raw`/`json` 输出合法 JSON，包含 `counts` 和 `sections` 字段
- 未知子命令返回 "Unknown /doctor subcommand" 和 Usage
- 认证失败时 Auth 区段整体标记为 fail，并提示 "run `/login` to authenticate"
- Terminal 区段复用 `terminal_setup` 模块的 `EnvProbe` 做终端检测

---

### `/experimental` (`/experiments`、`/exp`)

**功能描述**
查看和覆盖实验性功能门控。无参数或 `status` 显示当前生效状态（来源为启动环境变量或会话覆盖）；`list` 列出所有已知实验性功能及其开关状态；`on`/`off` 在当前 TUI 会话范围内启用/禁用所有实验功能；`reset` 恢复到启动时的环境变量设置。

**输出示例**
```
Experimental features: startup environment

  kairos                        OFF KAIROS assistant mode [CLAUDE_CODE_EXPERIMENTAL_KAIROS]
  agent_teams                   OFF Agent Teams support [CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS]
  coordinator                   OFF Coordinator mode [CLAUDE_CODE_COORDINATOR_MODE]
```

`/experimental on`：
```
Experimental features enabled for this TUI session.
Startup-only background processes may still require a restart.

  kairos                        ON  ...
  agent_teams                   ON  ...
```

**源码路径**
`crates/cc-commands/src/experimental.rs` — `ExperimentalHandler`

**边界情况**
- `on`/`enable` 等效；`off`/`disable` 等效
- `reset` 恢复到启动环境变量，不等于 `off`
- `status` 显示来源："startup environment" 或 "session override"
- 未知子命令返回 "Unknown experimental subcommand" 和 usage
- `help`/`--help`/`-h` 显示 usage 文本
- 功能门控修改仅影响当前 TUI 会话，不持久化到磁盘
- 启动时的后台进程（如 daemon）可能需要重启才能生效，`on` 命令会提示此限制
