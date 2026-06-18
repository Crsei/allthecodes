# Workspace All-Targets Warning Budget Plan

日期：2026-05-20

来源：

- `docs/ui/great/plans/plan-08-dead-code-cleanup.md` 的实施后遗留问题
- 当前 Full Build 阶段要求：不再用 Lite 缩减作为 warning / lint 债务边界

范围：

- Rust 编译器 warning：`cargo check --workspace --all-targets --message-format short`
- Rust TUI test target warning：当前主要集中在 `crates/claude-code-rs/src/ui/**`
- 长期 `allow(dead_code|unused_imports|unused)` 债务：整个 `crates/**`
- 环境型 build-script warning：单独记录，不计入 Rust 代码 warning budget

非目标：

- 本计划不要求一次性启用全 workspace `clippy -D warnings`。Clippy 预算作为后续加严阶段处理。
- 本计划不把本机缺少 `npm` 导致的 web-ui build script 提示作为 Rust 代码 warning。
- 本计划不删除仍属于 Full Build parity 目标的 UI 模块；这些模块需要接线、测试门控或文档化归属。

## 当前基线

2026-05-20 采样命令：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
cargo check --workspace --all-targets --message-format short
```

结果：

- 命令退出码：0
- Rust warning：180 条，全部来自 `claude-code-rs` 的 test target
- 主要 warning 类别：`dead_code`、`unused`、`field is never read`、`variant is never constructed`
- 主要目录：`crates/claude-code-rs/src/ui/**`
- 环境 warning：`claude-code-rs` build script 因本机未安装 `npm` 跳过 web-ui dependency install/build
- 当前 `allow(dead_code|unused_imports|unused)` 追踪基线：`crates/**` 共 15 处，其中 `crates/claude-code-rs/src/ui/**` 共 4 处

当前 Rust warning budget：

| Gate | 当前值 | 目标值 | 是否计入代码预算 |
| --- | ---: | ---: | --- |
| `cargo check --workspace --all-targets` Rust warnings | 180 | 0 | 是 |
| `cargo build --workspace --release` Rust warnings | 0 | 0 | 是 |
| `allow(dead_code|unused_imports|unused)` in `crates/**` | 15 | 0 或显式豁免 | 是 |
| `npm` 缺失 build-script warning | 1 个环境事项 | 记录即可 | 否 |

## 需要追踪的内容

| 追踪项 | 说明 | 采集命令 | 关闭条件 |
| --- | --- | --- | --- |
| Active Rust warnings | 当前编译器实际输出的 warning | `cargo check --workspace --all-targets --message-format short` | 输出中没有 Rust source warning |
| UI test-target dead code | `claude-code-rs` test target 暴露的 UI 未接线表面 | 同上，筛选 `crates/claude-code-rs/src/ui/` | 全部接线、删除、改为 test-only，或标为 intentional |
| Suppression debt | 长期 `allow(dead_code|unused_imports|unused)` | `rg 'allow\((dead_code|unused_imports|unused)\)' crates -g '*.rs'` | 无无说明 allow；保留项有 owner、原因、移除条件 |
| Environment warnings | 机器环境导致的 build-script warning | all-targets / release build 输出 | 只允许已记录环境项，不新增代码 warning |
| New warning regression | 后续 PR 是否新增 warning | 对比本计划基线与每次 phase closeout | 新 warning 必须在同一提交内修复或列入本计划 |
| Clippy hardening backlog | clippy 还未进入主预算的规则 | `cargo clippy --workspace --all-targets` | P5 后决定是否升级为 `-D warnings` |

## Phase Plan

### Phase 0：预算口径与基线固化

目的：把 warning budget 从“人工观察”变成可重复采集的项目状态。

工作内容：

- 保存当前 all-targets warning 分类，至少按 crate、目录、lint kind、是否 test-only 四列记录。
- 把 `npm` 缺失提示归类为环境 warning，不计入 Rust source budget。
- 建立最小统计脚本或文档命令，避免每次人工从 cargo 输出里重新数 warning。
- 给每个保留的 `allow(dead_code|unused_imports|unused)` 补齐 owner、原因、移除条件；无法说明的直接进入修复队列。

退出条件：

- `cargo check --workspace --all-targets --message-format short` 的 warning 数量和分类可复现。
- 文档中明确区分 Rust source warning 与环境 warning。
- 新增代码不得增加 warning budget。

验证：

```bash
cargo check --workspace --all-targets --message-format short
rg 'allow\((dead_code|unused_imports|unused)\)' crates -g '*.rs'
```

### Phase 1：Rust TUI test-target warning 分组清理

目的：处理当前 180 条 warning 的主体来源。

分组：

| 分组 | 典型路径 | 处理策略 |
| --- | --- | --- |
| Agent surfaces | `ui/agents/**` | 接入 agent editor/menu 生产路径；未落地的编辑动作转为明确 test-only 或计划项 |
| App runtime adapters | `ui/app/**` | 区分真实 runtime glue、测试 adapter、未来 IPC adapter；真实路径接线，测试工具 `cfg(test)` |
| Components and overlays | `ui/components/**`, `ui/overlays/**` | 已使用组件补生产调用；纯 fixture/helper 收进测试模块 |
| Input helpers | `ui/input/**`, `ui/prompt_input.rs` | 将已完成 helper 接到 composer/palette；未启用平台能力加 feature/cfg |
| Rendering helpers | `ui/rendering/**`, `ui/messages/**` | 保留用户可见渲染能力，删除重复 helper，测试专用模拟器进 test module |
| Runtime/platform surfaces | `ui/runtime/**`, `ui/platform/**` | 按平台、debug、voice、session log 分 owner；平台不可用项使用条件编译 |

退出条件：

- 每个 UI warning 都有处理结果：生产接线、删除、移动到 `#[cfg(test)]`、feature/platform cfg、或写入 intentional follow-up。
- `cargo check -p claude-code-rs --all-targets --message-format short` 不再输出 UI test-target warning。

验证：

```bash
cargo check -p claude-code-rs --all-targets --message-format short
cargo test -p claude-code-rs ui:: -- --nocapture
```

### Phase 2：全 workspace suppression debt 清理

目的：清掉隐藏 warning 的长期宽限，避免 all-targets 归零后又被 allow 掩盖。

当前需要追踪的 allow 区域：

| 区域 | 当前状态 | 处理策略 |
| --- | --- | --- |
| `cc-query/src/deps.rs` | 2 处 `dead_code` | 判断是未来 dependency contract 还是未接线字段；能接线则接线，不能接线则 test-only 或文档化 intentional |
| `cc-ipc-protocol/src/subsystem_*` | 2 个文件级 `dead_code` | IPC 扩展类型若为未来 contract，改为 feature/cfg 或在计划中列 owner；避免无期限文件级 allow |
| `cc-tools/src/hooks/*` | 2 处 `unused` | 完成 hooks runtime wiring 或删除未使用 wrapper |
| `cc-engine/src/lifecycle/submit_message.rs` | 3 处 `dead_code` | 对照 lifecycle submit path，区分测试 helper 与未接线分支 |
| `claude-code-rs/src/ui/**` | 4 处 `unused_imports/unused` | 与 Phase 1 一并清理 |
| `cc-services/src/telemetry/*` | 2 处 `dead_code` | telemetry feature 接线或 feature-gated contract |

退出条件：

- `rg 'allow\((dead_code|unused_imports|unused)\)' crates -g '*.rs'` 没有无说明结果。
- 若确需保留，必须有局部注释说明 owner、原因、计划移除条件，不允许宽泛文件级 allow。
- 保留项不得屏蔽当前 all-targets warning gate。

验证：

```bash
rg 'allow\((dead_code|unused_imports|unused)\)' crates -g '*.rs'
cargo check --workspace --all-targets --message-format short
```

### Phase 3：全 workspace Rust warning 归零

目的：把 all-targets 从“可编译但有 warning”升级为 Rust source warning 预算为零。

工作内容：

- 跑全 workspace all-targets，确认非 UI crate 是否还有新 warning。
- 每次只处理一个 owner area，避免跨 crate 大范围清理造成回归。
- 对每个 warning 优先选择真实使用或删除；其次使用 `cfg(test)` / feature gate；最后才允许 intentional 记录。
- 更新相关功能计划的遗留问题，避免 warning budget 文档成为唯一追踪入口。

退出条件：

- `cargo check --workspace --all-targets --message-format short` 没有 Rust source warning。
- `cargo build --workspace --release` 仍保持无 Rust warning。

验证：

```bash
cargo check --workspace --all-targets --message-format short
cargo build --workspace --release
```

### Phase 4：Warning gate 自动化

目的：防止 warning budget 清零后反复回涨。

工作内容：

- 增加本地/CI 可执行 gate：`cargo check --workspace --all-targets` 的 Rust source warning 必须为 0。
- 对环境 warning 采用 allowlist，并要求输出中不出现新的 Rust source warning。
- 评估是否用 `RUSTFLAGS="-D warnings"` 做最终 gate；如果 build-script 环境 warning 影响过大，先使用输出解析 gate。
- 在提交模板或开发文档中写明：新增 warning 与新增无说明 allow 均不能单独合入。

退出条件：

- gate 能稳定区分 Rust source warning 和已知环境 warning。
- 新增 warning 会导致验证失败。
- `docs/WORK_STATUS.md` 或对应迁移计划记录 warning gate 已启用。

验证：

```bash
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets
```

如果 `RUSTFLAGS=-D warnings` 因环境型 build script 输出不稳定，保留解析式 gate，并在文档中记录原因。

### Phase 5：Clippy budget 后续加严

目的：在 rustc warning 归零后，再决定 clippy 是否进入主线预算。

工作内容：

- 采样 `cargo clippy --workspace --all-targets` 当前输出。
- 按 owner area 分类 clippy warning，不与 rustc warning 清理混在同一提交。
- 先清理 correctness/readability 高信号规则，再评估是否启用 `-- -D warnings`。
- 对确属风格差异或 Full Build parity 需要的 lint，局部说明原因。

退出条件：

- 有独立 clippy warning budget 文档或本计划 P5 附录。
- `cargo clippy --workspace --all-targets -- -D warnings` 要么通过，要么失败项全部有 owner 与关闭路径。

验证：

```bash
cargo clippy --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

## 执行顺序

1. Phase 0 先落地，冻结基线和口径。
2. Phase 1 与 Phase 2 可以并行，但同一文件只允许一个 owner 修改。
3. Phase 3 在 Phase 1/2 主要清理完成后执行。
4. Phase 4 必须在 all-targets Rust warning 归零后再启用。
5. Phase 5 不阻塞 rustc warning budget 清零，但不能掩盖新的 rustc warning。

## 风险与注意

- Rust TUI 仍保留大量上游 parity 表面，不能因为 test target warning 就机械删除。
- 文件级 `#![allow(unused)]` 容易掩盖新问题，清理时应优先缩小到具体 item 或改为条件编译。
- `#[cfg(test)]` 只能用于真实测试辅助代码；生产未来表面应进入对应功能计划，而不是无限期测试门控。
- `RUSTFLAGS="-D warnings"` 可能把 build script 或依赖输出放大成阻塞项，启用前必须先验证本机和 CI 的一致性。
- 每个 phase 完成后需要单独提交文档或代码变更，提交信息直接说明本次清理目标。
