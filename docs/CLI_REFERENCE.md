# cc-rust CLI 接口参考

> 以当前代码实现为准，主要来源：
> - `crates/claude-code-rs/src/main.rs` 中的 `clap` 参数定义
> - `run_full_init()` 中的模式分派顺序
> - `crates/cc-commands/src/lib.rs` 中的斜杠命令注册表
>
> 本文覆盖两层接口：
> 1. 进程启动参数（真正的 CLI 参数）
> 2. 进入 REPL 后可用的斜杠命令（交互式 CLI）
>
> 每个斜杠命令的子参数和示例见 [`COMMAND_REFERENCE.md`](COMMAND_REFERENCE.md)。

## 1. 可执行入口

当前 Cargo 包名是 `claude-code-rs`，因此默认生成的二进制通常是：

```bash
claude-code-rs
```

在 `clap` 定义中命令名被设置成了 `claude`，但实际从 Cargo 构建出来的文件名仍然是 `claude-code-rs(.exe)`。除非后续单独做了安装别名，否则仓库内文档与脚本应优先使用 `claude-code-rs` 这个真实二进制名。

## 2. 启动模式

同一套 CLI 参数会根据组合方式进入不同运行模式。当前代码里的优先级如下：

1. `--version`
2. `--dump-system-prompt`
3. `--init-only`
4. `--output-format json`
5. `--print`
6. `--web`
7. `--daemon`
8. `--headless`
9. 默认交互式 TUI / REPL

这意味着：

- 如果同时传了 `-p` 和 `--output-format json`，会走 JSON 模式，不走纯文本 print 模式。
- 如果同时传了 `--web` 和 `--daemon`，会先进入 `--web`。
- 如果同时传了 `--resume` 和 `--continue <id>`，会优先走 `--resume`。

## 3. 参数总表

### 3.1 对外可见参数

| 参数 | 短写 | 类型 | 默认值 | 说明 |
|------|------|------|--------|------|
| `--version` | `-V` | flag | `false` | 打印版本并退出 |
| `--print` | `-p` | flag | `false` | 非交互模式，输出模型回复后退出 |
| `--resume` | — | flag | `false` | 恢复最近一次会话 |
| `--continue <SESSION_ID>` | — | string | — | 恢复指定会话 ID |
| `--max-turns <N>` | — | usize | — | 限制 agentic loop 的最大轮数 |
| `--cwd <PATH>` | `-C` | string | 当前进程目录 | 覆盖工作目录，并在初始化早期执行 `set_current_dir()` |
| `--model <MODEL>` | `-m` | string | 由配置 / provider 默认模型决定 | 覆盖当前模型 |
| `--system-prompt <TEXT>` | — | string | — | 完全替换默认系统提示词 |
| `--append-system-prompt <TEXT>` | — | string | — | 在默认系统提示词后追加内容 |
| `--permission-mode <MODE>` | — | string | `default` | 权限模式。代码实际支持 `default` / `auto` / `bypass` / `plan` |
| `--verbose` | `-v` | flag | `false` | 提高 stderr 日志级别到 debug |
| `--max-budget <USD>` | — | f64 | — | 设置美元预算上限 |
| `--output-format <FMT>` | — | string | — | 输出格式声明。当前只有 `json` 被专门处理 |
| `--port <PORT>` | — | u16 | `19836` | daemon HTTP 端口，仅 `--daemon` 有效 |
| `--computer-use` | — | flag | `false` | 注册原生 Computer Use 工具（截图、点击、输入、滚动等） |
| `--web` | — | flag | `false` | 启动 Web UI 模式 |
| `--web-port <PORT>` | — | u16 | `3001` | Web UI HTTP 端口，仅 `--web` 有效 |
| `--no-open` | — | flag | `false` | Web UI 启动后不自动打开浏览器 |
| `[PROMPT]...` | — | repeatable positional | 空 | 位置参数，会被拼接成初始提示词 |

### 3.2 隐藏 / 内部参数

这些参数在源码中已实现，但默认 help 不展示：

| 参数 | 类型 | 说明 |
|------|------|------|
| `--dump-system-prompt` | flag | 打印完整系统提示词并退出 |
| `--init-only` | flag | 只做初始化，不进入任何交互模式 |
| `--headless` | flag | 无 TUI，通过 stdin/stdout JSONL 和前端通信 |
| `--daemon` | flag | KAIROS daemon 模式，需要 `FEATURE_KAIROS=1` |

