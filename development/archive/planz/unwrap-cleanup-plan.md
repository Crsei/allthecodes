# Plan: unwrap/expect/panic 清零计划

> 基于 `development/debug/unwarp-solution.md` 的理论框架 + 实际代码扫描数据
> 本文档定义哪些地方需要处理、如何处理、执行顺序和验收标准。

---

## Context

**问题**: `shishan-assessment.md` 报告全项目有 5,038 个 unwrap/expect/panic 调用，这是全项目最大的可靠性风险。但进一步分析显示**其中 ~97% 在测试代码中**——实际生产代码中非测试的调用只有 **183 处，分布在 61 个文件**。

**目标**: 将生产代码中的 unwrap/expect/panic 消除到合理范围，并通过 clippy 配置防止回潮。**不需要也绝不应该试图修改测试代码中的 unwrap**——测试中 `.unwrap()` 是 Rust 标准实践，失败即测试失败，是预期的行为。

**遵循原则**（来自 `unwarp-solution.md`）:
- IO、网络、解析、配置读取 → 返回 `Result` + `?` + `context()`
- `Option::unwrap()` → `ok_or_else(|| anyhow!(...))?`
- 有默认值的 → `unwrap_or` / `unwrap_or_else` / `unwrap_or_default`
- 内部不变量 → `expect("解释为什么不可能失败")`
- 测试代码 → 保持原样，不修改

---

## 实际扫描结果

| 模式 | 非测试调用数 | 涉及文件数 |
|------|------------|-----------|
| `.unwrap()` | 80 | ~35 |
| `.expect("...")` | 93 | ~20 |
| `panic!()` | 2 | 2 |
| `unreachable!()` | 8 | ~5 |
| `todo!()` | 0 | 0 |
| **合计** | **183** | **61** |

---

## 调用分类与处理策略

### A 类: 安全的，可以不处理（但要加 expect 消息）— ~71 处

#### A1: Mutex/RwLock lock().unwrap() — ~42 处
这些调用发生在锁中毒时 panic，而 Rust 标准库中锁中毒意味着持有锁的线程已经 panic，进程已经处在未定义状态。这是 Rust 标准实践接受的权衡。

**但 17 处使用了裸 `.unwrap()` 而非 `.expect("lock poisoned")`**，应该加上消息以便调试。

**涉及文件**:
| 文件 | 调用数 |
|------|--------|
| `crates/allthecodes-tools/src/hooks/async_registry.rs` | ~11 |
| `crates/allthecodes-tools/src/hooks/hook_events.rs` | ~6 |
| `crates/allthecodes-engine/src/hooks/file_watcher.rs` | ~6 |
| `crates/allthecodes-engine/src/hooks/hook_helpers.rs` | ~3 |
| `crates/allthecodes-engine/src/hooks/http_hook.rs` | ~2 |
| `crates/allthecodes-engine/src/multi_agent_v2.rs` | ~2 |
| `crates/allthecodes/src/plan_workflow.rs` | ~6 |
| 其他小而散的 | ~6 |

**处理方式**: 每个文件批量替换 `.unwrap()` → `.expect("Mutex/RwLock poisoned")`

#### A2: LazyLock<Regex> 硬编码正则编译 — ~29 处
这些是 `Regex::new("^[a-z_]+$").expect("hardcoded regex must be valid")` 模式，硬编码的正则文字不可能编译失败。

**涉及文件**:
| 文件 | 调用数 |
|------|--------|
| `crates/allthecodes-utils/src/git_operation_tracking.rs` | ~21 |
| `crates/allthecodes-utils/src/bash.rs` | ~8 |
| `crates/allthecodes-shell-command/src/fallback.rs` | ~5 |

**处理方式**: **保持不动**。这些已经是 `.expect("...")` 且有有用消息——它们符合 Rust 官方对内部不变量的建议。

---

### B 类: 需要处理（中等优先级）— ~70 处

