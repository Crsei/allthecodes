# allthecodes 代码安全与可靠性审计风险记录

> 日期：2026-07-10  
> 状态：待修复  
> 审计范围：Rust workspace 的 Web、Daemon、Gateway、Engine、IPC、MCP、插件、Sandbox、认证、配置、进程生命周期和 CI 质量门。  
> 说明：本文只记录已经通过源码调用链或实际命令确认的问题；未读取 `.env`、token 或其他真实凭据。

## 1. 执行摘要

本轮审计确认以下主要风险：

- Web 控制面缺少统一认证，终端和文件接口可被未认证调用。
- Daemon assistant worker 的心跳与长任务执行耦合，正常执行超过 10 秒可能被误判 stale 并终止。
- Daemon Abort 与 Submit 共用串行队列，无法及时取消当前运行。
- Daemon、MCP、REPL、快捷 Bash、TerminalCapture 等路径存在子进程或进程树清理不完整。
- Headless IPC 可并发启动多个共享状态 turn，且 permission/question 回调存在响应丢失竞态。
- OAuth 凭据和 Daemon control token 的文件权限依赖 umask。
- Sandbox 在隔离原语不可用时默认 fail-open。
- 插件下载和解压缺少统一资源上限。
- Workspace lint、feature/platform/npm 构建和依赖漏洞扫描覆盖不足。
- 实际 `cargo test --workspace` 未通过，PTY TUI E2E 有 21 个失败。

建议将严重项和高危项视为发布阻塞项，不应仅记录为后续优化。

## 2. 实际验证基线

使用仓库规定的本地 Rust 工具链与仓库外 `CARGO_TARGET_DIR` 执行：

| 命令 | 结果 | 说明 |
|---|---:|---|
| `cargo fmt --all --check` | 通过 | 退出码 0，约 5 秒 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 退出码 0，使用已有构建缓存 |
| `cargo test --workspace` | 失败 | 退出码 101，约 2188 秒 |

Workspace 测试最终结果：

- 192 passed
- 21 failed
- 34 ignored
- 失败集中在 `crates/allthecodes/tests/pty_tui_e2e/`
- 完整运行日志：`/tmp/allthecodes-audit-20260710/test.log`

其中 `script_default_kill_teardown_does_not_wait_case_timeout` 实际 teardown 用时约 120.93 秒，测试要求小于 8 秒：

- `crates/allthecodes/tests/pty_tui_e2e/script.rs:1077-1092`

此外，本机未安装 `cargo-audit`、`cargo-deny`、`osv-scanner`，因此本文不声称依赖无已知 CVE。

## 3. 严重风险

### C-01 Web 控制面无统一认证，可创建终端并执行任意程序

**状态：待修复**

证据链：

- Web router 注册全部协议接口但没有统一认证中间件，并使用 permissive CORS：
  - `crates/allthecodes-web/src/mod.rs:31-85`
- `POST /api/terminal/sessions` 和终端 WebSocket 直接公开：
  - `crates/allthecodes-web/src/handler_registry.rs:631-660`
- 请求可指定任意绝对路径或 PATH 中的 executable 和参数：
  - `crates/allthecodes-web/src/ws/terminal.rs:663-683`
  - `crates/allthecodes-web/src/ws/terminal.rs:1361-1406`
- PTY 最终启动调用者选择的程序：
  - `crates/allthecodes-web/src/ws/terminal.rs:396-408`
  - `crates/allthecodes-web/src/ws/terminal.rs:942-953`
- 专用 terminal WebSocket 未验证 `Origin`：
  - `crates/allthecodes-web/src/ws/terminal.rs:1021-1036`
- `--listen` 可接受任意 `SocketAddr`，ServerManager 直接绑定：
  - `crates/allthecodes-server/src/transport.rs:187-196`
  - `crates/allthecodes-server/src/server_manager.rs:132-169`

默认绑定 loopback 只能降低直接网络暴露面，不能替代认证；显式绑定非回环地址时，该问题成为远程命令执行。loopback 场景仍受恶意网页、本机低权限进程及多用户主机威胁。

修复要求：

