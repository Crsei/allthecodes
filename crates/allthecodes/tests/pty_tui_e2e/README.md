# PTY TUI E2E 测试套件

通过真实伪终端 (PTY) 端到端测试 allthecodes Rust TUI 的完整行为。

## 架构

```text
┌──────────┐    PTY    ┌──────────────┐    screen    ┌───────────┐
│ 测试代码  │ ────────→ │ allthecodes │ ──────────→ │ vt100 解析│
│ (Rust)   │ ←──────── │ (TUI 二进制)  │ ←────────── │ (断言)    │
└──────────┘  stdin    └──────────────┘  stdout      └───────────┘
```

- `portable-pty` 创建真实伪终端，启动 `allthecodes` 二进制
- 模拟键盘输入（文本、快捷键、斜杠命令）
- `vt100` 解析器读取终端屏幕内容，用于断言和截图
- 自动回复 crossterm DSR 查询 (`\x1b[6n`)，防止进程阻塞

## 文件说明

| 文件 | 说明 |
|------|------|
| `main.rs` | 测试入口，声明所有模块，关联 command-operation-display-plan.md Phase 7 |
| `harness.rs` | PTY 会话封装：启动、输入、屏幕读取、等待、截图 |
| `script.rs` | **步骤式模板引擎**：用数据结构定义测试，自动执行和截图 |
| `welcome.rs` | 欢迎屏幕：Logo、模型名、会话 ID、终端尺寸 |
| `commands.rs` | 斜杠命令：/help /version /cost /status、命令面板 |
| `conversation.rs` | 完整对话流：单轮、多轮上下文、工具调用、Ctrl+C 中断 |
| `model_flow.rs` | 模型验证：settings.json 配置、/model 切换、authProfile 切换 |
| `permissions.rs` | 权限系统：bypass/default/auto 模式、Always Allow、Auto Review |
| `status.rs` | 状态栏：ready 状态、消息计数、操作后状态恢复 |
| `screenshot.rs` | 截图功能：HTML 渲染、mid-session 快照、多格式保存 |

## 运行

```bash
# 设置 Rust 工具链路径
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

# 运行所有离线测试（不需要 API key）
cargo test -p allthecodes --test pty_tui_e2e -- --nocapture

# 运行所有在线测试（需要真实 API key，从 ~/.allthecodes/settings.json 读取）
cargo test -p allthecodes --test pty_tui_e2e -- --ignored --nocapture

# 运行单个模块
cargo test -p allthecodes --test pty_tui_e2e welcome -- --nocapture
cargo test -p allthecodes --test pty_tui_e2e commands -- --nocapture

# 运行模板引擎测试
cargo test -p allthecodes --test pty_tui_e2e -- script --nocapture

# 运行指定的模板测试
cargo test -p allthecodes --test pty_tui_e2e -- script_command_palette --nocapture
cargo test -p allthecodes --test pty_tui_e2e -- --ignored script_conversation_verify --nocapture
```

## 模板引擎 (`script.rs`)

### 核心概念

```text
TestCase（测试脚本）
  └─ Vec<TestStep>（步骤序列）
       ├─ SkipTrustGate          // 跳过首次信任确认
       ├─ Input("text")          // 输入文本 + Enter
       ├─ Command("model gpt-5.4") // 斜杠命令（自动加 "/"）
       ├─ Key(TestKey::CtrlC)    // 快捷键
       ├─ Snapshot("label")      // 截图保存
       ├─ WaitForText("x", 30s)  // 等待文本出现
       ├─ AssertStatusBar("gpt") // 断言状态栏
       ├─ SetPermission("full access") // 设置权限
       └─ LoginSwitch("profile") // 切换 authProfile
```

### TestStep 完整列表

