# 代码安全审计修复计划与完成状态

> 日期：2026-07-10
> 审计依据：[`code-security-audit-risks-2026-07-10.zh.md`](./code-security-audit-risks-2026-07-10.zh.md)
> 状态：修复实现已基本完成；本地质量门通过；跨平台 CI 与完整 PTY 总套件仍待最终验收。

## 1. 范围与验收原则

本轮覆盖 Rust workspace 的 Web/daemon 认证边界、IPC/MCP 并发和生命周期、敏感文件权限、sandbox 默认策略、插件资源限制、日志脱敏、CI/release/npm 质量门与供应链扫描。

验收不以清单勾选或“单个测试可通过”代替真实执行结果。只有取得对应命令的 exit 0，才记录为已通过。Windows/macOS 行为必须由对应 runner 验证，Linux 本机交叉编译不能替代运行时验证。

## 2. 已完成的修复

### 2.1 Critical / High

- [x] Web API/WS 控制面统一 token 校验、origin 限制与非 loopback 安全策略。
- [x] daemon 管理路由统一 control token 鉴权；CLI/e2e harness 使用发布的 token，不再匿名访问。
- [x] daemon 活跃 submit 的 abort/steer 控制命令与 heartbeat 并行处理。
- [x] daemon 启动失败回滚、readiness 失败清理、supervisor shutdown/abort 生命周期补齐。
- [x] Unix 进程组终止与 Windows `taskkill /T /F` 进程树终止路径补齐。
- [x] IPC turn single-flight、permission/question 回调先登记再发送与 timeout 清理。
- [x] credentials、session、daemon state/command、MCP OAuth/PKCE 文件使用统一私有权限实现：Unix 文件 0600、目录 0700；Windows 关闭 ACL 继承并只授予当前用户 FullControl。
- [x] Windows ACL 行为测试加入 `platform-check` Windows runner；Linux 本机 `x86_64-pc-windows-gnu` cfg Clippy `-D warnings` 已通过。

### 2.2 Medium

- [x] MCP stdio/SSE/HTTP 单消息上限统一为 8 MiB，模型上下文 resource 内容上限为 256 KiB，并使用 UTF-8 安全截断。
- [x] MCP reader 退出后更新 disconnected/health 状态；shutdown 在锁内摘取 clients、锁外 await。
- [x] MCP OAuth store 的 `exists()`/read TOCTOU 按空 store 安全处理；测试使用 serial + 独立 home。
- [x] 插件 HTTP 下载使用流式上限，archive 校验覆盖条目数、单项大小、总展开大小和压缩比。
- [x] MCP 启动日志不输出完整参数/敏感值；请求日志只保留结构化元数据和长度。
- [x] sandbox 默认 fail-closed；未显式允许时不降级执行 unsandboxed command。
- [x] status-line/REPL/quick-bash/daemon 子进程使用可取消、可超时的进程树清理。
- [x] Mutex poisoning、生产路径 `expect`/`unwrap`、Result 传播及 conservative provider capability fallback 已系统修复。
- [x] PTY 测试修复：离线 profile 的 `/effort high` 验证 fail-closed 诊断；`/fast` 断言匹配真实输出；kill teardown timing 只测 teardown，不再把并发启动排队计入上限。

### 2.3 Quality / CI / Supply Chain

- [x] workspace 全部 crate 继承统一 lint；Clippy `-D warnings` 门生效。
- [x] CI feature matrix 覆盖 all-features、no-default-features、storage 和 protocol codegen 组合。
- [x] Linux/macOS/Windows platform-check matrix 已加入 CI。
- [x] npm staging、pack/install、launcher smoke、平台包和 dist-tag 规则有脚本化测试。
- [x] `deny.toml` 使用 fail-closed licenses/sources/advisories/bans 策略。
- [x] `cargo audit` 只 narrow-ignore 无修复且在实际 target/feature graph 不可达的 `RUSTSEC-2023-0071`。
- [x] git2 等依赖已升级并完成破坏性 API 适配。

