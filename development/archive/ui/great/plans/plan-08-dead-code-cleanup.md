# 执行计划：plan-08-dead-code-cleanup

## 当前状态

Rust UI 代码库中存在 50+ 处 `#[allow(dead_code)]` 注解，分布在 18+ 个文件中。这些注解掩盖了编译器的未使用代码警告，使多个子系统处于"已实现但未接线"的状态。

### 死代码按区域分类

#### 1. 权限系统（42 处，17 个 `mod.rs` 文件）
详情见 `plan-07-permissions-wiring.md`。主索引文件 `permissions.rs` 中 29 个模块全部标记死代码（仅 `dialog_overlay` 除外）。

#### 2. `messages.rs` — 8 个子模块（35 处）
```
/ui/messages.rs:3-105 — 每 3 行一个 `#[allow(dead_code)]`，8 个子模块各有多处
```
子模块：`user_agent_notification_message`、`system_api_error_message`、`system_text_message`、`user_text_message`、`assistant_text_message`、`assistant_tool_use_message`、`attachment_message`、`user_tool_result_message`。

这些是消息渲染器。它们实现为 `pub fn render_*(...) -> String` / `Vec<Line>`，由 `render.rs` 中的中央调度器调用。标记死代码的原因是：完整的消息类型枚举尚未被覆盖，且某些变体尚未被 `render.rs` 完全引入。

#### 3. `app/agent_navigation.rs` — 模块级（1 处）
路径：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app.rs:1`

整个 `agent_navigation` 模块标记为死代码。它包含：
- `AgentThreadEntry`（20 行）— 线程元数据
- `AgentNavigationState`（~30 行）— BTreeMap 状态的导航模型
- 方法：`upsert`、`remove`、`current`、`navigate`、`entries`、`active_threads_count`

**原因**：`AgentNavigationState` 不在 `App` 结构体中；`render_agent_tree` 已实现但从未被调用。渲染管线中无 agent 树/线程状态。

#### 4. `metadata.rs` vs `render.rs` — 冗余函数（13 个重复函数）
两个文件包含**完全相同的函数签名**：

| 函数名 | metadata.rs:行 | render.rs:行 |
|--------|:--------------:|:------------:|
| `message_copy_text` | 5 | 250 |
| `message_primary_reference` | 25 | 270 |
| `message_content_copy_text` | 34 | 894 |
| `content_block_copy_text` | 45 | 912 |
| `image_reference` | 60 | 940 |
| `tool_input_summary` | 68 | 861 |
| `tool_primary_input` | 80 | 869 |
| `message_content_reference` | 105 | 905 |
| `content_block_reference` | 112 | 927 |
| `attachment_copy_text` | 125 | 948 |
| `attachment_reference` | 140 | 963 |
| `strip_system_reminders` | 153 | 976 |
| `tool_result_content_text` | 166 | 517 |

关键区别：
- `metadata.rs`：全部标记 `pub(in crate::ui)`，但没有被非测试代码引用（死代码）
- `render.rs`：部分相同函数标记为 `pub(in crate::ui)` 或 `pub fn`，部分为 `fn`（私有的），且被 `render_single_message` 等实际渲染代码引用

**根本原因**：可能是机械生成或手动复制的结果。`metadata.rs` 似乎是一个较旧的独立模块，后来功能被整合到 `render.rs` 中，但原始文件未被删除。

| 指标 | 值 |
|------|:----:|
| metadata.rs 行数 | 175 |
| metdata.rs 中 **0 个**独有函数 | 是 |
| render.rs 行数 | 1145 |
| 冗余行数（估计） | ~120（13 个函数的主体） |

#### 5. `mod.rs` — 顶级 UI 模块（35 处）
路径：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/mod.rs`

覆盖大多数子模块，包括：
- `agents`、`app`、`diff`、`hooks`、`lsp_recommendation`、`mcp`、`memory`、`messages`、`permissions`、`skills`、`tasks`、`teams`
- 组件：`approval_overlay`、`better_view_panel`、`bottom_pane`、`chat_composer`、`chatwidget` 等 15+ 个
- 输入：`clipboard_paste`、`clipboard_text`、`file_search`、`insert_history` 等 7 个
- 渲染：`markdown`、`markdown_render`、`spinner`、`theme` 等 13 个
- 运行时：`event_router`、`frame_requester`、`streaming_controller` 等 7 个
- 平台：`audio_device`、`custom_terminal` 等 5 个

这些注解实际上是一种架构选择：`mod.rs` 导出所有模块供 `crate::ui` 级别使用，但实际的入口点 `tui.rs` 可能只静态链接了其中的一部分，其他模块仅用于 `#[cfg(test)]`。