1. 除健康检查和静态资源外，所有 Web REST/WS 路由统一强制认证。
2. 非 loopback 监听在没有 TLS 和显式远程 token 时必须拒绝启动。
3. 终端、文件写入、插件安装和凭据管理使用独立高权限 capability。
4. 使用精确 Origin allowlist，并对全部 WebSocket 统一校验 Origin/Host/token。
5. 增加未认证、错误 Origin、缺失 token、非回环监听的负向集成测试。

### C-02 Assistant worker 正常执行超过 10 秒可能被 supervisor 误杀

**状态：待修复**

证据链：

- stale 阈值固定为 10 秒：
  - `crates/allthecodes-daemon/src/supervisor.rs:30-32`
- worker 只在命令消费循环外层写心跳：
  - `crates/allthecodes-daemon/src/supervisor.rs:329-340`
- worker 串行完整等待 `handle_worker_command`：
  - `crates/allthecodes-daemon/src/supervisor.rs:341-352`
- Submit 一直消费 engine stream 到结束：
  - `crates/allthecodes-daemon/src/gateway_bridge.rs:612-667`
- supervisor 超过 10 秒未见心跳即终止并准备重启 worker：
  - `crates/allthecodes-daemon/src/supervisor.rs:196-223`

正常模型请求、权限等待或工具调用很容易超过 10 秒，因此这是稳定可达的产品路径。影响包括流式输出中断、run 失败、worker 重启、工具子进程遗留及持久状态不一致。

修复要求：

1. 心跳由独立任务更新，不能依赖命令消费循环。
2. stale 判定区分“进程失活”和“正在执行长任务”。
3. 引入 command lease/active-run 状态及单调时间。
4. 增加持续 30 秒以上 Submit 不被误杀的回归测试。

### C-03 Daemon Abort 无法中断正在执行的 Submit

**状态：待修复**

证据链：

- HTTP abort 只将 Abort 写入 command queue：
  - `crates/allthecodes-daemon/src/routes.rs:506-526`
- worker 串行领取并完整等待每个命令：
  - `crates/allthecodes-daemon/src/supervisor.rs:341-347`
- Submit handler 一直等待整个 engine stream：
  - `crates/allthecodes-daemon/src/gateway_bridge.rs:653-667`
- 真正的 `runtime.abort()` 只有 Abort 命令被领取后才执行：
  - `crates/allthecodes-daemon/src/gateway_bridge.rs:431-439`

Submit 运行时，Abort 只能排在它后面，必须等 Submit 自己结束才能执行，取消已失去意义。

修复要求：

1. Abort、permission response、shutdown 等控制信号与 Submit 数据队列分离。
2. 当前 run 持有独立 cancellation token。
3. HTTP abort 直接触发 cancellation token，再异步持久化 command/event。
4. 使用 `tokio::select!` 同时驱动当前 run 和控制信号。

## 4. 高危风险

### H-01 工作区文件 API 无认证，可读写或删除整个工作区

**状态：待修复**

- 文件读取、写入、上传、复制、移动和删除路由直接注册：
  - `crates/allthecodes-web/src/handler_registry.rs:679-702`
- 文件读取和写入实现：
  - `crates/allthecodes-web/src/handlers/files.rs:798-845`
  - `crates/allthecodes-web/src/handlers/files.rs:1051-1111`
- 路径限制和 symlink/path traversal 防护位于：
  - `crates/allthecodes-web/src/handlers/files.rs:52-167`

未确认工作区外路径穿越；问题是工作区内所有文件能力暴露给未认证调用者。攻击者可读取源码和 `.env`、植入代码、修改构建脚本或删除文件。

### H-02 Daemon 历史、SSE、账户和部分控制接口缺少 token 保护

**状态：待修复**

- Daemon router 使用 permissive CORS：
  - `crates/allthecodes-daemon/src/server.rs:19-31`
- `/api/history`、`/api/status`、`/api/attach`、bridge session 和账户接口未统一认证：
  - `crates/allthecodes-daemon/src/routes.rs:253-304`
- status 暴露 PID、worker 和本地绝对路径：
  - `crates/allthecodes-daemon/src/routes.rs:691-735`
- attach/history 返回历史事件和消息：
  - `crates/allthecodes-daemon/src/routes.rs:755-765`
  - `crates/allthecodes-daemon/src/routes.rs:800-812`