| 步骤 | 说明 |
|------|------|
| `Wait(Duration)` | 等待固定时长 |
| `Input(String)` | 输入文本并按 Enter |
| `TypeText(String)` | 只输入文本，不按 Enter |
| `Command(String)` | 斜杠命令（自动加 "/" 前缀 + Enter） |
| `Key(TestKey)` | 发送快捷键 |
| `Snapshot(String)` | 手动截图（保存 .html + .log/.stream.log/.raw） |
| `OpenPalette` | 打开命令面板（发送 "/"，等待 "Commands" 出现） |
| `PaletteSelect(usize)` | 命令面板选择第 N 项（Down N-1 次 + Enter） |
| `ClosePalette` | 关闭命令面板（Esc） |
| `AssertScreenContains(String)` | 断言屏幕包含指定文本 |
| `AssertTextContains(String)` | 断言纯文本输出包含指定文本 |
| `AssertStatusBar(String)` | 断言状态栏包含指定文本 |
| `AssertScreenNotContains(String)` | 断言屏幕不包含指定文本 |
| `WaitForScreenText(String, Duration)` | 等待当前可见屏幕包含指定文本 |
| `AssertPromptContains(String)` | 断言 prompt/当前屏幕包含指定文本 |
| `AssertFileContains(String, String)` | 断言文件包含指定文本 |
| `AssertFileNotContains(String, String)` | 断言文件不包含指定文本 |
| `WaitForText(String, Duration)` | 等待文本出现在输出中 |
| `WaitForAny(Vec<String>, Duration)` | 等待任意一个文本出现 |
| `WaitForStatus(String, Duration)` | 等待状态栏包含指定文本 |
| `MultilineInput(Vec<String>)` | 多行输入（每行依次发送） |
| `SkipTrustGate` | 跳过 workspace trust gate |
| `SetPermission(String)` | 通过 /permissions 设置权限 |
| `LoginSwitch(String)` | 通过 /login 切换 authProfile |
| `AssertNoPanic` | 断言输出中无 "panicked" |

### TestKey 快捷键

| 快捷键 | 说明 |
|--------|------|
| `CtrlC` | 中断（ETX 0x03） |
| `CtrlD` | 退出（EOT 0x04） |
| `CtrlU` | 清除当前行 |
| `CtrlL` | 清屏 |
| `CtrlR` | 反向搜索 |
| `F12` | TUI debug snapshot |
| `Enter` | 回车 |
| `Escape` | Esc |
| `Tab` | Tab |
| `Up` / `Down` | 方向键 |
| `Left` / `Right` | 左右方向键 |
| `Backspace` / `Delete` | 删除键 |
| `PageUp` / `PageDown` | 翻页键 |
| `Home` / `End` | 行首/行尾键 |

### 用法示例

#### 1. 基础对话验证

```rust
use crate::harness::*;
use crate::script::*;
use std::time::Duration;

#[test]
#[ignore = "requires real API key"]
fn verify_model_identity() {
    let settings = read_settings();
    let case = TestCase::new(format!("conv_{}", settings.active_auth_profile))
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::AssertStatusBar(settings.expected_model.clone()))
        .step(TestStep::Input("Answer with ONLY your model ID.".into()))
        .step(TestStep::WaitForAny(
            vec![settings.expected_model.clone()],
            API_TIMEOUT,
        ))
        .step(TestStep::Snapshot("model_reply".into()));

    TestRunner::new().run(&case).assert_no_errors();
}
```

#### 2. 切换 authProfile

```rust
#[test]
#[ignore = "requires real API key"]
fn switch_profile() {
    let case = TestCase::new("switch_to_claude_code")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::LoginSwitch("claude-code".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertStatusBar("deepseek-v4-pro".into()))
        .step(TestStep::Snapshot("after_switch".into()));

    TestRunner::new().run(&case).assert_no_errors();
}
```

#### 3. 权限设置 + 工具执行

```rust
#[test]
#[ignore = "requires real API key"]
fn permissions_and_tool_use() {
    let case = TestCase::new("perm_tool")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Input("Use Bash to run: echo OK".into()))
        .step(TestStep::WaitForText("OK".into(), API_TIMEOUT))
        .step(TestStep::Snapshot("tool_done".into()));

    TestRunner::new().run(&case).assert_no_errors();
}
```

#### 4. Ctrl+C 中断 + 恢复

