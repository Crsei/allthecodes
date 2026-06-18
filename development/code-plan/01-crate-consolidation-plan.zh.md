# Crate 合并与精简计划

> 基于 `10-api-architecture-implementation-review.md` 的架构观察和依赖分析。
> 目标：41 个 crate → ~30 个，减少 ~25% 的构建单元，降低认知负载。

---

## 总览

| 阶段 | 操作 | 涉及合并 | 影响范围 | 风险 |
|------|------|----------|----------|------|
| 0 | IPC 栈合并（5→1~2） | `ipc-protocol`, `ipc-transport`, `ipc-adapters`, `ipc-client` → `ipc` | 5 个 crate 的生产者/消费者 | 🟡 中 |
| 1 | 微型 crate 内联 | `worktree`→`engine`, `safety`→`engine`, `keybindings`→`main` | 3 个删除 | 🟢 低 |
| 2 | 可观测性合并 | `langfuse`→`observability` | 1 个删除 | 🟢 低 |
| 3 | 架构重叠合并 | `gateway`→`daemon` | 1 个删除 | 🔴 高 |
| 4 | 可选合并 | `models`, `voice`, `startup`, `tasks` | 待定 | 🟡 中 |

---

## 阶段 0：IPC 栈合并（高优先级）

### 现状

```
allthecodes-ipc           (6 个文件) — 主模块、subsystem handlers、runtime
allthecodes-ipc-protocol  (11 个文件) — 信封类型 + 子系统事件
allthecodes-ipc-transport (5 个文件) — JSONL 帧、sink
allthecodes-ipc-adapters  (1 个文件) — 协议适配器
allthecodes-ipc-client    (9 个文件) — 客户端、sink、transport、callbacks
```

### 目标

合并为一个 crate: **`allthecodes-ipc`**（保留），删除其余 4 个。

### 步骤

1. **重构 `allthecodes-ipc/src/lib.rs`**
   - 保留原有公有 API（头戴式、子系统事件、agent handlers）
   - 增加 `pub mod protocol`（当前 `ipc-protocol/` 的内容）
   - 增加 `pub mod transport`（当前 `ipc-transport/` 的内容）
   - 增加 `pub mod adapters`（当前 `ipc-adapters/` 的内容）
   - 增加 `pub mod client`（当前 `ipc-client/` 的内容）

2. **移动文件**
   ```
   crates/allthecodes-ipc-protocol/src/       →  crates/allthecodes-ipc/src/protocol/
   crates/allthecodes-ipc-transport/src/       →  crates/allthecodes-ipc/src/transport/
   crates/allthecodes-ipc-adapters/src/lib.rs  →  crates/allthecodes-ipc/src/adapters.rs
   crates/allthecodes-ipc-client/src/          →  crates/allthecodes-ipc/src/client/
   ```

3. **更新 `Cargo.toml`**
   - 合并所有依赖项到 `allthecodes-ipc/Cargo.toml`
   - 删除其余 4 个 crate 的 `Cargo.toml` 和目录
   - 从 workspace `members` 中移除 4 个 crate

4. **更新消费者**
   全局搜索替换（约 40+ 处引用）：
   - `allthecodes_ipc_protocol::` → `allthecodes_ipc::protocol::`
   - `allthecodes_ipc_transport::` → `allthecodes_ipc::transport::`
   - `allthecodes_ipc_adapters::` → `allthecodes_ipc::adapters::`
   - `allthecodes_ipc_client::` → `allthecodes_ipc::client::`

### 风险缓解

- `ipc-client` 作为独立 crate 的意义是允许外部使用者仅依赖客户端部分。合并后外部消费者需依赖整个 `allthecodes-ipc`——但所有 IPC 消费者已是内部 crate，这不是问题。
- 使用 `#[doc(hidden)]` 和 `pub use` 重导出来保持向后兼容 API。

### 预计收益

- 编译时间：减少 4 个单独的编译单元 → 更快的增量编译
- 认知负载：1 个概念单元代替 5 个

---

## 阶段 1：微型 crate 内联（高优先级）

### 1a. `allthecodes-worktree` → `allthecodes-engine`

- **源文件**: `mod.rs` (30 行) + `tool.rs` (200 行)
- **消费者**: 仅 `allthecodes-engine`, `allthecodes-startup`
- **动作**:
  1. 将 `tool.rs` 内容移至 `allthecodes-engine/src/worktree_tool.rs`
  2. 将 `mod.rs` 中的状态类型移至 `allthecodes-engine/src/` 下
  3. 在 `engine/src/lib.rs` 中添加 `pub mod worktree`
  4. 更新 `engine/Cargo.toml` 添加 `worktree` 的专用依赖
  5. 删除 `allthecodes-worktree` crate 和其在 `Cargo.toml workspace.members` 中的条目
  6. 更新 `allthecodes-startup` 的依赖（从 `allthecodes-worktree` 改为 `allthecodes-engine`）

### 1b. `allthecodes-safety` → `allthecodes-engine`

- **源文件**: `lib.rs` + `classifier.rs`（约 300 行）
- **消费者**: 仅 `allthecodes-engine` 和 `allthecodes-query` 通过引擎间接使用
- **动作**:
  1. 移至 `allthecodes-engine/src/safety/mod.rs` + `classifier.rs`
  2. 在 `engine/src/lib.rs` 中添加 `pub mod safety`
  3. 合并 `Cargo.toml` 依赖
  4. 删除 `allthecodes-safety` crate