- SSE 不验证 control token：
  - `crates/allthecodes-daemon/src/sse.rs:31-105`
- logout 可清除本地登录态：
  - `crates/allthecodes-daemon/src/account_auth.rs:253-266`

除健康检查外，Daemon 路由应统一经过认证中间件；SSE 应使用 header 或一次性连接票据认证。

### H-03 OAuth 凭据和 Daemon control token 未强制私有权限

**状态：待修复**

- `credentials.json` 包含 access/refresh token：
  - `crates/allthecodes-auth/src/token.rs:13-24`
- 保存只使用 `create_dir_all` 和 `std::fs::write`：
  - `crates/allthecodes-auth/src/token.rs:37-45`
- 数据根目录没有强制 `0700`：
  - `crates/allthecodes-config/src/paths.rs:45-59`
- control token 通过通用 JSON 原子写入：
  - `crates/allthecodes-daemon/src/process_state/storage.rs:394-429`
- 底层使用 `File::create`，没有 `0600`：
  - `crates/allthecodes-daemon/src/process_state/mod.rs:49-89`

审计环境实测 umask 为 `0002`。普通新文件可能成为 `0664`，目录可能成为 `0775`。多用户或共享组环境下，其他用户可能读取凭据或 control token。

修复时应将 `~/.allthecodes` 和 daemon 目录收紧为 `0700`，敏感文件使用原子 `0600` 创建，并迁移已存在文件；Windows 使用仅当前用户可访问的 ACL。

### H-04 Daemon server 启动失败会绕过清理

**状态：待修复**

- server 绑定前已写入 started 状态：
  - `crates/allthecodes/src/full_init.rs:164-170`
- 随后启动 supervisor：
  - `crates/allthecodes/src/full_init.rs:195-201`
- `manager.start(...).await?` 失败时直接返回：
  - `crates/allthecodes/src/full_init.rs:203-204`
- cleanup 位于其后：
  - `crates/allthecodes/src/full_init.rs:207-217`

端口占用或监听失败时可能留下 worker、supervisor、team-memory 子进程和错误的 running 状态。应使用生命周期 guard，并仅在成功绑定且 ready 后写 started。

### H-05 `drop(supervisor_handle)` 不会停止 supervisor

**状态：待修复**

- supervisor spawn 和 handle drop：
  - `crates/allthecodes/src/full_init.rs:199-207`
- supervisor 仍会继续 poll 和重启 worker：
  - `crates/allthecodes-daemon/src/supervisor.rs:189-238`
  - `crates/allthecodes-daemon/src/supervisor.rs:300-316`

Tokio JoinHandle 被 drop 只会 detach。cleanup 终止 worker 后，仍运行的 supervisor 可能重新拉起 worker，导致 stopped 状态与真实进程不一致。

### H-06 Supervisor 部分启动失败不会回滚已启动 worker

**状态：待修复**

- `start_all` 逐个启动，错误立即返回：
  - `crates/allthecodes-daemon/src/supervisor.rs:164-169`
- 已启动 child 被插入 registry：
  - `crates/allthecodes-daemon/src/supervisor.rs:276-284`

`std::process::Child` 被 drop 不会终止进程。后续 worker 启动失败时，之前已启动的 worker 会失去 supervisor 管理。

### H-07 Unix `terminate_process_tree` 实际只终止根 PID

**状态：待修复**

- Unix soft terminate：
  - `crates/allthecodes-daemon/src/process_state/platform.rs:119-126`
- Unix force kill：
  - `crates/allthecodes-daemon/src/process_state/platform.rs:129-135`
- tree wrapper：
  - `crates/allthecodes-daemon/src/process_state/platform.rs:164-173`

代码向正 PID 发送 SIGTERM/SIGKILL，没有建立/终止进程组，也没有遍历后代。worker 启动的工具、MCP 或后台服务可能在 worker 被终止后继续运行。

### H-08 `daemon start` readiness 失败后保留已 spawn 的 daemon

**状态：待修复**

- `crates/allthecodes-daemon/src/process_state/management.rs:110-129`

`wait_for_ready(...)?` 失败时，局部 `std::process::Child` 被 drop，但进程不会终止。CLI 报告启动失败后 daemon 仍可能运行，并导致后续启动端口冲突。