```rust
#[test]
#[ignore = "requires real API key"]
fn abort_and_recover() {
    let case = TestCase::new("abort_recover")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Input("Write a 2000-word essay.".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Snapshot("after_abort".into()))
        .step(TestStep::Input("Say exactly: RECOVERED".into()))
        .step(TestStep::WaitForText("RECOVERED".into(), API_TIMEOUT))
        .step(TestStep::Snapshot("recovered".into()));

    TestRunner::new().run(&case).assert_no_errors();
}
```

#### 5. 命令面板操作

```rust
#[test]
fn command_palette_flow() {
    let case = TestCase::new("palette_flow")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::OpenPalette)
        .step(TestStep::AssertScreenContains("Commands".into()))
        .step(TestStep::Snapshot("palette_open".into()))
        .step(TestStep::ClosePalette);

    TestRunner::new().run(&case).assert_no_errors();
}
```

#### 6. CommandSurface 交互测试

`tests/commands_surface.rs` 覆盖 `/config`、`/model`、`/mcp`、`/skills` 等
无参数斜杠命令打开的交互面板。另有 13 个子模块覆盖详细功能测试：

| 文件 | 测试内容 |
|------|----------|
| `commands_surface.rs` | 所有 CommandSurface 的打开内容、导航、筛选、快捷键、空态/禁用态 |
| `tests/` 下其余 13 文件 | 核心信息、查询、认证、git、MCP/plugin、memory/skills/hooks、permissions/sandbox、agent/team、别名、session、kairos、任务中命令、综合流程 |

运行方式：

```bash
cargo test -p allthecodes --test pty_tui_e2e commands_surface -- --nocapture
```

#### 7. 链式 Builder 语法

```rust
let case = TestCase::new("my_test")
    .cols(120)
    .rows(40)
    .env("ALLTHECODES_HOME", "/tmp/test-home")
    .exit(ExitMethod::CtrlC)
    .timeout(Duration::from_secs(180))
    .step(TestStep::SkipTrustGate)
    .step(TestStep::Input("hello".into()));
```

## Harness API (`harness.rs`)

### PtySession

```rust
// 启动
let session = PtySession::spawn(&args, cols, rows, strip_keys);
let session = PtySession::spawn_with_env(&args, cols, rows, strip_keys, &envs);

// 输入
session.send_line("text");          // 文本 + Enter
session.send_raw(b"\x03");          // 原始字节
session.send_ctrl_c();              // Ctrl+C
session.send_ctrl_d();              // Ctrl+D
session.send_ctrl_u();              // Ctrl+U (清除行)
session.send_ctrl_l();              // Ctrl+L (清屏)
session.send_ctrl_r();              // Ctrl+R (搜索)
session.send_f12();                 // F12 (debug snapshot)
session.send_up() / send_down();    // 方向键
session.send_escape();              // Esc
session.send_tab();                 // Tab

// 屏幕读取
let screen = session.current_screen();   // 当前可见屏幕文本
let text = session.current_text();       // 累积纯文本（ANSI 去除）
let bar = session.status_bar();          // 状态栏内容
let row = session.screen_row(0);         // 指定行

// 等待
session.wait_for_text("needle", timeout);      // 等待文本出现
session.wait_for_any(&["a", "b"], timeout);    // 等待任一文本
session.wait_for_screen_text("x", timeout);    // 等待屏幕文本
session.wait_status("ready", timeout);          // 等待状态栏
session.wait_response_done(min_msgs, timeout); // 等待响应完成

// 截图
session.snapshot("label");                          // 保存到 logs_dir()
session.snapshot_to("label", &dir);                 // 保存到指定目录
session.finish(timeout, "test_name");               // 退出 + 保存到 logs_dir()
session.finish_to(timeout, "test_name", &dir);      // 退出 + 保存到指定目录
```

### 辅助函数