**真实状态**：这些模块中许多确实在编译后被使用（例如 `tui.rs` 引用 `App` 结构体，而 `App` 使用 `render.rs`），但编译器和 `dead_code` lint 只能追踪到模块是否在 crate 内被引用。由于 `mod.rs` 是顶级，许多模块可能仅通过导出可达，但不是从二进制入口直接引用。

### 死代码汇总

| 区域 | 文件数 | `#[allow(dead_code)]` 实例 | 行数 | 风险 | 优先度 |
|------|:-----:|:--------------------------:|:----:|:----:|:------:|
| 权限系统 | 17 | 42 | ~60 | 高 — 功能就绪但未接线 | **P0**（接线前置）|
| `messages.rs` | 1 | 35 | ~50 | 中 — 消息渲染器由中央调度 | **P2** |
| `agent_navigation.rs` | 2（模块级） | ~1 | ~80 | 高 — 代码已写好 | **P1**（易接线）|
| `metadata.rs` 冗余 | 1 | 0（无 dead_code 注解） | 175 | 低 — 纯重复 | **P2** |
| `mod.rs` 模块级 | 1 | 35 | ~50 | 低 — 架构选择 | **P3** |
| **总计** | **22** | **~113** | **~415** | | |

## 目标状态

1. 所有 `#[allow(dead_code)]` 注解从代码库中移除
2. 任何标记过的代码要么被接线（被真实代码引用），要么被删除（如果确实是未使用的遗留代码）
3. `metadata.rs` 被删除，其公共 API 改为委托给 `render.rs`
4. `mod.rs` 中的模块级死代码注解处理为：接线、删除或替换为显式的 `#[cfg(test)]` 守卫
5. 编译器不再在清理后的区域产生 `dead_code` 警告

## 分步实施

### Phase 1：权限系统死代码（与 plan-07 同步）

这是两个计划的高级依赖。权限系统的死代码是 plan-07 的前置条件。实施 Plan-07 Phase 1-5 的同时自然移除了权限系统的死代码。

**不进行独立工作**。权限死代码是接线工作的一部分，不是独立任务。

### Phase 2：`app/agent_navigation.rs` 接线

**工作量**：小（0.5-1 天）
**影响**：高（代码已实现；只需移除 `#[allow(dead_code)]` 并创建连接点）

步骤：

1. **将 `AgentNavigationState` 添加到 `App` 结构体中**
   - 当前 `app.rs:2` 有 `#[allow(dead_code)] pub mod agent_navigation;`
   - 分析 `AgentNavigationState` 的依赖项
   - 在 App 中添加字段：`agent_nav: AgentNavigationState`
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app/agent_navigation.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app/input.rs`（事件处理）

2. **接入 `render_agent_tree` 调用**
   - 确定渲染管线中 agent 树面板应该出现的位置（线程列表/状态栏/侧边面板？）
   - 根据设计意图将 agent 树渲染到相应的视图区域
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app/render.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app.rs`

3. **移除 `#[allow(dead_code)]`**
   - 从 `app.rs:2` 中移除 `#[allow(dead_code)]`
   - 从 `agent_navigation.rs` 模块级注解中移除（如果存在）
   - 验证编译通过且无 dead_code 警告

### Phase 3：`messages.rs` 子模块死代码

**工作量**：小（1-2 天）
**影响**：中（需要验证每个模块是否真的是死代码）

步骤：

1. **分类每个子模块**
   - 检查每个 `#[allow(dead_code)] pub mod X` — X 是否在 `render.rs` 或 `mod.rs` 中被引用？
   - 如果被引用，移除 `#[allow(dead_code)]`
   - 如果未被引用，决定是移除模块（如果过时）还是找出它应该被引用的位置