### 1c. `allthecodes-keybindings` → `allthecodes`（主 TUI crate）

- **源文件**: 7 个文件，约 500 行
- **消费者**: `allthecodes`（主 crate）、`allthecodes-engine`、`allthecodes-commands`
- **动作**:
  1. 移至主 crate 的 `src/keybindings/` 目录
  2. 在主 crate 的 `lib.rs`(或 `mod.rs`) 中导出
  3. 更新 `engine` 和 `commands` 的依赖引用

---

## 阶段 2：可观测性合并（中优先级）

### `allthecodes-langfuse` → `allthecodes-observability`

- **源文件**: `lib.rs`, `sanitize.rs`, `stub.rs`, `convert.rs`
- **消费者**: `allthecodes-engine`, `allthecodes-services`
- **动作**:
  1. 在 `allthecodes-observability/src/langfuse/` 下创建子模块
  2. 使用 feature gate：`langfuse` feature 控制是否包含 langfuse 相关代码
  3. 通过 `pub use` 保留向后兼容的路径
  4. 更新 `engine/Cargo.toml` 和 `services/Cargo.toml` 的依赖

---

## 阶段 3：架构重叠合并（高风险-需要仔细设计）

### `allthecodes-gateway` → `allthecodes-daemon`

**这是最高风险的操作**，因为两个 crate 都有相当的复杂度，且涉及运行时行为。

### 前置条件

- [ ] 理解 `gateway` 的 API（`api.rs`, `api_support.rs`, `store/`, `delivery.rs`）如何被 daemon 的代码引用
- [ ] 确认 daemon 的 `gateway_client.rs` 是 gateway crate 的消费者还是提供者
- [ ] 确认 `gateway_bridge.rs` 的角色

### 计划步骤

1. **代码审计** — 映射两个 crate 之间的所有引用关系
2. **创建 `daemon/src/gateway/` 模块**
   - 引入 gateway crate 的代码
   - 保留 `gateway::` 路径前缀以减少破坏性变更
3. **处理 webhook**（两个 crate 都有 `webhook` 模块）
   - 合并 `gateway/webhook.rs` + `gateway/webhook_render.rs` + `gateway/delivery.rs` 到 daemon 的 webhook 模块
4. **处理适配器**（`gateway/adapters/lark.rs`, `telegram.rs`）
   - 移至 `daemon/src/adapters/`
5. **清理**
   - 删除 `allthecodes-gateway` crate
   - 更新 workspace `Cargo.toml`

---

## 阶段 4：可选合并（低优先级，按需决定）

### 4a. `allthecodes-models` → 何处去？

**选项 A** → `allthecodes-protocol`：模型元数据、别名、定价与 API 协议定义高度相关
**选项 B** → `allthecodes-types`：纯类型 crate，与现有类型自然融合
**选项 C** → 保持独立：如果模型数据频繁独立于协议发布，保留独立 crate 也有道理

**建议**: 选项 A（`allthecodes-protocol`）

### 4b. `allthecodes-tasks`（13 个文件）

- 零 inter-crate 依赖（仅 std + uuid + chrono）
- 被 6 个 crate 引用
- **建议**: 暂时保留。任务是核心领域概念，拆分合理。等待观察是否会因其他合并导致依赖循环再做决定。

### 4c. `allthecodes-startup`（7 个文件）

- 启动编排逻辑，仅在 `allthecodes` 主 crate 中调用
- **建议**: 内联到 `allthecodes` 主 crate 的 `src/startup/`

### 4d. `allthecodes-voice`（6 个文件）

- 语音听写功能，如果非活跃开发则移除或内联
- **建议**: 若不活跃则标记为 deprecated，下一版本移除

---

## 风险矩阵

| 风险 | 阶段 | 描述 | 缓解措施 |
|------|------|------|----------|
| 🔴 | 3 | `gateway` + `daemon` 合并可能导致运行时 bug | 分步进行，每步测试 |
| 🟡 | 0 | IPC 模块间存在隐式循环引用 | 先做静态分析 |
| 🟡 | 1b | `safety` 可能被预期为独立的安全边界 | 确认无安全上下文隔离依赖 |
| 🟢 | 0-4 | 全局搜索可能遗漏字符串形式的引用 | `cargo check` 每个阶段后 |
| 🟢 | 0-4 | CI 兼容性 | 每个 commit 保持可编译 |

---

## 执行顺序建议

```
阶段 0 (IPC 合并)
  │
  ▼
阶段 1 (微型 crate 内联)
  │
  ▼
阶段 1b (safety → engine)
  │
  ▼
阶段 2 (langfuse → observability)
  │
  ▼
cargo check --workspace  【中间验证点】
  │
  ▼
阶段 3 (gateway → daemon)  ← 需要前置代码审计
  │
  ▼
阶段 4 (可选)  ← 按需决定
  │
  ▼
cargo test --workspace  【最终验证】
```

每个阶段完成后都应该能独立通过 `cargo check` 和 `cargo test`。

---

## 预计收益

| 指标 | 当前 | 预期 | 改善 |
|------|------|------|------|
| Workspace crate 数量 | 41 | ~30 | -27% |
| 独立编译单元 | 41 | ~30 | -27% |
| 增量编译时间 | 基线 | 减少 ~20% | 更多内联减少 LLVM 代码生成 |
| `cargo check --workspace` | 基线 | 减少 ~15% | 减少元数据序列化 |