```rust
workspace()          // 测试工作区路径（E2E_WORKSPACE 或 /tmp/cc-rust-e2e-test-{runner_pid}）
logs_dir()           // 日志根目录（logs/pty_tui_e2e_{timestamp}_{runner_pid}/）
test_subdir("name")  // 测试专属子目录（logs/pty_tui_e2e_{timestamp}_{runner_pid}/{name}/）
binary_path()        // allthecodes 二进制路径
default_args()       // 标准启动参数：-C {workspace} --permission-mode bypass
read_settings()      // 读取 ~/.allthecodes/settings.json 的 activeAuthProfile 和 model
skip_trust_gate()    // 跳过首次 workspace 信任确认
```

## 输出结构

```
crates/allthecodes/logs/pty_tui_e2e_{YYYYMMDDHHMM}/
├── {test_name}/                    ← 每个测试独立文件夹
│   ├── step_001_skip_trust.html    ← 步骤截图（HTML 终端渲染）
│   ├── step_001_skip_trust.log     ← 当前屏幕文本（适合人工阅读）
│   ├── step_001_skip_trust.stream.log ← 累积纯文本流（ANSI 去除）
│   ├── step_001_skip_trust.raw     ← 原始 PTY 字节流
│   ├── step_003_input_question.html
│   ├── step_003_input_question.log
│   ├── ...
│   ├── index.html                  ← 步骤报告和产物导航
│   ├── session_full.html           ← 完整会话 HTML 截图
│   ├── session_full.log            ← 完整会话最终屏幕文本
│   ├── session_full.stream.log     ← 完整会话累积纯文本流
│   ├── session_full.raw            ← 完整会话原始 PTY 字节流
│   └── errors.txt                  ← 错误汇总（仅在有错误时生成）
├── {test_name_2}/
│   └── ...
```

HTML 文件可在浏览器中打开查看终端截图，带暗色终端样式和测试/步骤元信息。`.log`
是 vt100 当前屏幕快照，适合人工阅读；`.stream.log` 保留旧的累计输出语义，用于搜索
历史输出和重绘残留；`.raw` 用于排查 ANSI 控制序列和 PTY 时序问题。

## 测试分类

### 离线测试（不需要 API key）

测试 UI 渲染和基本交互，使用 `--permission-mode bypass` 和空 API key：

- `welcome::*` — 欢迎屏幕、模型名、终端尺寸
- `commands::*` — 斜杠命令、命令面板
- `status::*` — 状态栏渲染
- `screenshot::*` — 截图功能
- `permissions::no_api_key_shows_error` — 无 API key 错误提示
- `permissions::all_permission_modes_start_cleanly` — 权限模式启动
- `permissions::permission_dialog_renders_in_screen_area` — 权限对话框渲染
- `script::tests::script_command_palette` — 模板引擎命令面板
- `tests::commands_surface::*` — ~36 个 CommandSurface 交互测试（打开内容、导航、筛选、快捷键、空态/禁用态）
- `tests::commands_core_info::*` — /info /version /cost 等核心信息命令
- `tests::commands_permissions::*` — /sandbox /permissions 命令
- `tests::commands_session::*` — /session 命令
- `tests::*` — 其余 10 个子模块共 ~100 个离线测试用例

### 在线测试（需要真实 API key）

从 `~/.allthecodes/settings.json` 读取 authProfile 配置，标记为 `#[ignore]`：

- `conversation::*` — 单轮/多轮对话、工具调用、中断恢复
- `model_flow::*` — 模型验证、/model 切换、authProfile 切换
- `permissions::bypass_mode_executes_without_dialog` — bypass 模式工具执行
- `permissions::default_mode_denies_tool` — default 模式拒绝工具
- `script::tests::script_conversation_verify` — 模板对话验证
- `script::tests::script_switch_auth_profile` — 模板 authProfile 切换
- `script::tests::script_set_permissions` — 模板权限设置
- `script::tests::script_abort_and_recover` — 模板中断恢复
- `script::tests::script_model_switch` — 模板模型切换

## 时效策略（信号化等待）

> 实施依据：`development/test/pty-tui-e2e-timing-plan.md`（Task 1 + Task 2 阶段）。

历史上离线套件使用大量硬编码 `TestStep::Wait(Duration::from_secs(2))` 等"等待屏幕静下来再读取"的固定等待。基线 1930s 中相当比例来自这些所选时间（其中很多并不必要），且会因机器波动 flaky。**信号化等待**的做法是：用屏幕内容判断"已经到该到的地方没"，命中即返回，未命中则短 timeout 后失败。