### H-09 Headless IPC 可在同一个 QueryEngine 上并发运行多个 turn

**状态：待修复**

- `SubmitPrompt` 没有 busy/single-flight 检查，直接 `begin_turn` 和 spawn：
  - `crates/allthecodes/src/app_runtime_adapters/ingress.rs:47-108`
- 每次提交创建 detached task，并调用共享引擎的 `reset_abort()`：
  - `crates/allthecodes-ipc/src/client/query_runner.rs:12-43`

多个 turn 可并发修改 transcript、usage、recorder、工具状态和 abort 标志；新 turn 还可能清除旧 turn 的取消。应实现 single-flight，或将状态、cancel token、event sink 和 JoinHandle 完全 turn-local 化。

### H-10 REPL 和 `!command` 超时后进程仍可能继续执行

**状态：待修复**

REPL：

- 声明支持 Cancel，但 call 不读取 abort signal：
  - `crates/allthecodes-engine/src/tools/exec/repl.rs:67-69`
  - `crates/allthecodes-engine/src/tools/exec/repl.rs:101-107`
- 只对 `cmd.output()` 使用 timeout：
  - `crates/allthecodes-engine/src/tools/exec/repl.rs:143-150`
  - `crates/allthecodes-engine/src/tools/exec/repl.rs:189-193`

快捷 Bash：

- `crates/allthecodes-engine/src/lifecycle/submit_message/command_handling.rs:194-217`

两条路径都没有可靠的 child handle、进程组、kill/wait 清理。工具返回 timeout 后，代码仍可能在后台修改文件、访问网络或启动后代进程。

### H-11 MCP 正常关闭和 disconnect 不能保证清理完整进程树

**状态：待修复**

- stdio spawn 没有进程组或 `kill_on_drop`：
  - `crates/allthecodes-mcp/src/client/stdio.rs:36-50`
- disconnect 只 kill 直接 child：
  - `crates/allthecodes-mcp/src/client/mod.rs:379-405`
- Drop 只 `start_kill()` 直接 child：
  - `crates/allthecodes-mcp/src/client/mod.rs:929-935`
- manager 被静态全局强引用：
  - `crates/allthecodes-mcp/src/runtime.rs:24-35`
- graceful shutdown 未调用 `disconnect_all()`：
  - `crates/allthecodes/src/shutdown.rs:64-167`

需要把 MCP manager 纳入显式 runtime owner，并在 shutdown 中等待 `disconnect_all()`；stdio server 使用独立进程组。

### H-12 TerminalCapture 超时/取消可能永久等待 reader

**状态：待修复**

- command spawn：
  - `crates/allthecodes-tools/src/product/capture.rs:299-315`
- timeout/abort 只 kill 直接 shell：
  - `crates/allthecodes-tools/src/product/capture.rs:345-374`
- reader 等待 pipe EOF：
  - `crates/allthecodes-tools/src/product/capture.rs:405-435`

若后代继承 stdout/stderr，shell 被杀后 pipe 仍保持打开，reader 不会收到 EOF，主 future 又无二次 timeout 地等待 reader JoinHandle，导致 timeout/cancel 分支本身卡住。

## 5. 中危风险

### M-01 IPC permission/question 存在快速响应丢失竞态

**状态：待修复**

当前顺序为先向 frontend 发送 request，随后才创建 channel 并写入 pending map：

- permission：`crates/allthecodes-ipc/src/client/callbacks.rs:52-64`
- question：`crates/allthecodes-ipc/src/client/callbacks.rs:127-142`

frontend 快速响应时，pending 尚不存在，响应会被丢弃；callback 随后又无 timeout 地等待。应先登记 pending，再发送 request，并为等待增加 timeout/cancellation。

### M-02 MCP reader 退出后状态仍可能显示 Connected

**状态：待修复**

- reader EOF/错误路径只清理 pending：
  - `crates/allthecodes-mcp/src/transport.rs:80-103`
- 连接时写入 Connected：
  - `crates/allthecodes-mcp/src/client/stdio.rs:95-105`
- 状态只在显式 disconnect 时变更：
  - `crates/allthecodes-mcp/src/client/mod.rs:397-415`

