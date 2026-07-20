# Rust 编译性能治理计划

> 日期：2026-07-20
> 状态：Proposed
> 范围：本地增量开发、隔离 worktree、CI、最终 release 验证
> 总入口：[`codebase-optimization-plan-2026-07-03.md`](codebase-optimization-plan-2026-07-03.md)
> 现状证据：[`current-state-audit-2026-07-20.md`](current-state-audit-2026-07-20.md)

## 结论

编译慢不是一个单独的 Cargo 参数问题。当前瓶颈由四类因素叠加：

1. 根 binary 无条件连接几乎所有运行模式，`--no-default-features` 也没有形成轻量 TUI 构建。
2. `allthecodes-web` 是一个 46,888 行的大编译单元，本轮冷 release 中单元耗时 185.3 秒。
3. vendored OpenSSL、bundled SQLite 等 native build 在全新 target 中成本高，但它们有跨平台/release 约束，不能直接删除。
4. 高扇出基础 crate 和长串行依赖链使 64 核机器平均只利用约 8 核；简单增加 `jobs` 不能消除关键路径。

执行顺序必须是“可复现基准与缓存治理 → 真正的运行模式 feature 隔离 → Web 编译边界 → 高扇出 contracts → CI 去重”。仅把大文件拆成同 crate 下多个文件可以改善可维护性，但不会把 185.3 秒的 Web rustc 单元变成可并行单元。

## 2026-07-20 基线

### 测量边界

本轮在主分支工作目录上使用全新隔离 target：

```text
CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-code-split-audit.Zfs3Xu
cargo build -p allthecodes --release --locked --timings
```

Cargo timing HTML 的原始本机路径是：

```text
/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-code-split-audit.Zfs3Xu/cargo-timings/cargo-timing.html
```

该路径是本机临时证据，不作为仓库长期接口。仓库内的稳定基线由本计划和当前状态审计报告保存。

| 指标 | 结果 | 来源 |
|---|---:|---|
| 冷 release wall time | 401.7 s | Cargo timings |
| 编译单元 | 517 dirty / 517 total | Cargo timings |
| Cargo 最大并发 | 64 | Cargo timings |
| warm no-op | 1.29 s | 同轮终端计时 |
| 平均 CPU | 794% | 同轮 `/usr/bin/time -v` 输出 |
| 峰值 RSS | 3.22 GiB | 同轮 `/usr/bin/time -v` 输出 |
| 本机 CPU / RAM | 64 logical CPU / 约 1 TiB | 本机只读检查 |

平均 CPU 约 794% 且内存余量很大，说明主要问题是 native build、依赖关键链和大 crate 内部串行 frontend/codegen，而不是内存不足。

### 最慢编译单元

| 排名 | 单元 | 总耗时 | frontend | codegen |
|---:|---|---:|---:|---:|
| 1 | `allthecodes-web` | 185.3 s | 90.9 s | 94.4 s |
| 2 | `openssl-sys` build script | 73.7 s | native | native |
| 3 | `libsqlite3-sys` build script | 64.9 s | native | native |
| 4 | `allthecodes` binary | 39.0 s | - | - |
| 5 | `allthecodes-commands` | 32.8 s | 9.3 s | 23.5 s |
| 6 | `agent-client-protocol-schema` | 30.6 s | 19.7 s | 11.0 s |
| 7 | `allthecodes-services` | 30.1 s | 7.0 s | 23.1 s |
| 8 | `allthecodes-engine` | 27.7 s | 14.9 s | 12.8 s |
| 9 | `allthecodes-tools` | 26.3 s | 14.7 s | 11.6 s |
| 10 | `allthecodes-protocol` | 26.3 s | 23.1 s | 3.2 s |
| 11 | `allthecodes-config` | 26.0 s | 4.1 s | 21.9 s |
| 12 | `allthecodes-mcp` | 24.9 s | 4.9 s | 20.0 s |
| 13 | `allthecodes-session` | 20.9 s | 8.4 s | 12.5 s |

单元时间会重叠，不能把表中各项直接相加当作 wall time。

### 依赖与磁盘基线

| 指标 | 当前值 |
|---|---:|
| workspace member | 42 |
| 根包 normal direct dependencies | 102（38 internal + 64 external） |
| Linux default normal+build 闭包 | 410 packages |
| Linux `--no-default-features` normal+build 闭包 | 391 packages |
| Linux `--all-features` normal+build 闭包 | 424 packages |
| 共享 `.tmp/allthecodes-target` | 187 GiB |
| 仓库内旧 `target/` | 76 GiB |
| 共享 target 的 incremental | 约 110 GiB、约 573,000 files |
| 文件系统 | `/dev/sda1`, ext4, rotational disk |

