# 命令 E2E 测试计划 10：查询返回命令（在线）

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_query.rs`
> 所有测试均为**在线**（需要真实 API 密钥），标记为 `#[ignore]`。
>
> 已隐藏命令（不在此计划中测试）：`/voice`

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 备注 |
|---|---|---|---|
| `/review` | -- | `Query` | 注入 PR 审查提示 |
| `/security-review` | `/secreview` | `Query` | 注入安全审查提示 |
| `/recap` | -- | `Output` | 总结当前会话 |
| `/loop` | -- | `Output` | 注册循环任务 |
| `/schedule` | `/cron` | `Output` | 管理 cron 任务 |

## 测试用例

### T01：`/review` 生成 PR 审查查询（在线）
```rust
#[ignore = "requires real API key"]
fn review_command_generates_query() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. SetPermission("full access")
    // 3. Command("review")
    // 4. Wait(10s)  // 模型需要时间处理
    // 5. AssertNoPanic
    // 6. Snapshot("review_output")
    // 断言：模型接收审查上下文并响应
}
```

### T02：`/security-review` 生成安全查询（在线）
```rust
#[ignore = "requires real API key"]
fn security_review_command() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. SetPermission("full access")
    // 3. Command("security-review")
    // 4. Wait(10s)
    // 5. AssertNoPanic
    // 6. Snapshot("security_review_output")
    // 断言：模型接收安全审查上下文
}
```