#### B1: HashMap/collection 访问 unwrap — ~25 处
**文件**: `crates/allthecodes-lsp-service/src/mod.rs`（13 处）、`crates/allthecodes-api/src/api/client/types.rs`（~6 处）、其他零散

模式是 `clients.get_mut(&key).unwrap()` —— 代码逻辑上 key 已经被验证存在，但这不是编译器保证的。如果哪天 bug 引入不一致，调用路径就直接 panic。

**处理方式**:
```rust
// Before
let client = clients.get_mut(&lang).unwrap();

// After
let client = clients.get_mut(&lang)
    .ok_or_else(|| anyhow!("LSP client not initialized for language: {lang}"))?;
```

#### B2: 环境变量/路径/IO 调用 — ~12 处
**文件**: `crates/allthecodes-lsp-service/src/mod.rs`（`std::env::current_dir().unwrap()` × 2）、`crates/allthecodes-web/`（~4 处）、其他

这些是 IO 操作，在任何系统环境下都可能失败（权限、路径不存在等），不应该 panic。

**处理方式**:
```rust
// Before
let cwd = std::env::current_dir().unwrap();

// After
let cwd = std::env::current_dir().context("failed to get current working directory")?;
```

#### B3: 序列化/解析 unwrap — ~12 处
**文件**: `crates/allthecodes-lsp-service/src/mod.rs`（`serde_json::to_value(...).unwrap()`）、`crates/allthecodes-api/src/api/bedrock.rs`（7 处 `try_into().expect("slice length")`）、其他

`bedrock.rs` 的情况**最严重**——二进制协议解析，如果服务端发来格式错误的帧，程序会 panic 而不是优雅报错。

**处理方式**:
```rust
// Before
let field: [u8; 4] = buf[0..4].try_into().expect("buffer must have 4 bytes");

// After
if buf.len() < 4 {
    bail!("bedrock protocol error: frame too short (expected >= 4, got {})", buf.len());
}
let field: [u8; 4] = buf[0..4].try_into().unwrap(); // 现在安全了，因为已检查
```
或者直接用 `anyhow::Context`：
```rust
let field: [u8; 4] = buf[0..4].try_into()
    .context("bedrock protocol error: failed to parse frame header")?;
```

#### B4: shell 解析 unwrap — ~8 处
**文件**: `crates/allthecodes-shell-command/src/fallback.rs`

模式是 `pending.front().unwrap()`（假设 list 非空）、`.as_ref().unwrap()`（假设 segment 存在）。在极端边缘情况下可能 panic。

**处理方式**: 改用 `if let Some()` 或 `ok_or_else`。需要检查调用上下文判断是否真的应该返回 `Result` 还是保持 `unreachable!()`。

#### B5: tokio::sync 锁/Mutex 变体 — ~8 处
类似 A1 但使用 tokio 异步锁。消息统一即可。

---

### C 类: 低优先级（可暂不处理）— ~30 处

#### C1: unsafe 块内的 unwrap — 少量
在 `unsafe` 块内的 `.unwrap()` —— 通常调用者已经手动验证了前置条件。

**处理方式**: 如果确认安全，转换为 `expect("解释为什么安全")`。

#### C2: 配置加载/启动路径 — ~8 处
**文件**: `crates/allthecodes/src/full_init.rs`、`crates/allthecodes/src/swift_loader.rs` 等

这些在进程启动路径上的 unwrap——如果失败程序也没法继续运行。**但改为 `?` 可以给出更好的错误消息**。

**处理方式**: 转换为 `Result` + `context()`，让启动失败时输出"配置加载失败: /path/to/file: No such file"，而不是赤裸的 panic 信息和堆栈。

#### C3: 小而散的文件 — ~15 处
分布在 `allthecodes-computer-use`（~4）、`allthecodes-services`（~3）、`allthecodes-safety`（~3）、`allthecodes-plugins`（~3）等。

**处理方式**: 逐文件审查，按上述规则统一处理。