当前根包默认 feature 只有 `image` 和 `syntect`，但 Web、SQLx、daemon、ACP 等内部依赖仍是无条件依赖。因此 `--no-default-features` 只减少 19 个包，不能代表最小 TUI 构建。

当前主要串行链可概括为：

```text
types ─┬─> config/session ─> tools ─> mcp/engine ─> commands/services ─> web ─> root
       └───────────────────────────────────────────────────────────────> root
```

越靠左的高扇出类型/配置改动，越容易让后续大 crate 全部失效；越靠右的 Web/root 大单元，越容易成为最后一段串行尾巴。

## 根因判断

### 1. 运行模式没有进入 Cargo feature 图

`crates/allthecodes/Cargo.toml` 当前把 38 个内部 crate 作为 normal dependency 无条件引入。`allthecodes-tools` 又默认开启 `full`，把 browser、permissions、sandbox、session、skills、tasks 等实现依赖带入闭包。CLI 的 TUI、Web backend、daemon、ACP 和 storage 模式在运行时分流，但在编译期仍然一起构建。

### 2. Web 同时是维护热点和编译热点

`crates/allthecodes-web/src` 有 46,888 行 Rust，其中 handlers 目录 36,426 行；`group_chat.rs` 2,695 行，`sessions.rs` 1,874 行，`ws/terminal.rs` 1,864 行，`api_dispatcher.rs` 1,802 行。该 crate 同时依赖大量内部服务和 Web/native 依赖，当前 185.3 秒的 frontend/codegen 几乎各占一半。

结论：Web 的领域拆分和编译单元拆分可以共用同一套边界，但必须先稳定 contracts，避免产生多个互相依赖的 Web crate。

### 3. Native 依赖贵，但不能破坏 release 约束

- workspace 的 `git2` 使用 `vendored-openssl`。
- `reqwest` 使用 `native-tls-vendored`。
- SQLx `sqlite` 解析到 bundled `libsqlite3-sys`。

这些设置支撑 macOS/Windows/Linux release 的可重复构建。不得为了本地基准直接去掉 vendored OpenSSL；正确路径是模式隔离、跨 target compiler cache 和 CI artifact 复用。

### 4. 本地缓存体积已反过来拖慢迭代

开发 profile 使用 `debug = "line-tables-only"` 和 `split-debuginfo = "packed"`。共享 target 中约 8.57 GiB `.dwp`，incremental 目录约 110 GiB。大量小文件位于机械盘上，metadata scan、写入和清理都会变慢。

### 5. CI 对相同闭包重复编译

主 CI job 连续运行 workspace/all-features clippy、多种 test/check/codegen/storage feature 组合，会产生 feature 变体重编译。PTY 又拆成四个独立 job，每个 shard 使用不同 cache key 并各自编译 root PTY binary。测试分片隔离是合理的，但“编译四次”不是必须条件。

### 6. 不是首要原因的项目

- 仓库两个 `build.rs` 只在 Windows 输出 `advapi32` 链接指令，不是当前 Linux 慢点。
- 本机没有安装 `mold`、`ld.lld`、`clang` 或 `sccache`；仓库 Linux linker 配置目前只是注释。不能直接取消注释后声称提速。
- 64 并发上限已经足够高；关键链存在时继续提高 `jobs` 不会让单个 Web rustc 并行。
- 仅移动函数到同一 crate 的其他 `.rs` 文件不会降低单元编译时间。

## 执行计划

### CP-001：固化可比较的编译基准

**目标：** 每次性能改动都能回答“冷构建、warm no-op 和一次典型局部修改分别快了多少”。

实施：

1. 新增仓库脚本，输出 JSON/Markdown，而不是只保留 Cargo HTML。
2. 每个样本使用全新、明确命名的隔离 target；每类运行三次并记录 median。
3. 固定三类场景：
   - cold `cargo build -p allthecodes --release --locked --timings`
   - warm no-op
   - touch/restore 一个 Web handler 后的 incremental build
4. 同时记录 wall time、CPU、RSS、dirty units、最慢 15 单元、target 增量和 binary size。
5. 记录 commit、dirty paths、rustc/Cargo 版本、CPU、存储设备和 feature 集合。

约束：Per-Session Worktree 内只编辑和提交脚本，任何 Rust build/timing 必须 fast-forward 合并后在主分支运行；失败则回原 worktree 修复。

验收：

- 同一结果文件能区分 Cargo timings 数据与 `/usr/bin/time -v` 数据。
- 第二个全新 target 可重复执行，不把同 target no-op 当作 compiler cache 命中。
- no-op 保持不高于 2 秒。