## 4. 参数语义与注意事项

### 4.1 `PROMPT...`

`PROMPT` 是可重复位置参数，最终会被代码按空格拼接：

```bash
claude-code-rs 帮我 重构 这个 函数
```

等价于：

```text
"帮我 重构 这个 函数"
```

用途分三种：

- 默认 TUI 模式：作为进入 REPL 后自动提交的首条消息
- `--print`：作为一次性查询内容
- `--output-format json -p`：作为 SDK/JSON 模式的输入

### 4.2 `--print`

`--print` 走纯文本单次查询路径：

```bash
claude-code-rs -p "解释 main.rs"
```

注意：

- 纯 `--print` 模式当前要求位置参数里有 prompt
- 纯 `--print` 不会自动从 stdin 读取 prompt
- 只有 `--output-format json -p` 这条路径会在没有位置参数时从 stdin 读取全文

### 4.3 `--output-format`

`clap` help 里写的是 `text`, `json`, `stream-json`，但当前实现里只有：

- `json`：显式进入 JSONL 模式

除此之外：

- `text`：没有单独分支，本质上仍然走普通分派逻辑
- `stream-json`：当前代码里没有对应实现分支

因此如果要做 SDK / 机器消费，当前可靠用法是：

```bash
claude-code-rs --output-format json -p "你好"
```

或者：

```bash
echo "你好" | claude-code-rs --output-format json -p
```

### 4.4 `--permission-mode`

代码实际支持 4 种值：

| 值 | 行为 |
|----|------|
| `default` | 默认模式 |
| `auto` | 自动模式 |
| `bypass` | 绕过权限检查 |
| `plan` | 计划模式 / 只读倾向 |

任何未知值都会回退到 `default`。

### 4.5 `--cwd`

`--cwd` 不只是传给内部配置，而是会真的改变进程工作目录。后续工具（Bash / Glob / Grep / File tools）都会以这个目录作为默认工作区。

如果路径不存在或不是目录，程序会直接失败退出。

### 4.6 `--resume` 和 `--continue`

行为规则：

- `--resume`：加载最近一次会话
- `--continue <id>`：加载指定会话
- 两者同时存在时，`--resume` 优先

### 4.7 `--web` / `--web-port` / `--no-open`

Web 模式会启动内嵌 HTTP 服务并加载 `web-ui/dist` 里的前端资源。

```bash
claude-code-rs --web
claude-code-rs --web --web-port 3002 --no-open
```

说明：

- `--web-port` 仅 `--web` 时生效
- 默认会尝试自动打开浏览器
- 用 `--no-open` 禁止自动打开

### 4.8 `--daemon` / `--port`

Daemon 模式用于 KAIROS 常驻助手：

```bash
FEATURE_KAIROS=1 claude-code-rs --daemon
FEATURE_KAIROS=1 claude-code-rs --daemon --port 19837
```

说明：

- 没有 `FEATURE_KAIROS=1` 会直接报错退出
- `--port` 当前是 daemon HTTP 端口
- `--daemon` 是隐藏参数，属于内部/实验性运行面

### 4.9 `--computer-use`

启用后会在工具注册阶段额外加入本地桌面控制工具，不依赖外部 MCP server。

```bash
claude-code-rs --computer-use
```

### 4.10 `--system-prompt` 与 `--append-system-prompt`

两者作用不同：

- `--system-prompt`：替换默认系统提示词
- `--append-system-prompt`：保留默认系统提示词，并在尾部追加

### 4.11 `--verbose`

`--verbose` 影响 stderr 输出层的日志级别。文件日志本身仍会持续写入 debug 级别的详细内容。

## 5. 常见调用方式

### 5.1 默认交互式模式

```bash
claude-code-rs
claude-code-rs "先帮我看下这个仓库结构"
```

### 5.2 单次纯文本输出

```bash
claude-code-rs -p "列出当前目录的 Rust 模块"
```

### 5.3 JSONL / SDK 模式

```bash
claude-code-rs --output-format json -p "你好"
echo "你好" | claude-code-rs --output-format json -p
```

### 5.4 指定工作目录和模型