---

## 执行计划

### 第一阶段: 基础设施（预计 1 天）

#### Step 1.1: 添加 clippy.toml
```toml
# clippy.toml — 放在项目根目录
allow-expect-in-tests = true
allow-unwrap-in-tests = true
allow-panic-in-tests = true
disallowed-methods = []
```

#### Step 1.2: 添加 workspace lint 配置到根 Cargo.toml
```toml
[workspace.lints]
clippy.unwrap_used = "warn"
clippy.expect_used = "warn"
clippy.panic = "warn"
clippy.panic_in_result_fn = "warn"
```

先在 `[workspace.lints]` 设置为 `"warn"` 而非 `"deny"`，允许在重构期间渐进式修复。

#### Step 1.3: 在 crate 级别屏蔽 test 代码中的 lint
在 `clippy.toml` 中我们已经允许了测试代码，但 integration tests（`tests/` 目录）和 examples 可能需要额外处理，参考 `unwarp-solution.md` 中提到的 clippy issue #13981。

### 第二阶段: A 类 — 锁 unwrap 加消息（预计 1 天，零逻辑风险）

无逻辑变更，纯字符串替换。可以批量完成：

1. `crates/allthecodes-tools/src/hooks/async_registry.rs` — 替换 `lock().unwrap()` 为 `lock().expect("PENDING_HOOKS lock poisoned")`
2. `crates/allthecodes-tools/src/hooks/hook_events.rs` — 同上
3. `crates/allthecodes-engine/src/hooks/file_watcher.rs` — 同上
4. `crates/allthecodes-engine/src/hooks/hook_helpers.rs` — 同上
5. `crates/allthecodes-engine/src/hooks/http_hook.rs` — 同上
6. `crates/allthecodes-engine/src/multi_agent_v2.rs` — 同上
7. `crates/allthecodes/src/plan_workflow.rs` — 同上
8. `crates/allthecodes/src/full_init.rs` — 同上
9. `crates/allthecodes/src/swift_loader.rs` — 同上

### 第三阶段: B 类 — 核心逻辑修复（预计 3-4 天，每个 crate 独立工作）

**高优先级**（影响可靠性和数据完整性）:

| 文件 | 变更 | 重要性 |
|------|------|--------|
| `crates/allthecodes-api/src/api/bedrock.rs` | 二进制协议解析检查 buffer 长度 | 🔴 最高——解析恶意/畸形数据可能 crash |
| `crates/allthecodes-lsp-service/src/mod.rs` | HashMap 访问 + env::current_dir 换 `?` | 🟠 高 |
| `crates/allthecodes-shell-command/src/fallback.rs` | shell 解析路径加安全检查 | 🟠 高 |
| `crates/allthecodes-api/src/api/client/types.rs` | 静态 HashMap 访问加更详细的 expect 消息 | 🟡 中 |

**中等优先级**:
| 文件 | 变更 | 重要性 |
|------|------|--------|
| `crates/allthecodes-teams/src/` (~10 处) | 逐文件审查 | 🟡 中 |
| `crates/allthecodes-web/src/` (~4 处) | IO/请求路径 | 🟡 中 |
| `crates/allthecodes-computer-use/src/` (~4 处) | 输入验证 | 🟡 中 |
| `crates/allthecodes-safety/src/` (~3 处) | 安全检查路径 | 🟡 中 |
| `crates/allthecodes-services/src/` (~3 处) | 服务调用 | 🟡 中 |
| `crates/allthecodes-plugins/src/` (~3 处) | 插件接口 | 🟡 中 |

### 第四阶段: CI 集成（预计 0.5 天）

#### Step 4.1: 添加 clippy 到 CI
在 `.github/workflows/release.yml` 中添加（或者新建 `ci.yml`）：

```yaml
name: CI
on: [push, pull_request]
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: 1.91.1
          components: clippy
      - run: cargo clippy --all-targets --all-features -- -D warnings
```

