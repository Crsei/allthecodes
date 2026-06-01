# 命令 E2E 测试计划 08：KAIROS 与功能门控命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_kairos.rs`
> 所有测试均为**离线**。功能门控命令在门控关闭时应显示有用的提示信息。

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 功能门控 |
|---|---|---|---|
| `/sleep` | -- | `Output` | `FEATURE_PROACTIVE=1` |
| `/assistant` | `/kairos` | `Output` | `FEATURE_KAIROS=1` |
| `/daemon` | -- | `Output` | `FEATURE_KAIROS=1` |
| `/notify` | -- | `Output` | `FEATURE_KAIROS_PUSH_NOTIFICATION=1` |
| `/remote` | -- | `Output` | 无（但相关） |
| `/channels` | -- | `Output` | `FEATURE_KAIROS_CHANNELS=1` |
| `/dream` | `/logs` | `Output` | `FEATURE_KAIROS=1` |

## 测试用例

### T01：`/sleep` 无功能门控
```rust
fn sleep_no_gate() {
    // 未设置 FEATURE_PROACTIVE=1 时：
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("sleep")
    // 3. Wait(2s)
    // 4. AssertScreenContains("feature") 或 AssertScreenContains("not available")
    //    或若门控已开启：AssertScreenContains("sleep") 或 AssertScreenContains("tick")
    // 断言：显示功能门控信息或用法说明
}
```

### T02：`/sleep` 无效参数
```rust
fn sleep_invalid_arg() {
    // 步骤：
    // 1. Command("sleep 0")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：拒绝无效值
}
```

### T03：`/sleep` 有效参数
```rust
fn sleep_valid_arg() {
    // 步骤：
    // 1. Command("sleep 60")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：已接受或显示功能门控信息
}
```

### T04：`/assistant` 无功能门控
```rust
fn assistant_no_gate() {
    // 步骤：
    // 1. Command("assistant")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：功能门控信息或状态
}
```