## 3. 本地验证结果

以下命令已取得 exit 0：

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --release`
- [x] `cargo test --locked -p allthecodes --test e2e_cli`（2/2）
- [x] `cargo test --locked -p allthecodes-mcp`（140/140）
- [x] ACL 相关 auth/config/session/daemon/MCP 定向测试（合计 743 passed，2 ignored）
- [x] PTY 定向回归：daemon teardown、unsupported effort、fast toggle 均通过
- [x] feature matrix、CI/npm 静态契约测试、Tools contract/full tests
- [x] `cargo audit --ignore RUSTSEC-2023-0071`
- [x] `cargo deny --locked check advisories bans licenses sources --warn unmaintained --warn unsound --hide-inclusion-graph`
- [x] `cargo clippy --locked -p allthecodes-config --target x86_64-pc-windows-gnu -- -D warnings`

完整 `cargo test --locked --workspace` 的最新两次运行均进入 247 项 `pty_tui_e2e` 后超过本地单命令 600 秒限制。首次运行发现并修复了 e2e daemon token、unsupported effort、Fast 输出大小写和 teardown timing 四处测试/harness 回归；这些回归均已定向取得 exit 0。第二次以 `RUST_TEST_THREADS=8` 运行时，所有进入 PTY 前的 workspace tests 均通过，但 PTY 总套件仍未在 600 秒内完成，因此不能把完整 workspace tests 标为通过。

## 4. 剩余问题与后续步骤

### P0：最终验收

- [ ] 修复 PTY 测试源码的版本控制缺口：`crates/allthecodes/tests/pty_tui_e2e/` 当前有 30 个本地测试文件，但根 `.gitignore` 的 `tests/` 规则会忽略整个目录，且 `git ls-files crates/allthecodes/tests/pty_tui_e2e` 无输出。在这些文件被正式跟踪，或明确决定保持本地专用并同步修改验收口径前，本地 247 项 PTY 结果不能作为干净 checkout 或 CI 的有效证据。
- [ ] 在不受 600 秒工具上限约束的环境运行完整 `cargo test --locked --workspace`，取得最终 exit 0。建议保留 runner 原生并发或按 CI 核心数设置；不要在高核心机器无上限并发启动全部 PTY。
- [ ] 由 GitHub Actions 的 Windows runner 执行 Windows ACL 行为测试，确认真实 DACL 只允许当前用户。
- [ ] 由 GitHub Actions 完成 Linux/macOS/Windows platform-check；本机只能交叉编译 Windows cfg，不能替代运行测试。

### P1：审计证据收尾

- [ ] 将原审计清单的 C/H/M/Q 每一项映射到实现文件、测试名和命令结果，形成最终验收矩阵；原风险清单继续保持只读。
- [ ] 对已迁移/删除的旧 Team Memory blocking client 路径完成最终等价性确认，确保不存在遗漏的无 timeout HTTP 调用。
- [ ] 在可用环境安装并运行 `osv-scanner`，将结果与 cargo-audit/cargo-deny 交叉核对。

### P2：测试时长治理

- [ ] 为 `pty_tui_e2e` 增加可靠的分片策略或 CI job matrix，避免单个 247-test binary 在高核心/资源受限机器上过度并发或超过任务超时。
- [ ] 记录 PTY 各组耗时基线；teardown timing 继续只测 teardown 阶段，不使用包含 startup 排队的 wall-clock 总时长。

## 5. 完成定义

本计划在以下条件全部满足后关闭：

1. PTY E2E 测试源码已被版本控制，且干净 checkout 能发现预期测试集合；
2. 完整 workspace test 取得 exit 0；
3. Windows ACL 与三平台 CI 取得 exit 0；
4. C/H/M/Q 验收矩阵逐项有源码与测试证据；
5. OSV、cargo-audit、cargo-deny 结果已交叉复核，所有 ignore 都是窄范围且有不可达/无修复依据。
