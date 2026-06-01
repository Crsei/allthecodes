# system_prompt.rs 拆分计划

> 原文件: `crates/cc-engine/src/system_prompt.rs`
> 原始行数: ~1542
> 目标: 拆分为 4 个子模块，每个 ≤ 400 行

## 当前结构分析

`system_prompt.rs` 负责构建发送给 LLM 的完整系统提示词。文件可明确分为以下逻辑区块：

1. **静态段定义**（L25–L189）：7 个函数生成不随会话变化的提示词段落（intro、system、doing_tasks、actions、using_tools、tone_and_style、output_efficiency），加上 `CYBER_RISK_INSTRUCTION` 常量和 `format_bullets`/`format_sub_bullets` 辅助函数。
2. **动态段定义**（L190–L486）：9 个函数/类型生成随环境变化的段落（env_info、git_status、language、mcp_instructions、coordinator_prompt、computer_use、browser），加上 `BrowserToolInfo` 结构体、`detect_browser_tools` 内部函数和 `SUMMARIZE_TOOL_RESULTS` 常量。
3. **组装逻辑**（L487–L937）：3 个 `build_system_prompt*` 公共函数串联静态/动态段、注入 CLAUDE.md 上下文和 Memory 上下文；`build_effective_system_prompt` 处理优先级覆盖；`build_subsystem_status_reminder` 构建子系统状态。
4. **测试**（L938–L1542）：~600 行测试代码，包含 `EnvGuard`/`FeatureOverrideGuard` 测试辅助结构。

**外部依赖关系**：
- `crate::config::claude_md`（CLAUDE.md 加载）
- `crate::prompt_sections`（段落缓存系统）
- `crate::types::tool::Tool`（工具描述）
- `crate::output_style`（输出风格解析）
- `crate::utils::git`（Git 状态查询）
- `crate::agent_runtime`（活跃 Agent 计数）
- `cc_browser`（浏览器 MCP 工具检测）
- `cc_config::features`（功能标志）
- `cc_lsp_service`（LSP 服务发现）
- `cc_mcp::discovery`（MCP 服务发现）
- `cc_session::memdir`（记忆上下文）
- `cc_skills`（技能加载）

**公共 API 消费方**：
- `lifecycle/submit_message.rs` L464 调用 `system_prompt::build_system_prompt_with_memory_contexts()`

## 拆分方案

### 子模块 1: `static_sections.rs` (~210 行)
- **职责**: 定义所有不随会话状态变化的提示词段落函数，以及段落格式化辅助工具。
- **迁移内容**:
  - `const CYBER_RISK_INSTRUCTION` (L30–L36)
  - `fn intro_section()` (L43–L53)
  - `fn system_section()` (L56–L67)
  - `fn doing_tasks_section()` (L70–L94)
  - `fn actions_section()` (L97–L106)
  - `fn using_tools_section(enabled_tools: &[&str])` (L109–L164)
  - `fn tone_and_style_section()` (L167–L176)
  - `fn output_efficiency_section()` (L179–L188)
  - `fn format_bullets(items: &[&str])` (L877–L883)
  - `fn format_sub_bullets(items: &[&str])` (L886–L893)
  - 对应区块分隔注释 (L25–L27, L38–L40, L872–L874)
- **可见性**: 全部 `pub(super)` — 仅供 `mod.rs` 组装函数调用
- **依赖**: 无外部 crate 依赖（纯字符串拼接）
- **被依赖**: `mod.rs`（组装函数中直接调用 `intro_section()`, `system_section()` 等）

### 子模块 2: `dynamic_sections.rs` (~340 行)
- **职责**: 定义所有随运行时环境变化的提示词段落函数，包括 Git 状态、环境信息、MCP/浏览器工具检测等。
- **迁移内容**:
  - `fn env_info_section(model, cwd)` (L195–L228)
  - `fn git_status_section(cwd)` (L236–L319)
  - `fn language_section(language)` (L322–L332)
  - `fn mcp_instructions_section()` (L335–L339)
  - `fn coordinator_prompt_section()` (L341–L358)
  - `fn computer_use_system_prompt(tools)` (L360–L387)
  - `struct BrowserToolInfo` (L389–L394)
  - `fn browser_system_prompt(tools, browser_server_names)` (L396–L451)
  - `fn detect_browser_tools(tools, browser_server_names)` (L453–L480)
  - `const SUMMARIZE_TOOL_RESULTS` (L483–L485)
  - `fn build_subsystem_status_reminder()` (L901–L936)
  - 对应区块分隔注释 (L190–L192, L487–L489, L894–L896)