2. **接线缺失的引用**
   - 可能的缺失接线：`user_agent_notification_message`、`system_api_error_message` 等可能未在 `render.rs` 消息调度中被完全引入
   - 将这些模块的公开函数引入调度器

3. **移除注解**
   - 结果：`messages.rs` 的 35 处 `#[allow(dead_code)]` 全部移除

### Phase 4：`metadata.rs` 去重

**工作量**：极小（0.5 天）
**影响**：低（纯清理）

步骤：

1. **验证冗余**
   - 确认 `metadata.rs` 中的 13 个函数与 `render.rs` 中的同名函数完全相同（不是近似匹配）
   - 检查函数体是否有差异（二分查找模式：每个函数比较逻辑）

2. **更新使用 metadata.rs 的引用**
   - 搜索 `crate::ui::messages::metadata::XXX` 的所有引用
   - 将它们重定向到 `render.rs` 中的对应函数
   - 注意：某些 `metadata.rs` 函数是 `pub(in crate::ui)`，`render.rs` 中对应函数也应设置为相同的可见性

3. **删除 `metadata.rs`**
   - 删除 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/messages/metadata.rs`
   - 从 `messages/mod.rs` 中移除 `pub mod metadata;` 行
   - 验证编译通过

### Phase 5：`mod.rs` 模块级死代码

**工作量**：中（2-3 天）
**影响**：中（需要逐个模块检查）

步骤：

1. **对每个标记的模块进行分类**

| 模块 | `#[allow(dead_code)]` | 分类 | 动作 |
|------|:---------------------:|:----:|:----:|
| `agents` | 有 | 可能是过时的 | 检查是否被引用 → 接线或标记为 `#[cfg(feature = "agents")]` |
| `app` | 有 | 实际被使用 | 被 `tui.rs` 间接引用 → 用 `#[cfg_attr(not(target_os = "windows"), allow(dead_code))]` 或检查引用链 |
| `diff` | 有 | 实际被使用 | 类似的间接引用问题 |
| `hooks` | 有 | 实际被使用 | 检查引用 |
| `lsp_recommendation` | 有 | 可能是新功能 | 检查是否已集成 |
| `mcp` | 有 | 可能是 feature-gated | 检查 feature gate |
| `memory` | 有 | 可能是 feature-gated | 检查 |
| `messages` | 有 | 实际被使用 | Phase 3 会解决此问题 |
| `permissions` | 有 | 实际被使用 | Phase 1 会解决此问题 |
| `skills` | 有 | 可能是过时的 | 检查 |
| `tasks` | 有 | 可能是过时的 | 检查 |
| `teams` | 有 | 可能是过时的 | 检查 |
| 15+ 组件模块 | 有 | 各种状态 | 逐个检查 |
| 7 个输入模块 | 有 | 各种状态 | 逐个检查 |
| 13 个渲染模块 | 有 | 各种状态 | 逐个检查 |
| 7 个运行时模块 | 有 | 各种状态 | 逐个检查 |
| 5 个平台模块 | 有 | 各种状态 | 逐个检查 |

2. **对于实际被使用的模块**（如 `app`、`messages`、`permissions`、`diff`）：
   - 根本原因：`mod.rs` 无条件导出模块，但入口二进制（`main.rs` → `tui.rs`）可能根据平台/功能有条件地编译
   - 解决方案：
     - 将 `mod` 声明移到 `tui.rs` 中，只在需要时编译
     - 或者添加显式的 `#[cfg_attr(not(any(feature = "tui", feature = "test")), allow(dead_code))]`

3. **对于真正的孤立马模块**（如可能过时的 `agents`、`tasks`、`teams`）：
   - 评审模块是否属于当前架构
   - 如果过时：删除模块及其文件
   - 如果属于未来计划：添加显式的 `#[cfg(feature = "future")]` 守卫

4. **清理 `mod.rs`**
   - 为每个模块使用显式的条件编译守卫
   - 仅在实际不需要的地方保留 `#[allow(dead_code)]`（且用注释解释原因）
   - 目标：`mod.rs` 中零 `#[allow(dead_code)]`

## 工作量估算