### CP-002：隔离 target 的跨树缓存与生命周期

**目标：** 保留每 worktree target 隔离，减少重复编译，并阻止缓存无限增长。

实施：

1. 安装前先记录无 `sccache` 基线；安装后设置 `RUSTC_WRAPPER=sccache`，用两个不同的 isolated target 做 A/B。
2. 分别报告 Rust cache hit 与 C/C++ compiler cache；只有验证后才考虑 `CC="sccache cc"` / `CXX="sccache c++"`。
3. worktree 继续使用 `.tmp/atc-<slug>`，禁止回到跨 worktree 共用同一 Cargo target。
4. worktree 已 push 且已删除后，先对精确 target 路径执行 `cargo clean --dry-run --target-dir <exact-path>` 审阅，再清理该路径。
5. 为长期共享 target 增加按大小、年龄和 incremental roots 的报告；80–100 GiB 或 1,000 roots 只作为告警起点，不自动 blanket clean。
6. A/B 测试 `[profile.test] debug = 0`、`split-debuginfo = "off"`，以及 dev 的 `packed` → `off`/`unpacked`；保留能够满足调试需求的最小配置。

验收：

- 第二个 isolated target 的 Rust `sccache` hit rate 达到 80% 以上，C/C++ 命中率单独报告。
- 不出现跨 worktree phantom metadata/link 错误。
- target 增长、清理候选和实际清理路径均可审计。

### CP-003：让 TUI/Web/daemon/ACP 成为真实编译模式

**目标：** 保持默认发布行为不变，同时让日常 TUI 修改不再编译 Web、SQLite、daemon 和 ACP。

建议 feature：

- `full`：默认，保持当前发布行为。
- `tui`：Rust TUI 与核心 query/tool runtime。
- `web`：Web/API/WS backend。
- `daemon`：daemon/runtime controller。
- `acp`：ACP bridge。
- `sqlite-storage` / `json-storage`：显式 storage forwarding。

实施：

1. 把只属于特定运行模式的内部依赖改为 `optional = true`，并显式 `default-features = false`。
2. 所有 mode feature 通过 `dep:` 和子 crate feature forwarding 组成，禁止在代码中仅用空 feature 名掩盖无条件依赖。
3. `allthecodes-tools` 的 contract/implementation 边界必须同时处理；根包不能无意中重新启用其默认 `full`。
4. 在 CI 增加 `cargo tree` 断言，防止 minimal TUI 闭包回涨。

验收：

- 默认 `full` 的 CLI、npm backend 行为和 release feature 保持不变。
- `--no-default-features --features tui` 的 normal+build 闭包不包含 `allthecodes-web`、`axum`、`sqlx`、`libsqlite3-sys`、`allthecodes-daemon`、`allthecodes-acp`。
- minimal TUI 闭包先以不高于 300 packages 为目标；首次实现后用实测重新校准阈值。
- TUI 局部修改的 cold/incremental 数据单独记录，不以 full release 数字掩盖。

### CP-004：按可并行领域拆 `allthecodes-web`

**目标：** 同时降低维护复杂度和最大 rustc 单元时间。

建议边界：

1. `web-core`：稳定的 auth/error/state/serialization contracts。
2. 2–4 个互不依赖的 domain handler crate：group chat、files/sessions、backend services 等按实际依赖图确定。
3. `web-runtime`：WS/IPC/terminal transport 和 runtime lifecycle。
4. 薄 `allthecodes-web` composition crate：只组合 routers 与共享 state。

边界规则：

- domain crate 返回稳定、尽量 type-erased 的 router/service 接口，不互相依赖。
- path/capability/auth policy 留在共享权威层，不能在 files/group-chat handler 各复制一套。
- group chat 按 room/invite/message store/delegation runtime/SSE projection 拆服务，而不是只按 HTTP 方法拆文件。
- terminal 按 transport/session/process/io/resize/cleanup 状态机拆。

验收：

- 最大单个 Web Rust 单元不高于 90 秒。
- full cold release median 不高于 300 秒，且比 401.7 秒基线至少降低 25%。
- 修改一个 domain handler 不触发其他独立 domain crate 重编。
- HTTP/SSE/WS 行为、鉴权、路径安全和 shutdown/cleanup 测试保持通过。

### CP-005：降低高扇出 contract 的传播范围

**目标：** 避免改一个基础类型让整条 `types → web → root` 链失效。

候选顺序：

