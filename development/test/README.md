# TDD 测试计划索引

> 根据最近 git commit 新增的计划文档生成，放在 `development/test/` 目录下。

| 文件 | 来源计划 | 实现状态 | 优先级 |
|------|----------|----------|--------|
| [code-split-tdd-test-plan.md](code-split-tdd-test-plan.md) | `development/code-split/codebase-optimization-plan-2026-07-03.md` | P0-P2 待实现 | ⭐ P0 |
| [cargo-build-test-system-plan.md](cargo-build-test-system-plan.md) | Codex 原版 build/test 体系对照 | 本计划 | ⭐ P0 |
| [record-replay-tdd-test-plan.md](record-replay-tdd-test-plan.md) | `development/record-replay/implementation-plan.md` | Phase 0-4 核心待实现 | ⭐ P0 |
| [command-risk-tdd-test-plan.md](command-risk-tdd-test-plan.md) | `development/command/unified-command-risk-classification-plan.md` | Phase 1 待实现 | ⭐ P0 |
| [cost-tdd-test-plan.md](cost-tdd-test-plan.md) | `development/cost/session-cost-log-plan.md` | Phase 1-2 待实现 | ⭐ P0 |
| [dynamic-workflow-tdd-test-plan.md](dynamic-workflow-tdd-test-plan.md) | `development/dynamic_workflow/01-arch-plan.zh.md` | Phase 1 待实现 | ✅ 已规划 |
| [mcp-scope-isolation-tdd-test-plan.md](mcp-scope-isolation-tdd-test-plan.md) | `development/mcp/mcp-plugin-scope-isolation-plan.md` | **已实现**（回归） | ✅ 回归 |
| [record-replay-engine-tdd-test-plan.md](record-replay-engine-tdd-test-plan.md) | `development/runtime/agent-runtime-execution-record-fields-plan.md` | **已实现**（回归） | ✅ 回归 |
| [worktree-session-tdd-test-plan.md](worktree-session-tdd-test-plan.md) | `development/worktree/worktree-aware-session-plan.md` | **已实现**（回归） | ✅ 回归 |
| [tui-command-operation-tdd-test-plan.md](tui-command-operation-tdd-test-plan.md) | `development/tui/command-operation-display-plan.md` | **已实现**（回归） | ✅ 回归 |
| [runtime-current-state-tdd-reference.md](runtime-current-state-tdd-reference.md) | `development/runtime/` 4 份 RFC 状态文档 | 已知边界参考 | 📖 参考 |
| [pty-tui-e2e-timing-plan.md](pty-tui-e2e-timing-plan.md) | `development/test/pty-tui-e2e-timing-plan.md`（本目录内） | **Phase 1 已实现**（信号化等待替换） | ✅ 部分 |

## 测试层级定义

| 层级 | 位置 | 说明 |
|------|------|------|
| L1 Unit | crate 内的 `#[cfg(test)]` | 函数/类型级，快，无外部依赖 |
| L2 Integration | `crates/*/tests/`、`tests/pty_tui_e2e/` | crate 边界行为，验证多模块协作 |
| L3 E2E | `tests/pty_tui_e2e/` 完整 CLI 路径 | 二进制级别验证，带 PTY |
| L4 Golden | `tests/fixtures/` | 黄金文件 + snapshot 验证 |

## 引用格式

每个测试计划文件中的代码块为 Rust 伪代码（`//` 注释 + 函数签名 + 关键断言），用于指导实际测试代码编写。实际编写时需适配：
1. 具体 mock/fixture 工具
2. crate 的 `#[cfg(test)]` 模块结构
3. 现有的 test harness（PTY TUI E2E 使用 `script.rs` + `PtySession`）

## 状态标记说明

| 标记 | 含义 |
|------|------|
| ⭐ P0 | **待实现** — 当前阶段核心目标 |
| ✅ 已规划 | 待实现但非当前紧急 |
| ✅ 回归 | 已落地功能，需防止退化 |
| 📖 参考 | 已知边界记录，非严格测试计划 |

## 运行

```bash
# Cargo build/test wrapper
scripts/cargo-build-test.sh --dry-run full
scripts/cargo-build-test.sh ci
scripts/cargo-build-test.sh full

# Optional just facade
just dry-run full
just ci
just full

# 单个计划
cargo test -p allthecodes-permissions command_risk
cargo test -p allthecodes-session
cargo test -p allthecodes-engine -- record_replay
cargo test -p allthecodes-web worktree

# 全量
cargo build --workspace --release
```

## Proactive

Focused verification:

```bash
cargo test -p allthecodes-services proactive
cargo test -p allthecodes-commands proactive
cargo test -p allthecodes-daemon proactive
cargo test -p allthecodes proactive
```