### 何时用哪种等待

| 模式 | 何时用 | 实现 |
|------|--------|------|
| `WaitForScreenText(needle, timeout)` | 紧跟 `Command` 之后需要断言屏幕出现某文字 | 命中 needle 即返回（实测 ~150-270ms）；超时才 stage 时间到达 timeout 上界 |
| `WaitForAny(needles, timeout)` | 多个等价成功信号任选其一（如 "Effort set to" 但也可能 "Current profile has no configured"） | 任一命中即返回 |
| `Wait(Duration::from_millis(500))` | 在 `Snapshot` 之前给缓冲一个短稳定窗口（命令输出刷屏已基本完成，但仍要给 vt100 时间分页） | 与上面信号化不冲突：Snapshot 读的是屏幕瞬时状态，500ms 量级已够 |
| `Wait(Duration::from_secs(2))` 之后 `AssertNoPanic` | **不要再用** | `AssertNoPanic` 内部已 sleep 200ms 足够 buffer flush |
| `Wait(Duration::from_secs(2))` 紧跟 `SkipTrustGate` | **不要再用** | `SkipTrustGate` 内部已 sleep `RENDER_WAIT`(3s) + 500ms |

### Drop / Replace 规则

每次替换前必须先看上下两行：

1. `SkipTrustGate` → `Wait(2s)` → **DROP**：后续步骤无信号依赖
2. `Command(X)` → `Wait(2s)` → `AssertScreenContains(needle)`：**REPLACE** 为
   `Command(X)` → `WaitForScreenText(needle, 3s)`
3. `Command(X)` → `Wait(Ns)` → `Snapshot`：**缩短** 为
   `Command(X)` → `Wait(Duration::from_millis(500))` → `Snapshot`
   （Snapshot 是调试用途，留个短窗口即可）
4. `Y` → `Wait(Ns)` → `AssertNoPanic/Snapshot`：**DROP**，下游已自带稳定窗口

### 范围与边界

- **离线测试**：替换原 11 个测试文件里 200+ 个固定 `Wait(secs)` 步骤中绝大多数。`commands_surface.rs` 已经 `SHORT_WAIT` 常量 + 信号化，无改动。
- **在线测试**（`#[ignore = "requires real API key"]`）：**不替换其内对 `API_TIMEOUT` 的依赖**，且保留它们原有的固定 `Wait` —— 在线测试的耗时由 API 调用决定，不在本计划的可提速范围（参 §1.4 + §7 非目标）。脚本扫描器对带有 `#[ignore]` 属性的函数体应跳过替换。

### 量化

按 plan §6.1 的验收：替换后单测墙钟大体会从 ~7-13s 降到 ~4-5s（命令输出回显在数百毫秒级，旧版每步固定砍 2s 浪费）。绝对套件墙钟（≤1200s Phase 1 阈值）请用 `time cargo test -p allthecodes --test pty_tui_e2e -- --nocapture` 自行度量（与硬件强相关，工作机差异 ±30%）。

### 维护纪律（针对新加测试）

写新的离线测试时：

- 在 `Command` 后要看屏幕就直接 `WaitForScreenText(needle, timeout)` —— 不要 `Wait(2s) + AssertScreenContains` 双步。
- 在 `SkipTrustGate` 后不要再插 `Wait(2s)`；trust gate 自己睡了 render。
- 想 snapshot 调试时留一个 `Wait(Duration::from_millis(500))` 短窗口，不要 `Wait(2s)`。
- 替代方案是 clippy-style 检查（待补）：作为本计划 Task 4 之后的收尾项。

### 并发与锁（Phase 2 + Phase 3 隔离）

历史上 `PtySession::spawn` 持有一个全局 `OnceLock<Mutex<()>>` 守卫 `_serial_guard`，**把整个会话生命周期串行化**——任何时刻只有一个 cc-rust 在 PTY 里跑。Phase 2 已落地：