```bash
claude-code-rs -C F:\\AIclassmanager\\cc\\rust -m gpt-5.4
```

### 5.5 Web UI

```bash
claude-code-rs --web --web-port 3001
```

### 5.6 Headless / 前端桥接

```bash
claude-code-rs --headless
```

### 5.7 KAIROS daemon

```bash
FEATURE_KAIROS=1 claude-code-rs --daemon --port 19836
```

## 6. REPL 斜杠命令注册表

下面是当前 `crates/cc-commands/src/lib.rs` 中已注册的交互式命令。它们不属于进程启动参数，但属于进入 REPL 后的 CLI 接口面。

### 6.1 基础命令

| 命令 | 别名 | 说明 |
|------|------|------|
| `/help` | `/h`, `/?` | 显示命令列表或某个命令的帮助 |
| `/clear` | — | 清空当前对话历史 |
| `/exit` | `/quit`, `/q` | 退出 REPL |
| `/version` | `/v` | 显示版本 |

### 6.2 配置 / 模型 / 权限

| 命令 | 别名 | 说明 |
|------|------|------|
| `/config` | `/settings` | 查看或修改配置 |
| `/model` | — | 查看或切换当前模型 |
| `/model-add` | `/ma` | 向 `.env` 写入一个带定价的模型条目 |
| `/cost` | `/usage` | 查看当前会话 token / 成本 |
| `/extra-usage` | `/eu` | 查看扩展用量统计 |
| `/rate-limit-options` | `/rlo`, `/rate-limit` | 查看当前模型的限流信息 |
| `/effort` | — | 设置思考强度 |
| `/fast` | — | 开关 fast mode |
| `/permissions` | `/perms` | 查看或修改工具权限设置 |

### 6.3 会话 / 上下文 / 工作区

| 命令 | 别名 | 说明 |
|------|------|------|
| `/session` | — | 显示当前会话信息或列出已保存会话 |
| `/resume` | — | 恢复旧会话 |
| `/status` | — | 查看当前会话状态 |
| `/context` | `/ctx` | 查看上下文使用情况 |
| `/compact` | — | 压缩当前对话，减少 token 占用 |
| `/files` | — | 列出当前对话引用过的文件 |
| `/copy` | `/cp` | 复制最后一条 assistant 回复 |
| `/add-dir` | — | 增加一个工作目录 |
| `/init` | — | 初始化项目配置 |

### 6.4 认证

| 命令 | 别名 | 说明 |
|------|------|------|
| `/login` | — | 认证入口：Claude Code / Anthropic-compatible、OpenAI Codex、OpenAI API、Bedrock / Vertex |
| `/login-code` | — | 用授权码完成 OAuth 登录 |
| `/logout` | — | 清除已保存凭据 |

`/login` 当前支持的入口选项：

| 用法 | 说明 |
|------|------|
| `/login` | 显示认证菜单 |
| `/login status` | 查看当前认证状态 |
| `/login claude-code` | 切换到 Claude Code / Anthropic-compatible profile |
| `/login anthropic` | 兼容别名，等价于 `/login claude-code` |
| `/login sk-ant-...` | 直接保存 Claude Code / Anthropic-compatible API Key |
| `/login claude-ai` | Claude.ai OAuth |
| `/login console` | Console OAuth |
| `/login codex` | 切换到 OpenAI Codex profile |
| `/login codex-oauth` | OpenAI Codex OAuth |
| `/login codex-cli` | 从 `~/.codex/auth.json` 导入 / 刷新 |
| `/login custom` | 切换到已有 `authProfiles.custom` |
| `/login bedrock` | 当前会话启用 AWS Bedrock provider 并显示配置状态 |
| `/login vertex` | 当前会话启用 GCP Vertex AI provider 并显示配置状态 |
| `/login cloud` | 显示 Bedrock / Vertex 环境变量配置提示 |

### 6.5 Git / 输出导出

| 命令 | 别名 | 说明 |
|------|------|------|
| `/diff` | — | 查看当前 git diff |
| `/branch` | `/br` | fork 当前会话并切换到新 session |
| `/commit` | — | 基于当前变更创建提交 |
| `/export` | — | 导出对话为 Markdown |
| `/audit-export` | `/audit` | 导出可校验审计记录 |
| `/session-export` | `/sexport` | 导出结构化 session JSON 包 |