| Phase | 描述 | 预估工作量 | 并行性 |
|-------|------|:----------:|:------:|
| Phase 1 | 权限系统死代码 | 与 plan-07 同步 | 阻塞于 plan-07 |
| Phase 2 | agent_navigation 接线 | 0.5-1 天 | 与 plan-07 并行 |
| Phase 3 | messages.rs 死代码 | 1-2 天 | 与 Phase 2 并行 |
| Phase 4 | metadata.rs 去重 | 0.5 天 | 与 Phase 2/3 并行 |
| Phase 5 | mod.rs 模块级清理 | 2-3 天 | 起始依赖 Phase 2/3/4 |
| **总计** | | **4-7 天** | |

## 依赖关系图

```
Phase 5 (mod.rs) ─── 依赖 Phase 2, 3, 4 完成
        │
Phase 2 (agent_nav) ── 独立
        │
Phase 3 (messages) ─── 独立
        │
Phase 4 (metadata) ─── 独立
        │
Phase 1 (permissions) ── 由 plan-07 覆盖（依赖关系在那边处理）
```

**关键发现**：Phase 2、3、4 完全独立，可以并行进行。Phase 5 应最后进行，因为它可能影响 mod.rs 中的所有模块声明。

## 风险/注意

1. **`mod.rs` 中大量模块级死代码的真实原因**：这些可能不是真正的"死代码"，而是 Rust 编译器对"模块已声明但未从二进制入口直接引用"的过度报告。许多模块通过 `crate::ui::XXX` 模式仅用于测试。解决方案应该是条件编译（`#[cfg(test)]`），而不是在模块声明上放置 `#[allow(dead_code)]`。

2. **`messages.rs` 子模块死代码**：同样，这些消息渲染器被 `render.rs` 引用，但可能是 `render.rs` 中的调度代码没有引用所有变体。真正的修复是确保调度器穷举所有渲染变体。

3. **`metadata.rs` 删除需要小心**：在删除前必须逐函数验证 `render.rs` 中的版本是**功能相同的**。如果有细微差异（如不同的截断逻辑或格式），需要先合并。

4. **避免回归**：当前测试套件（特别是 `permissions.rs` 中的 snapshot 测试）依赖于现有代码结构。清理后验证所有测试仍能通过，特别是 snapshot 测试。

5. **`agent_navigation.rs` 接线后可能需要 UI 布局变更**：agent 树面板将占用屏幕空间。确保已有布局槽位或新增加载方案。

6. **不要在 Phase 5 中意外删除 "保留但仅测试用" 的模块**：某些模块可能被 `#[cfg(test)]` 测试引用但不在生产代码中引用。这些应该用 `#[cfg(any(test, feature = "tui"))]` 保护，而不是删除。

## 实施后遗留问题（2026-05-20）

本计划的生产路径清理已经完成：`crates/claude-code-rs/src/ui/` 下不再保留 `dead_code` 相关 allow，`messages/metadata.rs` 已删除，生产 `cargo check -p claude-code-rs` 与 `cargo build --workspace --release` 未产生 Rust 警告。

仍需在后续计划中跟踪：

1. `cargo test -p claude-code-rs ui:: -- --nocapture` 仍会在测试目标中报告若干 `dead_code` 警告，主要来自仅测试引用或尚未完整接线的 UI 镜像表面；本次清理目标是生产构建零 `dead_code` 宽限，而不是测试目标全 lint 归零。
2. 部分 API 通过 `#[cfg(test)]`、小型 marker/helper 方法或集中测试引用保留，以避免未完成表面被误删；后续当这些表面进入生产渲染路径时，应移除测试门控并用真实调用替代。
3. 本计划未覆盖 `unused_imports`、`unused` 或测试 target 下的所有普通 warning；全 workspace/all-targets warning budget 已单独开计划：`development/archive/plan/workspace-all-targets-warning-budget-plan-2026-05-20.md`。
4. `mod.rs` 的模块暴露已经收敛到当前生产入口可接受的形态，但 Rust UI 仍保留大量上游 parity 模块；新增模块时应优先接线、删除或显式 `cfg(test)`，不要重新引入无说明的 `#[allow(dead_code)]`。
