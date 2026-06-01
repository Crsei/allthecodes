# dangerous.rs 拆分计划

> 原文件: `crates/cc-permissions/src/dangerous.rs`
> 原始行数: ~1993
> 目标: 拆分为 5 个子模块，每个 <= 400 行

## 当前结构分析

文件包含两大职责，通过以下目录结构组织：

```
crates/cc-permissions/src/dangerous.rs (1993 行)
├── Auto 模式权限管理 (~315 行)
│   ├── Structs: AutoModePermissionStrip, AutoModeRuntimeTransition  (L52-L67)
│   ├── 公共函数: strip/restore/set_permission 等 6 个函数 (L69-L220)
│   └── 私有辅助函数: merge_transition, dangerous_auto_mode_allow_reason 等 (L214-L330)
├── Bash 危险命令检测 (~320 行)
│   ├── DANGER_PATTERNS 静态 Regex 集合 (L332-L432)
│   ├── is_dangerous_command() 公共入口 (L780-L812)
│   └── git_clean_forced_without_dry_run() 专用检测 (L1397-L1422)
├── PowerShell 危险命令检测 (~575 行)
│   ├── POWERSHELL_DANGER_PATTERNS 静态 Regex 集合 (L434-L609)
│   ├── POWERSHELL_CLM_ALLOWED_TYPES 类型白名单 (L611-L760)
│   ├── POWERSHELL_NEW_OBJECT_RE 静态 Regex (L762-L765)
│   ├── is_dangerous_powershell_command() 公共入口 (L814-L852)
│   ├── 语法解析辅助: ~15 个私有函数 (L854-L1396)
│   └── PowerShell 特有检测: obvious parse errors, AST heuristics, type literal checks
└── 测试 (~566 行)
    ├── Auto 模式测试: 6 个 #[test] (L1428-L1701)
    ├── Bash 安全命令测试 (L1704-L1715)
    ├── Bash 危险命令测试: ~11 个 #[test] (L1717-L1788)
    ├── PowerShell 测试: ~6 个 #[test] (L1790-L1992)
    └── 辅助函数: test_permission_context() (L1576-L1590)
```

### 公共 API（外部使用者）

| 符号 | 类型 | 被使用位置 |
|------|------|-----------|
| `is_dangerous_command()` | pub fn | security.rs, bash.rs, classifier.rs |
| `is_dangerous_powershell_command()` | pub fn | security.rs, classifier.rs |
| `set_permission_mode_with_auto_mode_safety()` | pub fn | plan_workflow.rs(2x), config_cmd.rs, handlers.rs, runtime_config.rs, agent/mod.rs, permissions_cmd.rs |
| `strip_dangerous_permissions_for_active_auto_mode()` | pub fn | plan_mode.rs, permissions_cmd.rs |
| `strip_dangerous_permissions_for_auto_mode()` | pub fn | (内部使用) |
| `restore_dangerous_permissions_after_auto_mode()` | pub fn | (内部使用) |
| `restore_auto_mode_stripped_permissions()` | pub fn | (内部使用) |
| `AutoModePermissionStrip` | pub struct | (内部使用) |
| `AutoModeRuntimeTransition` | pub struct | permissions_cmd.rs |

## 拆分方案

### 子模块结构概览

```
dangerous/
├── mod.rs              (~50 行) — 模块声明 + 重导出公共 API
├── auto_mode.rs        (~270 行) — Auto 模式权限管理
├── patterns.rs         (~320 行) — Bash 危险命令模式定义 + 检测入口
├── powershell.rs       (~580 行) — PowerShell 危险命令检测 (可进一步拆分为 clm 子模块)
└── tests.rs            (~566 行) — 所有单元测试
```

### 子模块 1: `auto_mode.rs` (~270 行)