### 6.6 技能 / 记忆 / 扩展

| 命令 | 别名 | 说明 |
|------|------|------|
| `/memory` | `/mem` | 查看和管理 `CLAUDE.md` 项目指令 |
| `/skills` | — | 列出可用技能 |
| `/mcp` | — | MCP server 管理 |
| `/plugin` | — | 插件管理 |

### 6.7 KAIROS / Assistant 命令

| 命令 | 别名 | 说明 |
|------|------|------|
| `/brief` | — | 切换 Brief 输出模式 |
| `/sleep` | — | 设置 proactive sleep 时间 |
| `/assistant` | `/kairos` | 查看 assistant mode 状态 |
| `/daemon` | — | 查看或控制 daemon 进程 |
| `/notify` | — | 推送通知设置 |
| `/channels` | — | 查看已连接 channels |
| `/dream` | — | 从每日日志蒸馏记忆 |

## 7. 当前实现与文档差异提醒

这是这次整理里确认到的几个现状：

- `crates/claude-code-rs/src/main.rs` 的 help 文案里 `--permission-mode` 还没把 `plan` 写进去，但代码已经支持
- `--output-format` 的 help 文案列了 `stream-json`，但实现里目前只有 `json` 有专门分支
- daemon / headless 属于真实存在但默认隐藏的接口
- 旧文档中提到的前端名称、二进制名称、部分登录方式和模式说明已经过时，应以本文为准

后续如果继续整理，可以把每个斜杠命令的子参数和示例再单独拆成 `COMMAND_REFERENCE.md`。

## 8. Remote-Control Gateway CLI Release Gate (2026-05-08)

The remote-control gateway is available through the daemon and local slash-command surfaces. It is gated by the KAIROS daemon runtime:

```powershell
$env:FEATURE_KAIROS = "1"
cargo run -p claude-code-rs -- daemon start --port 19836
```

The loopback gateway uses the daemon control token published under `~/.cc-rust/daemon/**`. Non-loopback remote-control operation requires a configured remote token and must fail closed without one.

### `/remote` Slash Command

| Command | Purpose | Notes |
| --- | --- | --- |
| `/remote` | Open the TUI `RemoteSurface` when entered with no arguments. | No blocking network I/O is performed in the render path. |
| `/remote status` | Show daemon/gateway status, auth mode, capacity, endpoint count, and steer support. | Reports stopped/stale daemon states with actionable diagnostics. |
| `/remote adapters` | List Telegram and Lark adapter configuration/status. | Output is redacted and provider-neutral. |
| `/remote connect <telegram|lark>` | Run adapter connect/health check. | First version verifies outbound capability only. |
| `/remote test-message <telegram|lark> <target> [text]` | Send a bounded test message to an allowlisted target. | Does not start a model run. |
| `/remote runs [--limit N]` | List recent durable gateway runs from local storage. | Works even when the daemon is stopped. |
| `/remote show <run_id>` | Show one durable run. | Falls back to local store if the daemon is not reachable. |
| `/remote events <run_id> [--limit N]` | Show durable event replay. | Reads gateway events, not transient UI state. |
| `/remote stop <run_id>` | Request run cancellation through the gateway API. | Requires a running daemon. |
| `/remote doctor` | Show daemon status and gateway paths. | Includes gateway/runs/adapters/config/token paths. |

Example smoke commands:

```powershell
$env:FEATURE_KAIROS = "1"
$env:CC_RUST_HOME = "$env:TEMP\cc-rust-remote-control-smoke"
cargo run -p claude-code-rs -- daemon start --port 21990
cargo run -p claude-code-rs -- --print "/remote status"
cargo run -p claude-code-rs -- --print "/remote adapters"
cargo run -p claude-code-rs -- daemon stop
```

### TUI Surface And Indicator

- The command palette advertises `/remote <status|adapters|connect|test-message|runs|show|events|stop|doctor> ...`.
- `RemoteSurface` has status, adapters, runs, and security-oriented rows that route actions back to `/remote` commands.
- The status widget supports a remote indicator snapshot such as `remote=attention/warn`.
- Secrets, bearer tokens, provider credentials, webhook signatures, and idempotency keys must be redacted from command and TUI output.