MCP server 崩溃后，UI 和工具注册表可能继续显示失效工具，直到后续调用通过 broken pipe 或 timeout 失败。

### M-03 插件下载和解压没有统一资源上限

**状态：待修复**

- URL 下载完整读入内存：
  - `crates/allthecodes-plugins/src/sources.rs:53-78`
- GitHub/npm 路径也完整缓冲响应：
  - `crates/allthecodes-plugins/src/sources.rs:135-169`
  - `crates/allthecodes-plugins/src/sources.rs:206-239`
- 普通安装使用无统一总量限制的解压：
  - `crates/allthecodes-plugins/src/installation.rs:184-213`
  - `crates/allthecodes-plugins/src/zip_cache.rs:106-153`
- TGZ 完整解压到 `Vec`：
  - `crates/allthecodes-plugins/src/zip_cache.rs:307-312`

攻击者控制的超大响应、ZIP bomb 或 TGZ bomb 可导致 OOM 或磁盘耗尽。所有来源应统一采用流式下载、下载大小、条目数、单文件和总展开大小上限。

### M-04 MCP 启动日志记录完整命令参数

**状态：待修复**

- `crates/allthecodes-mcp/src/client/stdio.rs:20-34`

MCP 配置若在 args 中携带 token、密码或签名 URL，完整值会进入 info 日志。默认应只记录 server 名、executable basename 和参数数量。

### M-05 Sandbox 原语不可用时默认 fail-open

**状态：待修复**

- `fail_if_unavailable` 默认 false，`allow_unsandboxed_commands` 默认 true：
  - `crates/allthecodes-sandbox/src/policy.rs:356-368`
- 不支持时只 warning 并原样返回命令：
  - `crates/allthecodes-sandbox/src/runner.rs:563-591`

用户启用 Sandbox 后仍可能在宿主执行命令。建议 Sandbox 启用时默认 fail-closed，只有显式同意才允许降级。

### M-06 API RPC info 日志记录完整入站 JSON

**状态：待修复**

- `crates/allthecodes-web/src/ws/api_rpc.rs:184-205`

完整 JSON-RPC frame 可能包含 prompt、文件内容、命令参数、MCP 配置和认证字段。默认只应记录 method、request id、payload 长度和脱敏字段。

### M-07 Team-memory 的 5 秒 deadline 可被单次 HTTP await 绕过

**状态：待修复**

- `crates/allthecodes-daemon/src/team_memory_proxy.rs:89-110`

deadline 只在每次请求前检查，`client.get(...).send().await` 本身没有 request timeout。目标端口接受连接但不返回响应时，daemon 初始化可无限卡住。

### M-08 Status-line command 只杀 shell，不保证杀后代

**状态：待修复**

- shell spawn 与 `kill_on_drop(true)`：
  - `crates/allthecodes-engine/src/status_line/runner.rs:213-246`
- timeout：
  - `crates/allthecodes-engine/src/status_line/runner.rs:270-287`

`kill_on_drop` 只作用于直接 shell，孙进程可能继续运行并持有 stdout/stderr pipe。

### M-09 PTY TUI E2E 当前有 21 个失败

**状态：待修复**

失败覆盖：

- teardown 延迟；
- CommandSurface 文本和快捷键；
- MCP、skills、hooks、memory 等 surface；
- running-task Tab 行为；
- login/full-access 流程。

主要测试文件：

- `crates/allthecodes/tests/pty_tui_e2e/script.rs`
- `crates/allthecodes/tests/pty_tui_e2e/tests/commands_surface.rs`
- `crates/allthecodes/tests/pty_tui_e2e/tests/running_task_slash_commands.rs`
- `crates/allthecodes/tests/pty_tui_e2e/tests/test1_login_structure.rs`
- `crates/allthecodes/tests/pty_tui_e2e/tests/test2_full_access.rs`

不能将这些失败整体视为快照陈旧，应逐项区分产品行为回归、测试隔离问题和预期文本漂移。

## 6. CI 与供应链覆盖缺口

### Q-01 Workspace lint 没有被成员 crate 继承

根 manifest 定义：

- `Cargo.toml:216-220`

包括 `unwrap_used`、`expect_used`、`panic` 和 `panic_in_result_fn`，但成员 crate 没有：