如果需要更渐进的方式，第一阶段先 `cargo clippy --all-targets --all-features`（不 deny），在 CI 中设置一个步骤检查 clippy 输出但不阻断 CI，仅预警。

#### Step 4.2: 将 clippy warn 提升为 deny
当第二阶段/第三阶段的修复完成后，将 workspace lints 从 `"warn"` 提升为 `"deny"`：
```toml
[workspace.lints]
clippy.unwrap_used = "deny"
clippy.expect_used = "deny"
clippy.panic = "deny"
clippy.panic_in_result_fn = "deny"
```
**注意**: 必须先确保 `clippy.toml` 中有 `allow-*-in-tests = true`，否则 CI 会因为测试代码中的 unwrap 而失败。

### 第五阶段: 可选增强（预计 1-2 天）

- 为 public API 函数添加 `# Panics` 文档部分（针对仍然可能 panic 的函数）
- 对 `std::env::current_dir()`、`std::env::var()` 等易错 IO 调用统一封装为返回 `Result` 的 helper
- 对 `serde_json::to_value()` 等序列化调用统一封装

---

## 不需要处理的文件

以下文件/模块的 unwrap/expect 全部在 `#[cfg(test)]` 内或测试文件中，**不需要修改**：

| 文件 | 原因 |
|------|------|
| `crates/allthecodes-tools/src/semantic_tool_tests.rs` | 纯测试文件 |
| `crates/allthecodes-tools/src/fs/safe_write.rs` (46 处被标记) | 全部在 `#[cfg(test)]` 中 |
| `crates/allthecodes-permissions/src/decision.rs` (36 处被标记) | 全部在 `#[cfg(test)]` 中 |
| `crates/allthecodes-tools/src/hooks/memdir/mod.rs` (57 处被标记) | 全部在 `#[cfg(test)]` 中 |
| `crates/allthecodes/src/ui/app/tests.rs` | 纯测试文件 |
| `crates/allthecodes-api/src/api/auth_env.rs` (92 处被标记) | 全部在 `#[cfg(test)]` 中 |
| `crates/allthecodes-mcp/src/tests.rs` (79 处被标记) | 纯测试文件 |
| `crates/allthecodes-ipc-protocol/src/test_helpers.rs` | 纯测试文件 |
| `crates/allthecodes-ipc-protocol/src/subnormalized.rs` (60 处被标记) | 全部在 `#[cfg(test)]` 中 |
| `crates/allthecodes-ipc-protocol/src/normalized.rs` (50 处被标记) | 全部在 `#[cfg(test)]` 中 |

**规则**: 任何 `#[cfg(test)] mod tests { ... }` 或 `tests/` 目录下的文件都不要碰。

---

## 工作量估算

| 阶段 | 内容 | 文件数 | 变更量 | 风险 | 估算天数 |
|------|------|--------|--------|------|---------|
| 1 | clippy.toml + workspace lints | 2-3 | 小 | 低 | 0.5 |
| 2 | 锁 unwrap → expect 加消息 | ~9 | ~30 处替换 | 极低 | 0.5 |
| 3 | B 类逻辑修复（细粒度） | ~25 | ~70 处重构 | 中 | 3-4 |
| 4 | CI 集成 | 1-2 | 小 | 低 | 0.5 |
| 5 | 可选增强 | ~5 | 中 | 低 | 1-2 |
| **合计** | | **~40 个文件** | **~100 处生产代码变更** | | **~5-7 人天** |

## 验收标准

1. `cargo clippy --all-targets --all-features -- -D warnings` 通过
2. `cargo test --all-targets --all-features` 全部通过
3. 生产代码中 `#[cfg(not(test))]` 环境下不再有裸 `.unwrap()` 调用
4. 所有保留的 `.expect()` 都附有解释性消息
5. 所有 `panic!()` 都有注释说明为什么这是正确行为
6. CI 中有 clippy 检查防止回潮