- **职责**: Auto 模式的权限剥离与恢复逻辑，包含相关类型定义
- **迁移内容**:
  - 类型 `AutoModePermissionStrip` (L52-L57)
  - 类型 `AutoModeRuntimeTransition` (L60-L67)
  - `strip_dangerous_permissions_for_auto_mode()` (L75-L104)
  - `restore_dangerous_permissions_after_auto_mode()` (L108-L121)
  - `set_permission_mode_with_auto_mode_safety()` (L125-L155)
  - `strip_dangerous_permissions_for_active_auto_mode()` (L161-L185)
  - `restore_auto_mode_stripped_permissions()` (L188-L212)
  - `merge_transition()` (L214-L220)
  - `dangerous_auto_mode_allow_reason()` (L222-L236)
  - `split_permission_rule()` (L238-L248)
  - `dangerous_shell_allow_reason()` (L250-L289)
  - `auto_allow_content_matches_pattern()` (L291-L304)
  - `auto_allow_content_matches_exact_pattern()` (L306-L312)
  - `windows_exe_auto_allow_pattern()` (L314-L320)
  - `normalize_auto_allow_specifier()` (L322-L329)
  - 所有 Auto Allow Pattern 常量 (L15-L50)

- **依赖**:
  - `cc_types::permissions`
  - `dangerous::patterns` (当前空 — auto_mode 不直接依赖 bash 检测, 但 `dangerous_shell_allow_reason` 引用 Auto Allow Pattern 常量, 这些常量可内联到 auto_mode 中)

- **被依赖**: 外部使用者 (plan_workflow.rs, config_cmd.rs, permissions_cmd.rs, web/handlers.rs, runtime_config.rs, plan_mode.rs, agent/mod.rs)

### 子模块 2: `patterns.rs` (~320 行)

- **职责**: Bash 危险命令的 Regex 模式定义与检测入口函数
- **迁移内容**:
  - `DangerPattern` 结构体 (L332-L335)
  - `DANGER_PATTERNS` LazyLock 静态变量 (L338-L432)
  - `is_dangerous_command()` (L780-L812)
  - `git_clean_forced_without_dry_run()` (L1397-L1422)
  - 模块级 import 行: `regex::Regex`, `cc_utils::bash::{contains_multiline_string, has_unterminated_quotes}`

- **依赖**:
  - `regex::Regex`
  - `cc_utils::bash::{contains_multiline_string, has_unterminated_quotes}`
  - `std::sync::LazyLock`

- **被依赖**:
  - `powershell.rs` (is_dangerous_powershell_command 调用 is_dangerous_command)
  - `tests.rs`
  - 外部使用者: security.rs, bash.rs, classifier.rs

### 子模块 3: `powershell.rs` (~580 行)

- **职责**: PowerShell 危险命令检测，包括 Regex 模式、CLM 类型白名单、语法解析与 AST 启发式检测
- **迁移内容**:
  - `POWERSHELL_DANGER_PATTERNS` LazyLock 静态变量 (L434-L609)
  - `POWERSHELL_CLM_ALLOWED_TYPES` LazyLock 静态变量 (L611-L760)
  - `POWERSHELL_NEW_OBJECT_RE` LazyLock 静态变量 (L762-L765)
  - `is_dangerous_powershell_command()` (L814-L852)
  - `powershell_obvious_parse_error_reason()` (L854-L925)
  - `powershell_ast_heuristic_reason()` (L927-L1034)
  - `powershell_segment_command_before()` (L1036-L1071)
  - `strip_powershell_module_prefix()` (L1073-L1085)
  - `is_powershell_safe_script_block_consumer()` (L1087-L1108)
  - `powershell_new_object_type_outside_clm()` (L1110-L1124)
  - `powershell_new_object_type_from_args()` (L1126-L1156)
  - `split_powershell_args()` (L1158-L1227)
  - `parse_powershell_parameter()` (L1229-L1244)
  - `is_powershell_param_prefix()` (L1246-L1248)
  - `powershell_param_abbrev_matches()` (L1250-L1252)
  - `new_object_value_param_consumes_next()` (L1254-L1259)
  - `clean_powershell_arg()` (L1261-L1266)
  - `powershell_type_literal_outside_clm()` (L1268-L1327)
  - `read_bracketed_type_literal()` (L1329-L1347)
  - `normalize_powershell_type_name()` (L1349-L1363)
  - `next_non_ws()` (L1365-L1373)
  - `is_powershell_command_boundary()` (L1375-L1377)
  - `is_powershell_variable_or_subexpression_start()` (L1379-L1383)
  - `is_powershell_type_start()` (L1385-L1387)
  - `is_powershell_identifier_start()` (L1389-L1391)
  - `is_powershell_identifier_continue()` (L1393-L1395)