- **lift 全局串行锁**：`PtySession` 不再持 guard；并发 invariant 由 `concurrent_sessions_do_not_serialize` 测试钉住（两线程同时 spawn 同会话 → overlap ≥ 2）。
- **收窄到 `cleanup_lock()`**：仅在 `cleanup_detached_workspace_processes()` 这段"扫 `/proc` + 发信号"短临界区持锁，避免两个并发收尾重复扫同一组 pid。
- **per-process 日志根**：`logs_dir()` 加 PID 后缀，nextest 多进程同秒领号互不覆盖；`test_subdir(test_name)` 已天然按测试名隔离。

Phase 2 初次实现时，`nextest` `tui_pty_e2e` test-group 仍保持 `max-threads = 1`。当时跑 16 个离线测试的抽样结果如下：

| `max-threads` | 通过 | 现象 |
|---------------|------|------|
| 1 | 16/16 | ✅ 稳定 |
| 2 | 14/16 | `effort_shows_current` / `config_alias_settings` flake（`WaitForScreenText` 在 3s 内未等到屏幕文字） |
| 4 | 13/16 | 上限；后续 cc-rust 启动被拖慢到 window 外 |

根因不是锁本身，而是所有 cc-rust 子进程曾共用同一个 `/tmp/cc-rust-e2e-test` workspace——并发的 `settings.json` / `sessions.db` 读写彼此竞争。Phase 3 已把默认路径改为 `/tmp/cc-rust-e2e-test-{runner_pid}`：nextest 每个 test case 使用独立 runner 进程，因此 workspace 隔离；显式 `E2E_WORKSPACE` 覆盖仍保持原语义。普通 libtest 的 test case 共用 runner PID，所以全套回归仍必须传 `--test-threads=1`。

隔离后依次验证 `max-threads=2` 的 `commands_core_info` 25/25（59.529s）、`max-threads=4` 25/25（32.027s），以及 `max-threads=10` 25/25（real 17.85s）。10 并发的最终完整回归为 220 passed、36 skipped、0 failed，nextest summary 143.308s、命令墙钟 144.65s。因此 `.config/nextest.toml` 现已正式提升到 10；若后续新增跨 workspace 的共享状态或宿主机容量回归，必须用完整套件证据决定是否回退。

首次 10-way 全跑曾暴露 `mcp_help_no_args` 的陈旧断言：`/mcp` 已打开完整 MCP server surface，但测试仍等待旧 help 文本中的小写 `list`。断言改为当前稳定可见语义 `MCP servers` 后，目标测试 1/1、整个 MCP 模块 19/19、最终完整套件 220/220 均通过。

#### 历史滤片 bug 与修复（2026-07-18 followup）

最初落地的 `nextest.toml` override 滤片写成了 `package(allthecodes) & test(pty_tui_e2e)`。**这是错的**：nextest 的 `test(NAME)` 按"测试名"（如 `welcome::shows_prompt_on_startup`）匹配，而 `pty_tui_e2e` 是测试 **binary** 名——没有任何测试名包含这个字面值，所以该 override 命中 0 个测试：

```bash
$ cargo nextest show-config test-groups
group: tui_pty_e2e (max threads = 1)
    (no matches)
```

`max-threads=1` 因此根本没生效，`cargo nextest run -p allthecodes --test pty_tui_e2e` 实际按 nextest 默认线程池并发跑全 217 个测试 → 共享 `/tmp/cc-rust-e2e-test` workspace 出现约 24/217 的 flake（与上表 "max-threads=2 flakes ~2/16" 是同一个根因，只是没被关回去）。`cargo test`（不读 nextest.toml）多线程跑同样 flake；只有 `cargo test -- --test-threads=1` 强制串行才能避开——这也是历史上 Phase 2 "16/16 稳定" 抽样看上去 OK 的原因（抽样规模小，恰好没撮到 flake 测试）。

修复：把 override 滤片改成 `binary(pty_tui_e2e)`（按测试 binary 名匹配），`tui_pty_e2e` group 才真正接管该 binary。该修复最初强制 `max-threads=1`；Phase 3 workspace 隔离完成并通过完整回归后，预算已提升为 4：