- **可见性**: `env_info_section`, `git_status_section` 等计算函数为 `pub(super)`；`BrowserToolInfo` 和 `detect_browser_tools` 为私有（文件内辅助）
- **依赖**:
  - `crate::utils::git`（`git_status_section` 中使用）
  - `crate::config::constants`（`env_info_section` 中使用）
  - `crate::types::tool::Tool`（`computer_use_system_prompt` 和 `browser_*` 函数签名）
  - `cc_browser`（浏览器 MCP 工具检测与分类）
  - `cc_config::features`（`coordinator_prompt_section` 中使用）
  - `cc_lsp_service`, `cc_mcp::discovery`, `cc_skills`, `crate::agent_runtime`（`build_subsystem_status_reminder` 中使用）
- **被依赖**: `mod.rs`（组装函数中调用各计算函数，并通过 `prompt_sections::cached_section` / `uncached_section` 注册）

### 子模块 3: `mod.rs` (~340 行)
- **职责**: 公共 API 入口 — 系统提示词组装主逻辑，串联静态/动态段、注入 CLAUDE.md 和 Memory 上下文、构建工具描述、处理优先级覆盖策略。
- **迁移内容**:
  - 模块级文档注释 (L1–L14)
  - `use` 语句（L15–L23，按需拆分到各子模块后 mod.rs 保留组装所需）
  - `pub fn build_system_prompt()` (L500–L527) — `#[cfg(test)]` 辅助
  - `pub fn build_system_prompt_with_session_memory()` (L529–L561)
  - `pub fn build_system_prompt_with_memory_contexts()` (L563–L828) — 核心组装函数
  - `pub fn build_effective_system_prompt()` (L830–L870) — `#[cfg(test)]` 辅助
  - `mod static_sections;` 声明
  - `mod dynamic_sections;` 声明
  - `#[cfg(test)] mod tests;` 声明
- **可见性**: 3 个 `pub fn` 保持公共（与原文件相同）
- **依赖**:
  - `super::static_sections`（各静态段函数）
  - `super::dynamic_sections`（各动态段计算函数）
  - `crate::config::claude_md`（CLAUDE.md 上下文加载）
  - `crate::prompt_sections`（段落缓存/注册系统）
  - `crate::output_style`（输出风格解析，仅在闭包中使用）
  - `cc_session::memdir`（Memory 上下文加载）
  - `cc_browser::detection`（浏览器 MCP 服务快照）
  - `chrono`（日期格式化，用于 `user_context`）
  - `serde_json`（工具 schema 序列化）
- **被依赖**:
  - `lifecycle/submit_message.rs` L464（调用 `build_system_prompt_with_memory_contexts`）
  - 外部测试模块（通过 `#[cfg(test)]` 的 `build_system_prompt` / `build_effective_system_prompt`）

### 子模块 4: `tests.rs` (~600 行)
- **职责**: 所有单元测试，包括测试辅助结构体。
- **迁移内容**:
  - `mod tests` 块内容 (L943–L1542)
  - `struct EnvGuard` (L949–L969)
  - `struct FeatureOverrideGuard` (L971–L978)
  - 全部 30 个 `#[test]` 函数：
    - `test_intro_section_contains_identity` (L981–L986)
    - `test_intro_section_contains_cyber_risk` (L989–L993)
    - `test_system_section_structure` (L996–L1003)
    - `test_doing_tasks_section` (L1006–L1013)
    - `test_actions_section` (L1016–L1022)
    - `test_using_tools_section` (L1025–L1040)
    - `test_using_tools_without_task_create` (L1043–L1046)
    - `test_tone_and_style` (L1049–L1055)
    - `test_output_efficiency` (L1058–L1062)
    - `test_language_section_none` (L1066–L1068)
    - `test_language_section_some` (L1071–L1076)
    - `test_env_info_section` (L1079–L1084)
    - `test_default_prompt_has_all_sections` (L1087–L1125)
    - `test_context_maps_are_metadata_not_prompt_sections` (L1128–L1146)
    - `test_coordinator_mode_injects_prompt_section_when_enabled` (L1149–L1173)
    - `test_language_setting_injects_section` (L1176–L1194)
    - `test_output_style_explanatory_injects_section` (L1197–L1214)
    - `test_output_style_default_emits_no_section` (L1217–L1234)
    - `test_custom_prompt_replaces_default` (L1237–L1253)
    - `test_append_prompt` (L1256–L1269)
    - `test_build_effective_override` (L1272–L1281)
    - `test_build_effective_agent_replaces_default` (L1284–L1293)
    - `test_build_effective_custom_replaces_default` (L1296–L1300)
    - `test_build_effective_append` (L1303–L1312)
    - `test_format_bullets` (L1315–L1318)
    - `test_claude_md_injection` (L1321–L1335)
    - `test_memory_context_injection` (L1338–L1360)
    - `test_prebuilt_memory_context_overrides_full_memory_scan` (L1363–L1402)
    - `test_auto_memory_context_respects_toggle` (L1407–L1450)
    - `test_session_memory_context_injection` (L1453–L1470)
    - `test_git_status_section_in_git_repo` (L1475–L1483)
    - `test_git_status_section_not_git_repo` (L1486–L1492)
    - `test_git_status_section_contains_main_branch` (L1495–L1504)
    - `test_git_status_section_limits_commits` (L1507–L1522)
    - `test_build_system_prompt_includes_git_status` (L1525–L1541)