- **依赖**:
  - `regex::Regex`, `std::collections::HashSet`, `std::sync::LazyLock`
  - `dangerous::patterns` (调用 `is_dangerous_command`)

- **被依赖**:
  - `tests.rs`
  - 外部使用者: security.rs, classifier.rs

### 子模块 4: `tests.rs` (~566 行)

- **职责**: 所有单元测试
- **迁移内容**:
  - 整个 `#[cfg(test)] mod tests { ... }` 块 (L1428-L1993)
  - `test_permission_context()` 辅助函数 (L1576-L1590)

- **依赖**: `super::*` (所有子模块的公共 API)

## 额外选项：进一步拆分 powershell.rs

由于 `powershell.rs` 约有 580 行，仍超过 400 行目标，可进一步拆分为：

```
dangerous/
├── mod.rs
├── auto_mode.rs            (~270 行)
├── patterns.rs             (~320 行)
├── powershell/
│   ├── mod.rs              (~50 行)  — 模块声明 + 重导出
│   ├── patterns.rs         (~175 行) — POWERSHELL_DANGER_PATTERNS
│   ├── clm.rs              (~150 行) — CLM 类型白名单 + CLM 类型检测函数
│   └── parser.rs           (~200 行) — 语法解析 / AST 启发式辅助函数
└── tests.rs                (~566 行)
```

此方案为**推荐方案**，确保每个子文件 ≤ 400 行。

## 拆分后目录结构（推荐方案）

```
crates/cc-permissions/src/dangerous/
├── mod.rs                          (~80 行)  — 子模块声明 + 重导出
├── auto_mode.rs                    (~270 行) — Auto 模式权限管理
├── patterns.rs                     (~320 行) — Bash 危险命令模式 + 检测入口
├── powershell/
│   ├── mod.rs                      (~50 行)  — 子模块声明 + 重导出
│   ├── patterns.rs                 (~175 行) — PowerShell 危险命令 Regex 模式
│   ├── clm.rs                      (~150 行) — CLM 类型白名单 + New-Object / 类型字面量检测
│   └── parser.rs                   (~200 行) — 语法解析辅助函数
└── tests.rs                        (~566 行) — 所有单元测试
```

## mod.rs 设计

```rust
//! Dangerous command detection.
//!
//! Identifies shell commands that could cause irreversible damage to the system.
//! Returns a human-readable reason string when a dangerous pattern is detected.

mod auto_mode;
mod patterns;
mod powershell;
#[cfg(test)]
mod tests;

pub use auto_mode::{
    strip_dangerous_permissions_for_auto_mode,
    restore_dangerous_permissions_after_auto_mode,
    set_permission_mode_with_auto_mode_safety,
    strip_dangerous_permissions_for_active_auto_mode,
    restore_auto_mode_stripped_permissions,
    AutoModePermissionStrip,
    AutoModeRuntimeTransition,
};
pub use patterns::is_dangerous_command;
pub use powershell::is_dangerous_powershell_command;
```

## 迁移步骤

### 步骤 1: 创建目录结构

```bash
# 将 dangerous.rs 移为 dangerous/mod.rs
git mv crates/cc-permissions/src/dangerous.rs crates/cc-permissions/src/dangerous/mod.rs
mkdir -p crates/cc-permissions/src/dangerous/powershell
```

### 步骤 2: 提取 `auto_mode.rs`（安全 — 无外部依赖冲突）