```toml
[[profile.default.overrides]]
filter = 'binary(pty_tui_e2e)'
test-group = 'tui_pty_e2e'
slow-timeout = { period = "45s", terminate-after = 2 }
```

滤片修复后的历史串行基线为全 217 个离线测试通过、1250.847s。Phase 3 隔离后的当前 10-way 完整回归为全 220 个离线测试通过、144.65s；此前 flaky 的 `effort_shows_current` / `config_alias_settings` / `welcome::wide_terminal_shows_integrated_nine_grid_logo` 也包含在这次全绿结果中。

#### 跑全套离线回归的正确姿势

| 命令 | 串行？ | 说明 |
|------|--------|------|
| `cargo nextest run -p allthecodes --test pty_tui_e2e --no-fail-fast` | 10-way | ✅ **推荐**。nextest 读 `.config/nextest.toml` 的 `tui_pty_e2e.max-threads=10`；runner PID 隔离默认 workspace。 |
| `cargo test -p allthecodes --test pty_tui_e2e -- --test-threads=1 --nocapture` | ✅ libtest 强制单线程，等价串行；不依赖 nextest。 |
| `cargo test -p allthecodes --test pty_tui_e2e -- --nocapture` | ❌ **不要直接用**：cargo test 不读 nextest.toml，且同一 runner PID 下的 case 仍共享默认 workspace。 |

> 单跑一个测试（如 `cargo nextest run -p allthecodes --test pty_tui_e2e -- filter` 或 `cargo test -p allthecodes --test pty_tui_e2e -- filter`）不受此影响，因为单测本身不并发。

### 收尾 / cleanup 节流（Phase 2 Task 5）

`capture_output_after_finish` 里两次常量被收紧：

| 参数 | 旧 | 新 |
|------|----|----|
| 收尾 buffer flush sleep | 200ms | 100ms |
| reader-thread join deadline | 500ms | 250ms |

`cleanup_detached_workspace_processes()` 自带 2s 节流窗（`OnceLock<Mutex<Option<Instant>>>` + `CLEANUP_THROTTLE`）：首次调用必定扫描 `/proc`；若距上次扫描不足 2s，后续调用才跳过，原子计数 `CLEANUP_SKIP_COUNT` 自增供测试观测。`cleanup_throttle_scans_first_call_then_throttles` 钉住“首次扫描 1 次、窗口内随后跳过”的 invariant，避免 nextest 每个 runner 唯一一次 cleanup 被错误节流。

## 添加新测试

### 方式 1：使用模板引擎（推荐）

在 `script.rs` 的 `#[cfg(test)] mod tests` 中添加：

```rust
#[test]
#[ignore = "requires real API key"]
fn script_my_new_test() {
    let case = TestCase::new("my_new_test")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Input("your prompt here".into()))
        .step(TestStep::WaitForText("expected".into(), API_TIMEOUT))
        .step(TestStep::Snapshot("result".into()));

    TestRunner::new().run(&case).assert_no_errors();
}
```

### 方式 2：直接使用 Harness

创建新文件 `my_tests.rs`，在 `main.rs` 中添加 `mod my_tests;`：

```rust
use crate::harness::*;
use std::time::Duration;

#[test]
fn my_test() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    session.send_line("/help");
    std::thread::sleep(Duration::from_secs(2));

    let screen = session.current_screen();
    session.send_ctrl_d();
    let output = session.finish(QUICK_TIMEOUT, "my_test");

    assert!(!output.contains("panicked"));
    assert!(screen.contains("help"));
}
```

## 依赖

```
portable-pty    # 伪终端创建
vt100           # 终端屏幕解析
strip-ansi-escapes  # ANSI 转义序列去除
chrono          # 时间戳（日志目录命名）
tempfile        # 临时目录（测试隔离）
assert_cmd      # cargo_bin() 二进制路径解析
which           # PATH 中查找二进制
dirs            # home 目录解析
serde_json      # settings.json 解析
unicode-width   # Unicode 字符宽度（HTML 渲染）
```