- **可见性**: `#[cfg(test)]` 模块，内部测试函数通过 `use super::*` 访问父模块及兄弟模块的 `pub(super)` 项
- **依赖**: `super::mod`（公共 API）、`super::static_sections`（直接测试静态段函数）、`super::dynamic_sections`（直接测试动态段函数）、`crate::config::features`、`crate::prompt_sections`、`cc_session::memdir`、`serial_test`
- **被依赖**: 无（叶子模块）

## 拆分后目录结构

```
system_prompt/
├── mod.rs              (~340 行) — 公共 API，组装主逻辑，re-exports
├── static_sections.rs  (~210 行) — 静态段落定义 + 格式化辅助
├── dynamic_sections.rs (~340 行) — 动态段落定义 + 子系统状态
└── tests.rs            (~600 行) — 全部单元测试 + 测试辅助结构
```

**合计**: ~1490 行（扣除空行/注释后与原文件 1542 行基本对齐，新增了模块声明和 `use` 语句的少量开销）

## 迁移步骤

每一步完成后应能通过 `cargo check -p cc-engine` 和 `cargo test -p cc-engine`。

### 步骤 1: 创建目录结构和 mod.rs 骨架
1. 将 `crates/cc-engine/src/system_prompt.rs` 重命名为 `crates/cc-engine/src/system_prompt/mod.rs`
2. 不修改 `lib.rs`（Rust 自动识别目录模块）
3. 运行 `cargo check -p cc-engine` 确认无变化

### 步骤 2: 提取 `tests.rs`
1. 创建 `crates/cc-engine/src/system_prompt/tests.rs`
2. 将 `mod tests { ... }` 的内部内容（L944–L1542）移到 `tests.rs`
3. 在 `mod.rs` 中替换为 `#[cfg(test)] mod tests;`
4. 在 `tests.rs` 顶部添加 `use super::*;` 和其他所需导入
5. 运行 `cargo test -p cc-engine` 确认所有测试通过

### 步骤 3: 提取 `static_sections.rs`
1. 创建 `crates/cc-engine/src/system_prompt/static_sections.rs`
2. 移入以下内容：
   - `CYBER_RISK_INSTRUCTION` 常量 (L30–L36)
   - `intro_section()` (L43–L53)
   - `system_section()` (L56–L67)
   - `doing_tasks_section()` (L70–L94)
   - `actions_section()` (L97–L106)
   - `using_tools_section()` (L109–L164)
   - `tone_and_style_section()` (L167–L176)
   - `output_efficiency_section()` (L179–L188)
   - `format_bullets()` (L877–L883)
   - `format_sub_bullets()` (L886–L893)
3. 所有函数加 `pub(super)` 可见性
4. 在 `mod.rs` 中添加 `mod static_sections;`，并删除已移出的代码
5. 在 `mod.rs` 的组装函数中改为 `use static_sections::*;` 或通过限定路径调用
6. 更新 `tests.rs` 中对这些函数的调用路径（`static_sections::intro_section()` 等）
7. 运行 `cargo test -p cc-engine` 确认通过