将以下内容从 `mod.rs` 移动到 `dangerous/auto_mode.rs`:
- 所有 Auto Allow Pattern 常量 (L15-L50)
- 类型定义 (L52-L67)
- Auto 模式管理函数 (L75-L212)
- 私有辅助函数 (L214-L329)
- 对应的 import 语句

此步骤后 `mod.rs` 减少约 330 行，且可独立编译。

```rust
// auto_mode.rs 开头需要:
use cc_types::permissions::{
    PermissionMode, StrippedPermissionRule, ToolPermissionContext, ToolPermissionRulesBySource,
};
use std::collections::HashSet;
```

### 步骤 3: 提取 `patterns.rs`（Bash 检测 + 入口函数）

从 `mod.rs` 移动:
- `DangerPattern` 结构体与 `DANGER_PATTERNS` (L332-L432)
- `is_dangerous_command()` (L780-L812)
- `git_clean_forced_without_dry_run()` (L1397-L1422)
- 相关 import: `regex::Regex`, `cc_utils::bash::{contains_multiline_string, has_unterminated_quotes}`

此步骤后 `mod.rs` 减少约 340 行，继续可编译。

### 步骤 4: 创建 `powershell/` 子模块

#### 4a: `powershell/mod.rs`
提供模块层级重导出:
```rust
mod clm;
mod parser;
mod patterns;

pub use super::patterns::is_dangerous_command;
pub use is_dangerous_powershell_command;
```

#### 4b: `powershell/patterns.rs`
- `POWERSHELL_DANGER_PATTERNS` (L434-L609)
- `is_dangerous_powershell_command()` (L814-L852)

#### 4c: `powershell/clm.rs`
- `POWERSHELL_CLM_ALLOWED_TYPES` (L611-L760)
- `POWERSHELL_NEW_OBJECT_RE` (L762-L765)
- `powershell_new_object_type_outside_clm()` (L1110-L1124)
- `powershell_type_literal_outside_clm()` (L1268-L1327)
- `read_bracketed_type_literal()` (L1329-L1347)
- `normalize_powershell_type_name()` (L1349-L1363)

#### 4d: `powershell/parser.rs`
- `powershell_obvious_parse_error_reason()` (L854-L925)
- `powershell_ast_heuristic_reason()` (L927-L1034)
- `powershell_segment_command_before()` (L1036-L1071)
- `strip_powershell_module_prefix()` (L1073-L1085)
- `is_powershell_safe_script_block_consumer()` (L1087-L1108)
- `split_powershell_args()` (L1158-L1227)
- `parse_powershell_parameter()` (L1229-L1244)
- `is_powershell_param_prefix()` (L1246-L1248)
- `powershell_param_abbrev_matches()` (L1250-L1252)
- `new_object_value_param_consumes_next()` (L1254-L1259)
- `clean_powershell_arg()` (L1261-L1266)
- `next_non_ws()` (L1365-L1373)
- `is_powershell_command_boundary()` (L1375-L1377)
- `is_powershell_variable_or_subexpression_start()` (L1379-L1383)
- `is_powershell_type_start()` (L1385-L1387)
- `is_powershell_identifier_start()` (L1389-L1391)
- `is_powershell_identifier_continue()` (L1393-L1395)
- `powershell_new_object_type_from_args()` (L1126-L1156)

注意: `clm.rs` 和 `parser.rs` 之间有少量相互引用:
- `powershell_new_object_type_outside_clm()` (在 clm.rs) 调用:
  - `POWERSHELL_CLM_ALLOWED_TYPES` (在 clm.rs)
  - `powershell_new_object_type_from_args()` (在 parser.rs)
- `powershell_type_literal_outside_clm()` (在 clm.rs) 调用:
  - `read_bracketed_type_literal()` (在 clm.rs)
  - `normalize_powershell_type_name()` (在 clm.rs)

这没有问题 — clm.rs 可以 `use super::parser::...` 引用解析辅助函数。