```toml
[lints]
workspace = true
```

因此这些 lint 实际未启用，CI 的 `-D warnings` 也无法提升未启用的 lint。

### Q-02 非默认 feature 和代码生成 binary 缺少 CI 覆盖

CI 默认 feature 以外主要只测试 `allthecodes-tools`，没有系统覆盖：

- `allthecodes` telemetry/tree-sitter；
- protocol schema/codegen binary；
- 多个替代存储 feature；
- 全 feature 或有代表性的 feature matrix。

位置：

- `.github/workflows/ci.yml:42-55`
- `crates/allthecodes/Cargo.toml:19-32`
- `crates/allthecodes-protocol/Cargo.toml:7-30`

### Q-03 六平台发布，但 PR CI 仅运行 Linux

- CI runner：`.github/workflows/ci.yml:15-18`
- release matrix：`.github/workflows/release.yml:61-86`
- release 仅构建目标 binary，不运行目标平台测试/Clippy：
  - `.github/workflows/release.yml:112-131`

Windows/macOS 条件代码和链接问题可能延迟到打 tag 后才暴露。

### Q-04 npm launcher 和打包链路没有 PR 级 smoke test

- Node launcher：`bin/allthecodes.js:19-80`
- Python 打包：`scripts/build_npm_package.py:174-231`
- release npm 组装：`.github/workflows/release.yml:211-278`
- 当前 CI：`.github/workflows/ci.yml:42-55`

建议在 PR CI 中增加 launcher 单测、`npm pack` 和本地安装 smoke test。

### Q-05 Cargo CI/release 命令未使用 `--locked`

- `.github/workflows/ci.yml:43-55`
- `.github/workflows/release.yml:112-131`
- `scripts/cargo-build-test.sh:132-153`

建议 CI 和 release 使用 `--locked`，保证已提交 `Cargo.lock` 与 manifest 一致。

### Q-06 缺少依赖漏洞与许可证审计

未发现 CI 中执行 `cargo audit`、`cargo deny` 或 OSV 扫描。建议增加锁文件漏洞扫描、许可证/来源策略和明确的例外配置。

## 7. 明确未确认的问题

本轮未确认以下问题，不应在后续报告中误写为已证实漏洞：

- Web 文件 API 的工作区外路径穿越；
- 可直接利用的插件 Zip Slip；
- Gateway 远程认证绕过；
- Keychain 服务名使用了原版项目名称；
- 明显不安全的通用反序列化；
- `unwrap`、TODO 或监听地址搜索命中本身构成漏洞。

## 8. 修复顺序

建议按以下顺序实施：

1. C-01：Web 控制面统一认证、Origin 校验和非 loopback fail-closed。
2. C-02：worker 独立心跳，避免长 Submit 被误杀。
3. C-03：建立可抢占的 Abort 控制通道。
4. H-04/H-05/H-06/H-08：统一 Daemon 启动、关闭、失败回滚生命周期。
5. H-07/H-10/H-11/H-12/M-08：统一进程组与后代清理。
6. H-09/M-01：修复 IPC single-flight 和 pending-response 竞态。
7. H-01/H-02/H-03：文件 API、Daemon 读取接口和敏感文件权限。
8. M-03/M-04/M-05/M-06：插件资源限制、日志脱敏和 Sandbox fail-closed。
9. M-09：逐项修复 21 个 PTY E2E 失败。
10. Q-01 至 Q-06：补齐 lint、feature、平台、npm 和供应链质量门。

## 9. 完成判定

只有满足以下条件后，本文任务才可标记为完成：

- 所有 C/H 项均已有实现、负向测试和回归测试。
- Web/Daemon 非健康路由默认需要认证。
- 长时间 Submit 不再触发 worker stale，Abort 可在受控时间内中断当前 run。
- 所有外部子进程路径在正常退出、超时、取消和 owner drop 时均清理进程组并 wait/reap。
- 敏感目录和文件权限在 Unix/Windows 均有测试。
- `cargo fmt --all --check`、workspace Clippy 和 `cargo test --workspace` 全部通过。
- CI 覆盖关键 feature、Windows/macOS、npm smoke、`--locked` 和依赖审计。

在上述验收项完成前，本文件状态保持“待修复”。