### T03：`/security-review` 别名 `/secreview`
```rust
#[ignore = "requires real API key"]
fn security_review_alias() {
    // 步骤：
    // 1. Command("secreview")
    // 2. Wait(10s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T04：`/recap` 总结会话（在线）
```rust
#[ignore = "requires real API key"]
fn recap_command() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Input("Hello, my name is TestBot")
    // 3. WaitForText("TestBot", API_TIMEOUT)
    // 4. Command("recap")
    // 5. Wait(10s)
    // 6. AssertNoPanic
    // 7. Snapshot("recap_output")
    // 断言：生成会话摘要
}
```

### T05：`/loop` 帮助
```rust
fn loop_command_help() {
    // 步骤：
    // 1. Command("loop")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 loop 用法/帮助
}
```

### T06：`/schedule` 帮助
```rust
fn schedule_command_help() {
    // 步骤：
    // 1. Command("schedule")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 schedule 用法/帮助
}
```

### T07：`/schedule` 别名 `/cron`
```rust
fn schedule_alias_cron() {
    // 步骤：
    // 1. Command("cron")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

## 优先级：低
查询返回命令需要 API 密钥。Voice/loop/schedule 是更简单的离线冒烟测试。

---

## 补充功能说明

以下为每个命令的功能细节，供 E2E 测试编写时参考。

---

### `/review`

#### 功能描述

注入一段结构化 PR 审查提示词到会话中，引导模型通过 `gh` CLI 获取 PR 上下文并执行代码审查。支持可选参数指定 PR 编号、分支名或 URL；无参数时自动查找当前分支对应的 open PR。

#### 输出示例

命令本身返回 `Query(Vec<Message>)`，不直接产生可见文本。注入的提示词要求模型输出以下结构化格式：

```
## Summary
<1-2 sentence overview>

## Must fix
<list, or 'None'>

## Suggestions
<list, or 'None'>

## Questions
<list, or 'None'>
```

#### 源码路径

`crates/cc-commands/src/review.rs`

关键函数：
- `ReviewHandler::execute()` — 入口，构造 `CommandResult::Query`
- `build_review_prompt()` — 根据是否传入 target 生成两种提示词模板

#### 边界情况

- 无参数 + 当前分支没有关联 PR → 模型应输出"no PR found"并停止（提示词中有明确指示）
- 传入不存在的 PR 编号（如 `/review 999999`）→ `gh pr view 999999` 会失败，依赖模型自行处理错误
- 传入 PR URL（如 `/review https://github.com/owner/repo/pull/42`）→ target 原样拼入 `gh pr view` 命令，`gh` CLI 支持 URL 作为参数
- 非 git 仓库中执行 → 命令本身不检查 git 状态（与 `/security-review` 不同），但 `gh pr list` 会失败

---

### `/security-review`

#### 功能描述

收集当前 git 仓库的分支名、工作区状态和最近提交记录，拼装成安全审查提示词注入会话。提示词要求模型以高级应用安全工程师身份审查当前分支的变更，输出按严重级别分类的安全发现。支持可选参数指定用户关注的安全领域。

#### 输出示例

命令本身返回 `Query(Vec<Message>)`。注入的提示词要求模型输出以下格式：

```
## Verdict
<APPROVE | CHANGES REQUESTED | BLOCKED — one line rationale>

## Findings
- **Severity:** Critical | High | Medium | Low
- **Location:** path:line
- **Issue:** one sentence
- **Impact:** what an attacker can do
- **Fix:** concrete remediation
```

若不在 git 仓库中执行，直接返回 `Output`：
```
Error: /security-review must be run inside a git repository.
```

#### 源码路径

`crates/cc-commands/src/security_review.rs`

关键函数：
- `SecurityReviewHandler::execute()` — 入口，先检查 `is_git_repo()`，再收集上下文
- `collect_git_context()` — 收集分支名、`git status --porcelain` 统计、最近 5 条 commit
- `build_security_prompt()` — 拼装提示词，可选注入 `User focus area` 段落

#### 边界情况

- 非 git 仓库 → 返回 `Output("Error: ...must be run inside a git repository.")`，不会注入 Query
- 带参数（如 `/security-review authentication flow`）→ focus 文本注入到 `User focus area: ...` 区块
- 空参数 → 不生成 `User focus area` 段落
- detached HEAD 状态 → `git_current_branch` 回退到 `git rev-parse --short HEAD`，显示短 SHA
- `git status` / `git log` 失败 → `collect_git_context` 静默跳过失败的子命令，最终可能输出 `(no git context available)`
- 别名 `/secreview` — 通过命令注册表映射，行为完全相同

---

### `/recap`

#### 功能描述

将会话历史摘要请求注入到对话中，让模型生成当前会话的总结。支持 `short`（默认，一句话）和 `long`（3-5 句段落）两种粒度。计数时排除 meta 消息和纯 tool-result 消息。

#### 输出示例

空会话时直接返回 `Output`：
```
Nothing to recap yet — the session has no user messages.
```

有用户消息时返回 `Query`，注入的提示词示例（short 模式）：
```
Summarize the current session in one short sentence (max ~25 words). The session has 2 user turn(s). Focus on what the user was trying to accomplish and the current state. No preamble, no trailing notes — just the sentence.
```

未知风格参数时返回 `Output`：
```
Unknown recap style: "super-long". Use `/recap` or `/recap long`.
```

#### 源码路径

`crates/cc-commands/src/recap.rs`

关键函数：
- `RecapHandler::execute()` — 解析粒度参数，计数 user turns，构造 Query
- `count_user_turns()` — 过滤 `is_meta` 和纯 tool-result 的 user 消息
- `build_recap_prompt()` — 根据 `Granularity::Short` / `Granularity::Long` 生成不同模板

#### 边界情况

- 空会话（零 user 消息）→ 返回 `Output("Nothing to recap...")`，不会调用模型
- 只有 meta 消息和 tool-result → `count_user_turns` 返回 0，行为等同空会话
- 未知参数（如 `/recap super-long`）→ 返回 `Output("Unknown recap style...")`
- 接受的风格别名：`""` / `short` / `brief` / `line` → Short；`long` / `full` / `detailed` → Long
- 不需要 API 密钥即可构造 Query（但实际总结需要模型在线处理）

---

### `/loop`

#### 功能描述

注册本地循环任务的用户友好包装器。支持创建、列出、删除、触发、暂停和恢复循环任务。创建时若 payload 是普通 prompt，会立即执行一次（返回 `Query`）；若 payload 是斜杠命令，仅注册不自动执行。任务持久化到 `{data_root}/scheduled_tasks.json`。

#### 输出示例

无参数（显示帮助）：
```
/loop — recurring local tasks.

  /loop <interval> <payload>   create a loop and run payload once
  /loop list                   list current loops
  /loop remove <id>            delete a loop
  /loop trigger <id>           mark a loop as fired now
  /loop pause <id>             temporarily suspend a loop
  /loop resume <id>            re-enable a paused loop

Interval examples: 30s, 5m, 1h, 2d, '*/10 * * * *'.
Payload is a slash command (/simplify) or a plain prompt.
```

创建普通 prompt 任务（返回 `Query`）：
```
Registered /loop review the last... (id=a1b2c3d4e5f6) every 5m. Running once now…

review the last commit
```

创建斜杠命令任务（返回 `Output`）：
```
Registered /loop simplify (id=a1b2c3d4e5f6) every 10m. Payload: /simplify.
The slash-command payload is not auto-executed — run `/simplify` now, or `/loop trigger a1b2c3d4e5f6` later.
```

`/loop list`：
```
Loops (1)
────────
  a1b2c3d4e5f6  every 5m  [local · active]
    payload (prompt): review the last commit
    next run: 2026-05-24T12:00:00Z
```

无注册任务时：
```
No /loop tasks registered.
```

#### 源码路径

`crates/cc-commands/src/loop_cmd.rs`

关键函数：
- `LoopHandler::execute()` — 打开 `SchedulerStore`，委托给 `dispatch()`
- `dispatch()` — 路由子命令：`list` / `remove` / `trigger` / `pause` / `resume` / `help` / 默认→`create_loop`
- `create_loop()` — 解析 interval + payload，区分 prompt（返回 Query）与 slash command（返回 Output）
- `render_list()` — 列出所有已注册循环任务
- `remove_task()` / `trigger_task()` / `set_paused()` — CRUD 操作

#### 边界情况

- 无参数 → 显示帮助文本（`CommandResult::Output`）
- 只传 interval 无 payload（如 `/loop 5m`）→ 返回 Usage 提示
- 无法解析的 interval（如 `/loop abc prompt`）→ 返回 "Could not parse interval" 错误
- 删除不存在的任务 ID → 返回 "No /loop task with id" 提示
- 暂停/恢复不存在的任务 → 同上
- payload 以 `/` 开头 → 识别为斜杠命令，不自动执行
- 支持的 interval 格式：`30s`、`5m`、`1h`、`2d`、`*/10 * * * *`（cron 表达式，仅支持分钟步长）
- 子命令别名：`ls` = `list`；`rm` / `delete` = `remove`；`fire` = `trigger`；`unpause` = `resume`

---

### `/schedule`

#### 功能描述

本地 cron 调度器的原始管理界面。与 `/loop` 共享同一个 `SchedulerStore` 后端，但提供更细粒度的子命令集，且不自动执行 payload。当前仅支持本地 cron 任务；远程触发器（`/schedule remote`）未实现，会提示用户查看 issue #60。

#### 输出示例

无参数或 `/schedule list`（有任务）：
```
Local scheduled tasks (2)
──────────────────────────
  a1b2c3  every 5m  [local · active]
    payload (prompt): review the last commit
    next run: 2026-05-24T12:00:00Z
  d4e5f6  every 1h  [local · paused]
    payload (command): /simplify
    next run: 2026-05-24T13:00:00Z

Remote triggers: (not implemented — see `/schedule remote`).
```

无任务时：
```
No scheduled tasks registered. Try '/schedule add <interval> <payload>'.
```

`/schedule add 5m run the deploy check`：
```
Added scheduled task 'run the deploy...' (id=a1b2c3) — runs every 5m starting at 2026-05-24T12:00:00Z.
```

`/schedule show <id>`：
```
Scheduled task a1b2c3
──────────────────────
  Name:          run the deploy...
  Kind:          local
  Status:        active
  Interval:      5m (raw: 5m)
  Payload kind:  prompt
  Payload:       run the deploy check
  Created at:    2026-05-24T11:00:00Z
  Last run at:   never
  Next run at:   2026-05-24T12:00:00Z
```

`/schedule due`（有到期任务）：
```
1 task(s) due now
────────────────────────
  a1b2c3  every 5m  [local · active]
    ...

Tip: the daemon tick loop (if running) will execute these on its next poll; use `/schedule trigger <id>` to mark as fired.
```

`/schedule remote add 1h foo`：
```
Remote triggers are not implemented yet in cc-rust (issue #60).
...
Use '/schedule list' to see local tasks.
```

`/schedule help`（包含存储路径）：
```
/schedule — local cron scheduler.

Scope: this command manages LOCAL cron tasks. Remote triggers
  (cloud-side scheduled agents) are tracked as a separate
  capability line and are not yet implemented — see
  `/schedule remote`.
  Storage: /home/user/.cc-rust/scheduled_tasks.json
...
Tip: /loop is a higher-level wrapper that also runs the payload once immediately.
```

#### 源码路径

`crates/cc-commands/src/schedule.rs`

关键函数：
- `ScheduleHandler::execute()` — 打开 `SchedulerStore`，委托给 `dispatch()`，始终返回 `CommandResult::Output`
- `dispatch()` — 路由子命令：`list` / `add` / `show` / `remove` / `pause` / `resume` / `trigger` / `due` / `remote` / `help`
- `add()` — 解析 interval + payload，创建 `ScheduledTask` 并持久化
- `show()` — 展示单个任务的完整详情（name, kind, status, interval, payload, created/last/next run）
- `due()` — 列出所有 `next_run_at <= now` 且未暂停的任务
- `remote_hint()` — 返回"未实现"提示文本

#### 边界情况

- 无参数 → 等价于 `list`
- `/schedule add` 缺少 payload → 返回 Usage 提示
- `/schedule add abc prompt`（无效 interval）→ 返回 "Could not parse interval" 错误
- `/schedule show` / `remove` / `pause` / `resume` / `trigger` 缺少 ID → 返回 Usage 提示
- 操作不存在的 ID → 返回 "No scheduled task with id" 错误
- `/schedule remote ...` → 返回"未实现"提示，不报错
- 未知子命令（如 `/schedule frobnicate`）→ 返回 "Unknown /schedule subcommand" 错误
- 子命令别名：`ls` = `list`；`create` / `new` = `add`；`info` = `show`；`rm` / `delete` = `remove`；`unpause` = `resume`；`fire` = `trigger`
- 与 `/loop` 共享同一存储后端（`SchedulerStore`），但 `/schedule add` 不自动执行 payload（纯 `Output`），而 `/loop` 对 prompt 类 payload 返回 `Query` 并立即执行一次
- cron 表达式仅支持分钟步长（`*/N * * * *` 或 `N * * * *`），其他字段必须为 `*`