依赖链: `patterns.rs` -> `parser.rs` -> `clm.rs`
`patterns.rs` 中的 `is_dangerous_powershell_command()` 调用:
- `is_dangerous_command()` (来自 `super::patterns`)
- `powershell_obvious_parse_error_reason()` (来自 `parser.rs`)
- `powershell_ast_heuristic_reason()` (来自 `parser.rs`)
- `powershell_new_object_type_outside_clm()` (来自 `clm.rs`)
- `powershell_type_literal_outside_clm()` (来自 `clm.rs`)

所以 `powershell/mod.rs` 做单层 re-export 即可。

### 步骤 5: 提取 `tests.rs`

将 `#[cfg(test)] mod tests { ... }` 块完整移动到 `tests.rs`。

注意: 如果测试只引用 `super::*`，则 `mod.rs` 需要在 `#[cfg(test)] mod tests;` 声明前已将各子模块 `pub use` 出来，否则测试需要按模块分开 `use`。

**推荐方案**: `tests.rs` 中保持 `use super::*;` 即可，因为 `mod.rs` 会将所有公共 API 重导出。私有函数的测试需要额外导入子模块。

部分测试引用了私有函数 `test_permission_context()` 和 `merge_transition()`。需在 `auto_mode.rs` 中将 `test_permission_context` 标记为 `#[cfg(test)] pub(crate)`，或在 `auto_mode.rs` 底部增加 `#[cfg(test)]` 模块导出该函数。

### 步骤 6: 清理 `mod.rs`

`mod.rs` 最终只包含:
- 模块文档注释
- 子模块声明 (`mod auto_mode; mod patterns; mod powershell; #[cfg(test)] mod tests;`)
- 重导出 (`pub use ...`)
- 约 80 行

### 步骤 7: 更新 `lib.rs`

取消对 `lib.rs` 的变更 — `pub mod dangerous;` 声明保持不变，因为 Rust 会自动将其解析到 `dangerous/` 目录。

### 步骤 8: 编译验证

```bash
cargo check -p cc-permissions
cargo test -p cc-permissions
cargo check -p cc-engine
cargo check -p cc-commands
cargo check -p cc-safety
cargo check -p web
cargo check -p start-up
cargo check -p cc-tools
```

## 风险与注意事项

### 1. 可见性问题 — 测试引用私有函数

当前测试 `tests` 模块（在同一个文件中）通过 `use super::*;` 访问私有函数。拆分后，测试在 `tests.rs` 中，只能引用 `super::*` 中 `pub use` 出来的东西。

**解决方案**: 在 `auto_mode.rs` 中将 `merge_transition()` 和 `test_permission_context()` 标记为 `pub(crate)` 或 `pub(super)` 加 `#[cfg(test)]` 门控。

`test_permission_context()` 需要在 `auto_mode.rs` 中保持 `#[cfg(test)] pub(crate) fn`。

### 2. PowerShell 子模块间的循环依赖风险

`powershell/patterns.rs` 中的 `is_dangerous_powershell_command()` 需要调用 `parser.rs` 和 `clm.rs` 的函数。这是单向依赖，不存在循环。

### 3. `LazyLock` 静态变量的初始化顺序

所有 `static` 变量使用 `LazyLock`，初始化顺序没有风险 — `LazyLock` 是惰性初始化的，在首次访问时才初始化。

### 4. `mod.rs` 中的 import 拆分

当前 `dangerous.rs` 顶部有 3 组 import:
```rust
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use cc_types::permissions::{...};
use cc_utils::bash::{contains_multiline_string, has_unterminated_quotes};
```

拆分后，各模块只需要自己的子集。`mod.rs` 可以删除所有 import 语句（仅保留模块声明和重导出）。

### 5. Git 历史追踪

使用 `git mv` 保留文件的历史追踪。`dangerous.rs` -> `dangerous/mod.rs` 的移动应在一次提交中完成，然后逐步提取子模块。

### 6. 外部使用者的导入路径不变

所有外部文件通过 `cc_permissions::dangerous::xxx` 引用，拆分后路径不变，因为 `dangerous/` 目录作为一个模块对外呈现，`mod.rs` 中的 `pub use` 确保了所有公共 API 在 `cc_permissions::dangerous::` 命名空间下可用。