### T05：`/assistant` 别名 `/kairos`
```rust
fn assistant_alias() {
    // 步骤：
    // 1. Command("kairos")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T06：`/daemon` 状态
```rust
fn daemon_status() {
    // 步骤：
    // 1. Command("daemon")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：功能门控信息或守护进程状态
}
```

### T07：`/daemon stop`
```rust
fn daemon_stop() {
    // 步骤：
    // 1. Command("daemon stop")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 "stop requested" 或功能门控信息
}
```

### T08：`/notify` 状态
```rust
fn notify_status() {
    // 步骤：
    // 1. Command("notify")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：功能门控信息或通知状态
}
```

### T09：`/notify test`
```rust
fn notify_test() {
    // 步骤：
    // 1. Command("notify test")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：测试通知或功能门控信息
}
```

### T10：`/notify on/off`
```rust
fn notify_toggle() {
    // 步骤：
    // 1. Command("notify on")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("notify off")
    // 5. Wait(2s)
    // 6. AssertNoPanic
    // 断言：切换操作无 panic
}
```

### T11：`/remote` 命令
```rust
fn remote_command() {
    // 步骤：
    // 1. Command("remote")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示远程网关状态或帮助
}
```

### T12：`/channels` 无功能门控
```rust
fn channels_no_gate() {
    // 步骤：
    // 1. Command("channels")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：功能门控信息或频道列表
}
```

### T13：`/channels list`
```rust
fn channels_list() {
    // 步骤：
    // 1. Command("channels list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出频道或功能门控信息
}
```

### T14：`/dream` 无功能门控
```rust
fn dream_no_gate() {
    // 步骤：
    // 1. Command("dream")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：功能门控信息或 dream 状态
}
```

### T15：`/dream --days 14`
```rust
fn dream_with_days() {
    // 步骤：
    // 1. Command("dream --days 14")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：已接受或功能门控信息
}
```

### T16：`/dream help`
```rust
fn dream_help() {
    // 步骤：
    // 1. Command("dream help")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 dream 帮助
}
```

### T17：`/dream` 别名 `/logs`
```rust
fn dream_alias_logs() {
    // 步骤：
    // 1. Command("logs")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T18：所有 KAIROS 命令批量测试
```rust
fn kairos_batch_no_crash() {
    // 执行：/sleep、/assistant、/daemon、/notify、/remote、/channels、/dream
    // 每个执行后 AssertNoPanic
    // 断言：即使无功能门控，所有命令也不会崩溃
}
```

## 优先级：中
功能门控命令应优雅降级。测试无论门控状态如何都不会崩溃。

## 补充功能说明

### `/sleep`

- **功能描述**：设置 proactive 睡眠时长（秒），暂停自主操作（tick loop）。无参数显示用法和当前 tick interval；有参数时通过 `SleepCommandRuntime` 适配器将睡眠时间写入守护进程状态，并在到期前暂停自主行为。
- **输出示例（门控关闭）**：
  ```
  Sleep command requires FEATURE_PROACTIVE=1
  ```
- **输出示例（无参数，门控开启）**：
  ```
  Usage: /sleep <seconds> (range: 1-3600)
  Current tick interval: disabled
  ```
- **输出示例（有效参数，门控开启）**：
  ```
  Sleep scheduled until 2026-05-24T12:00:00+00:00 (60 seconds). Proactive actions paused.
  ```
- **输出示例（无效参数）**：
  ```
  Invalid number: 'abc'. Usage: /sleep <seconds> (1-3600)
  ```
- **源码路径**：`crates/cc-commands/src/sleep_cmd.rs`（`SleepCmdHandler`）
- **边界情况**：
  - 参数 `0`：超出 `MIN_SLEEP_SECS=1`，应提示范围
  - 参数 `3601`：超出 `MAX_SLEEP_SECS=3600`，应提示范围
  - 参数 `abc`：无法解析为 u64，应显示用法
  - 参数为空：应显示用法+当前 tick interval
  - `SleepCommandRuntime` 未安装时：返回错误，说明 runtime adapter 不可用
  - 门控关闭时所有参数都走分支持拒绝路径

### `/assistant`（别名 `/kairos`）

- **功能描述**：显示 KAIROS / assistant mode 的综合状态面板，包括 daemon 活性、brief mode、assistant mode、terminal focus、tick interval 和当前模型。当前实现不解析子参数，额外参数被忽略。
- **输出示例（门控关闭）**：
  ```
  Assistant mode requires FEATURE_KAIROS=1
  ```
- **输出示例（门控开启）**：
  ```
  === Assistant (KAIROS) Status ===
  Daemon active:     no
  Brief mode:        OFF
  Assistant mode:    OFF
  Terminal focus:    no
  Tick interval:     disabled
  Model:             gpt-5.5
  ```
- **源码路径**：`crates/cc-commands/src/assistant.rs`（`AssistantHandler`）
- **边界情况**：
  - 传任意参数（如 `/assistant foo`）：被忽略，仍显示状态面板
  - 门控关闭时所有状态字段均为 `app_state` 默认值
  - 别名 `/kairos` 应与主命令完全等价
  - `autonomous_tick_ms` 为 `None` 时显示 "disabled"

### `/daemon`

- **功能描述**：查看/控制 daemon 进程。`status`（默认）显示 daemon PID、health URL、state file 路径和 worker 列表；`stop` 请求 daemon 关闭；`start`/`restart` 返回 shell 命令提示。状态数据通过 `DaemonCommandRuntime` 适配器获取。
- **输出示例（running）**：
  ```
  === Daemon Status ===
  Running:    yes
  PID:        12345
  Health URL: http://127.0.0.1:8080/health
  State file: /home/user/.cc-rust/daemon/supervisor.json
  Workers:    2
    - worker-1 kind=loop pid=12346 status=running
    - worker-2 kind=notify pid=12347 status=running
  ```
- **输出示例（stopped）**：
  ```
  === Daemon Status ===
  Running:    no
  State file: /home/user/.cc-rust/daemon/supervisor.json
  ```
- **输出示例（stale）**：
  ```
  === Daemon Status ===
  Running:    stale
  Last PID:   12345
  State file: /home/user/.cc-rust/daemon/supervisor.json
  ```
- **输出示例（stop 成功）**：
  ```
  Daemon stop requested for PID 12345.
  ```
- **源码路径**：`crates/cc-commands/src/daemon_cmd.rs`（`DaemonCmdHandler`）
- **边界情况**：
  - `DaemonCommandRuntime` 未安装时：返回错误
  - `start`/`restart`：提示用户走 shell 命令，不直接启动
  - 未知子命令：显示完整用法
  - `stop` 在 Stopped 状态：提示 "Daemon is not currently running."
  - `stop` 在 Stale 状态：提示需用 shell 命令刷新
  - 门控关闭时：由调用侧（lib.rs）进行门控检查，handler 内部无门控

### `/notify`

- **功能描述**：管理推送通知设置。`status`（默认）显示通知状态；`test` 发送测试通知；`on`/`off` 启用/禁用。当前实现为占位逻辑，返回固定文本。
- **输出示例（门控关闭）**：
  ```
  Notifications require FEATURE_KAIROS_PUSH_NOTIFICATION=1
  ```
- **输出示例（status）**：
  ```
  Notification status: check settings.json for configuration.
  ```
- **输出示例（test）**：
  ```
  Test notification sent.
  ```
- **输出示例（on/off）**：
  ```
  Notifications enabled.
  ```
- **源码路径**：`crates/cc-commands/src/notify.rs`（`NotifyHandler`）
- **边界情况**：
  - 未知子命令：显示用法
  - 门控关闭时所有子命令均拒绝，不区分参数
  - 门控开启时所有子命令返回固定文本，不涉及真实推送

### `/remote`

- **功能描述**：查看和控制本地 remote gateway。功能包括：查看 daemon 状态和 gateway capabilities（`status`）；列出适配器连接状态（`adapters`）；连接适配器（`connect <telegram|lark>`）；发送测试消息（`test-message`）；管理远程 runs（`runs`、`show`、`events`、`stop`）；诊断（`doctor`）。输出中敏感信息（bearer token、key、secret）会被自动脱敏。
- **输出示例（status，daemon stopped）**：
  ```
  Remote Gateway Status
  Daemon: stopped
  Gateway: unavailable
  Action: start the daemon with `FEATURE_KAIROS=1 claude daemon start`.
  ```
- **输出示例（status，daemon running）**：
  ```
  Remote Gateway Status
  Daemon: running (pid=12345)
  Gateway: http://127.0.0.1:8080
  Health: http://127.0.0.1:8080/health
  Version: 1.0.0
  Auth: {"mode":"token"}
  Steer: true
  Capacity: running=3 queued=0
  Endpoints: 5
  ```
- **输出示例（adapters）**：
  ```
  Remote adapters (2)
  telegram  configured=true  state=connected  message=Connected
  lark  configured=false  state=unconfigured  message=Not configured
  ```
- **输出示例（runs）**：
  ```
  Remote runs (latest 3)
  run-abc  status=Running  session=sess-1  updated=1234567890
  ```
- **源码路径**：`crates/cc-commands/src/remote_cmd.rs`（`RemoteHandler`）
- **边界情况**：
  - 无子命令：默认等价于 `status`
  - 未知子命令：显示 "Unknown /remote subcommand 'xxx'" + 完整用法
  - `connect` 缺 provider：显示用法
  - `test-message` 缺 provider/target：显示用法，text 为空时使用默认文本
  - `show`/`events`/`stop` 缺 run_id：显示用法
  - Gateway 适配器未注册时：返回 "cannot reach the local gateway adapter" 诊断
  - 输出中 `Bearer <token>`、`key=value` 等敏感字段会被脱敏为 `<redacted>`
  - `--limit`/`-n` 控制显示条数，范围 1-100，默认 20
  - `doctor` 汇总 status + 所有 gateway 路径

### `/channels`

- **功能描述**：查看 gateway-backed outbound adapter 连接状态。子命令 `list`/`status`（等价）复用 `/remote adapters` 的渲染逻辑。当前 inbound channel sessions 为 deferred 状态。
- **输出示例（门控关闭）**：
  ```
  Channels require FEATURE_KAIROS_CHANNELS=1
  ```
- **输出示例（list/status，门控开启）**：
  ```
  Channels
  Inbound channel sessions are deferred; outbound remote adapters are the real channel surface currently wired.

  Remote adapters (0)

  Use `/remote adapters` for the same gateway-backed adapter status.
  ```
- **源码路径**：`crates/cc-commands/src/channels.rs`（`ChannelsHandler`）
- **边界情况**：
  - 未知子命令：显示用法
  - 门控关闭时所有参数均拒绝
  - 底层依赖 `remote_cmd::render_adapters()`，需确保 gateway 适配器存在
  - `list`/`status`/无参数行为完全等价

### `/dream`（别名 `/logs`）

- **功能描述**：将最近的 session 日志蒸馏为长期记忆摘要。无参数默认处理最近 7 天；`--days N` 指定天数；`help`/`--help` 显示帮助。当前实现为占位逻辑，返回确认文本而不真正执行蒸馏。
- **输出示例（门控关闭）**：
  ```
  Dream mode requires FEATURE_KAIROS=1
  ```
- **输出示例（无参数，门控开启）**：
  ```
  Distilling last 7 days of logs into memory...
  ```
- **输出示例（`--days 14`）**：
  ```
  Distilling last 14 days of logs into memory...
  ```
- **输出示例（`help`）**：
  ```
  Usage: /dream [--days N]  (default: 7 days)
  ```
- **源码路径**：`crates/cc-commands/src/dream.rs`（`DreamHandler`）
- **边界情况**：
  - `--days 0`：拒绝（要求 N > 0），显示用法
  - `--days abc`：parse 失败，显示用法
  - `--days` 后无值：parse 返回 None，fallback 到 7 天
  - 参数 `--unknown`：parse_days 返回 None，fallback 到 7 天
  - `help` 和 `--help` 行为等价
  - 门控关闭时所有参数均拒绝
  - 别名 `/logs` 与 `/dream` 等价