### 步骤 4: 提取 `dynamic_sections.rs`
1. 创建 `crates/cc-engine/src/system_prompt/dynamic_sections.rs`
2. 移入以下内容：
   - `env_info_section()` (L195–L228)
   - `git_status_section()` (L236–L319)
   - `language_section()` (L322–L332)
   - `mcp_instructions_section()` (L335–L339)
   - `coordinator_prompt_section()` (L341–L358)
   - `computer_use_system_prompt()` (L360–L387)
   - `BrowserToolInfo` 结构体 (L389–L394)
   - `browser_system_prompt()` (L396–L451)
   - `detect_browser_tools()` (L453–L480)
   - `SUMMARIZE_TOOL_RESULTS` 常量 (L483–L485)
   - `build_subsystem_status_reminder()` (L901–L936)
3. 为组装函数需要调用的函数添加 `pub(super)` 可见性：`env_info_section`, `git_status_section`, `language_section`, `mcp_instructions_section`, `computer_use_system_prompt`, `browser_system_prompt`, `SUMMARIZE_TOOL_RESULTS`, `build_subsystem_status_reminder`, `coordinator_prompt_section`
4. `BrowserToolInfo` 和 `detect_browser_tools` 保持 `pub(super)`（tests.rs 中可能需要测试 `detect_browser_tools`，或可保持私有）
5. 在 `mod.rs` 中添加 `mod dynamic_sections;`，删除已移出代码
6. 在 `mod.rs` 中添加 `use dynamic_sections::*;` 或限定路径调用
7. 更新 `tests.rs` 中对动态段函数的调用路径（如 `dynamic_sections::env_info_section(...)` 等）
8. 运行 `cargo test -p cc-engine` 确认通过

### 步骤 5: 清理 mod.rs
1. 检查 `mod.rs` 中是否有多余的 `use` 语句（已随代码移到子模块的）
2. 确认 `mod.rs` 仅保留组装逻辑所需的导入
3. 运行 `cargo clippy -p cc-engine` 检查 lint 警告
4. 运行 `cargo test -p cc-engine` 全量确认
5. 运行 `cargo build -p cc-engine --release` 确认 release 编译

## 风险与注意事项

1. **`#[cfg(test)]` 公共函数的可见性**：`build_system_prompt()` 和 `build_effective_system_prompt()` 标注了 `#[cfg(test)]` 和 `pub`。拆分后它们留在 `mod.rs` 中，仍可通过 `crate::system_prompt::build_system_prompt()` 访问。需确认外部测试模块（如有）通过 `crate::system_prompt::*` 导入仍能找到这些函数。

2. **`build_subsystem_status_reminder` 的归属**：此函数虽然位于"子系统状态提醒"区块，但它在组装函数中通过 `cached_section("subsystem_status", build_subsystem_status_reminder)` 注册。将其放入 `dynamic_sections.rs` 是因为它探测 LSP/MCP/Skills/Agents 等运行时状态，属于动态内容。如果未来需要独立测试，可考虑单独文件。

3. **闭包中的移动语义**：`build_system_prompt_with_memory_contexts` 中大量使用 `move ||` 闭包捕获局部变量（`model_owned`, `cwd_owned`, `language_owned` 等）。拆分后这些闭包仍在 `mod.rs` 中，不会受影响，但需注意闭包中调用的函数需要正确的可见性。

4. **`cc_browser` 依赖的隔离**：`computer_use_system_prompt` 和 `browser_system_prompt` 依赖 `cc_browser` crate。将它们移入 `dynamic_sections.rs` 后，只有该子模块需要 `cc_browser` 依赖，有利于编译隔离。但如果 `cc_browser` 是可选依赖（feature-gated），需要在 `dynamic_sections.rs` 上添加对应的 `#[cfg]` 属性。

5. **测试中对内部函数的直接调用**：当前测试直接调用 `intro_section()`、`git_status_section()` 等私有函数（通过 `use super::*`）。拆分后需改为 `use super::static_sections::*` 或 `use super::dynamic_sections::*`。如果测试通过 `mod.rs` 的 re-export 间接访问，需在 `mod.rs` 中添加 `pub(crate) use static_sections::intro_section;` 等 re-export。

6. **Cargo 编译单元**：将单一文件变为目录模块不改变编译单元数量（仍为一个 crate），对编译速度影响微乎其微。

7. **Git blame 归属**：文件重命名（`system_prompt.rs` → `system_prompt/mod.rs`）后，可使用 `git log --follow` 追踪历史。子模块为新文件，首次 commit 的 author 为执行拆分的人。