1. 从 `allthecodes-tools` 物理抽出稳定 `tool-contracts`；engine/MCP/LSP/commands 只依赖 contracts，不依赖默认 `full` 实现。
2. 在 engine 内先形成 ports/events/contracts；query loop 仍保持唯一实现，不重建平行 `allthecodes-query`。
3. 把 config 的纯 schema/types 与 I/O/runtime resolution 分层。
4. 只在 churn + timings 证明收益后拆 `allthecodes-types` 的高变领域；禁止按类型数量机械建 crate。
5. commands 先抽 metadata/dispatch contracts，再按领域拆实现。
6. protocol 的 runtime wire types 与仅 Web/codegen 消费的 catalog 分层，但只保留一个生成真相源。

验收：

- 每次边界调整都给出修改前后 reverse-dependency 数和 incremental timing。
- 新增 crate 不形成环，也不依赖比自身更高层的 composition/runtime crate。
- 架构阶段 full cold release median 不高于 280 秒（相对基线至少降低 30%）。

### CP-006：CI feature 与 PTY 编译去重

**目标：** 相同 OS/feature 组合只做一次重编译，测试分片只分执行负载。

实施：

1. 统计主 CI job 中每种 feature 组合实际产生的重复 unit；合并能共享的 clippy/check/test 组合。
2. PTY job 先构建一次，再由四个 shard 消费可重定位 nextest archive 或 build artifact。
3. 如果 PTY harness 不支持安全重定位，则改为单 job 中 `nextest` 最多 10 个 test process 并行；不要让 libtest 在同一 runner PID 下盲目 10 路并发。
4. shard-specific cache key 只保留确实不同的测试数据；依赖/build cache 使用统一 key/restore key。

验收：

- 同一 Linux/feature 的 PTY compile 从四次降为一次。
- shard coverage、隔离目录、失败日志上传和最多 10 并发的既有语义保持不变。
- CI 总 wall time、compute minutes 和 cache size 都进入对比报告。

### CP-007：小收益依赖与 profile 实验

这些任务必须排在模式隔离和 Web 边界之后：

- 评估把直接 `tokio-tungstenite` 0.26 对齐到 Axum 使用的 0.29。
- 评估 `rand` 0.8/0.9 等重复版本；仅在 API 迁移成本低时处理。
- 新增仅用于中间 smoke 的 `release-fast`（例如 `opt-level = 2`、更多 codegen units），不得替代正式 release gate。
- 可对 `allthecodes-web` 单独 A/B 更多 `codegen-units`，同时比较 binary size、启动时间和 Web throughput。
- 安装并验证 `mold`/`lld` 后才允许改 Linux linker；仓库配置不能指向本机不存在的工具。

## 状态跟踪

| ID | 优先级 | 状态 | 退出条件 |
|---|---:|---|---|
| CP-001 | P0 | proposed | 三类可重复 timing + 结构化报告 |
| CP-002 | P0 | proposed | isolated target cache 与可审计生命周期 |
| CP-003 | P0 | proposed | minimal TUI 闭包不含 Web/SQLite/daemon/ACP |
| CP-004 | P0 | proposed | Web 最大单元 ≤ 90 s，full cold ≤ 300 s |
| CP-005 | P1 | proposed | 高扇出 contract 收窄，full cold ≤ 280 s |
| CP-006 | P1 | proposed | PTY 相同闭包 compile 4 → 1 |
| CP-007 | P2 | proposed | 重复依赖/profile/linker 有 A/B 证据 |

## 禁止事项

1. 不以删除 vendored OpenSSL 或 bundled SQLite 破坏跨平台 release 可重复性。
2. 不让多个并行 worktree 共用一个 Cargo target；compiler cache 可以共享，target metadata 不共享。
3. 不把 crate 数量减少或增加本身当作目标；目标是缩短关键链、降低变更传播和并行独立单元。
4. 不用同 target no-op 时间冒充 cold build 或 compiler cache 成果。
5. 不在未安装并验证 linker 的机器上直接启用仓库级 linker 配置。
6. 不用 `release-fast` 替代最终 `cargo build --workspace --release`。
7. 不在任务 worktree 内运行 Rust build/test/timing；遵守主分支验证与失败回原 worktree修复的仓库流程。

## 完成定义

- 基准脚本、结果格式和 cleanup 边界可重复、可审计。
- 默认 full 功能、各 storage 模式和跨平台 check 不因性能优化被缩减。
- 第一阶段 cold median ≤ 300 秒；架构阶段 ≤ 280 秒；no-op ≤ 2 秒。
- minimal TUI 闭包和 CI PTY 编译次数达到各自退出条件。
- 每个优化 PR 同时报告 wall、CPU、RSS、dirty units、top units、target 增量和 binary size。
- 所有最终 Rust 验证只在 fast-forward 合并后的主分支执行，且按仓库分层验证 SOP 进行。
