# 命令 E2E 测试计划：总览

> 创建时间：2026-05-24
> 状态：规划中
> 命令总数：68 个已注册，8 个已测试，60 个待添加

## 当前覆盖率

已在 `crates/claude-code-rs/tests/pty_tui_e2e/` 中测试的命令：
- `/help`、`/version`、`/cost`、`/status`（离线，`commands.rs`）
- `/clear`（在线，`conversation.rs`）
- `/model`（在线，`model_flow.rs` + `script.rs`）
- `/login`（在线，`model_flow.rs` + `script.rs`）
- `/permissions` 部分测试（在线，`permissions.rs` + `script.rs`）
- 命令面板 `/` 触发（离线，`commands.rs`）

## 计划文件

| # | 文件 | 命令 | 模式 | 优先级 |
|---|---|---|---|---|
| 01 | [核心与信息](command-e2e-test-plan-01-core-info.md) | `/exit`、`/config`、`/debug`、`/effort`、`/fast`、`/context`、`/files`、`/copy`、`/compact`、`/keybindings`、`/statusline`、`/terminal-setup`、`/doctor`、`/experimental`（14 个命令） | 离线 | 高 |
| 02 | [会话与上下文](command-e2e-test-plan-02-session-context.md) | `/session`、`/resume`、`/rename`、`/rewind`、`/insights`、`/branch`、`/export`、`/audit-export`、`/session-export`、`/add-dir`、`/init`（11 个命令） | 混合 | 高 |
| 03 | [认证与模型](command-e2e-test-plan-03-auth-model.md) | `/logout`、`/advisor`（2 个命令） | 混合 | 中 |
| 04 | [Git 与 Diff](command-e2e-test-plan-04-git.md) | `/diff`、`/commit`、`/branch`、`/gbranch`（4 个命令） | 混合 | 高 |
| 05 | [权限与沙箱](command-e2e-test-plan-05-permissions-sandbox.md) | `/sandbox`、`/permissions` 未测试子命令（2 个命令组） | 混合 | 高 |
| 06 | [MCP 与插件](command-e2e-test-plan-06-mcp-plugin.md) | `/mcp`、`/plugin`、`/reload-plugins`、`/ide`、`/lsp`、`/chrome`（6 个命令） | 离线 | 中 |
| 07 | [代理与团队](command-e2e-test-plan-07-agent-team.md) | `/agents`、`/team`、`/team-onboarding`、`/tasks`、`/btw`、`/simplify`、`/coordinator`、`/plan`（8 个命令） | 混合 | 中 |
| 08 | [KAIROS 与功能门控](command-e2e-test-plan-08-kairos-feature-gated.md) | `/sleep`、`/assistant`、`/daemon`、`/notify`、`/remote`、`/channels`、`/dream`（7 个命令） | 离线 | 中 |
| 09 | [记忆、技能与钩子](command-e2e-test-plan-09-memory-skills-hooks.md) | `/memory`、`/skills`、`/hooks`（3 个命令组） | 离线 | 高 |
| 10 | [查询与审查](command-e2e-test-plan-10-query-review.md) | `/review`、`/security-review`、`/recap`、`/loop`、`/schedule`（5 个命令） | 在线 | 低 |
| 11 | [别名与批量](command-e2e-test-plan-11-alias-batch.md) | 所有别名 + 边界情况 | 离线 | 高 |

## 实施顺序

### 第一阶段：离线冒烟测试（无需 API 密钥）
1. **计划 11** — 别名批量冒烟测试（尽早发现注册错误）
2. **计划 01** — 核心与信息命令（价值最高，完全离线）
3. **计划 09** — 记忆、技能与钩子（核心功能，离线）
4. **计划 05** — 权限与沙箱模式（安全关键）
5. **计划 06** — MCP 与插件（离线冒烟测试）

### 第二阶段：混合离线/在线
6. **计划 02** — 会话与上下文（先测试离线部分）
7. **计划 04** — Git 与 Diff（先测试离线 diff）
8. **计划 03** — 认证与模型（离线部分）
9. **计划 07** — 代理与团队（离线列表/状态测试）
10. **计划 08** — KAIROS 与功能门控（优雅降级）

### 第三阶段：在线测试（需要 API 密钥）
11. **计划 10** — 查询与审查（全部在线）
12. 计划 02-08 中剩余的在线测试

## 测试架构

所有测试使用现有的 PTY TUI E2E 框架：
- **模板引擎**（`script.rs`）：使用 `TestCase` + `TestStep` 进行声明式测试
- **测试工具**（`harness.rs`）：使用 `PtySession` 进行直接 PTY 控制
- **离线模式**：`--permission-mode bypass`，空 API 密钥
- **在线模式**：`#[ignore = "requires real API key"]`，读取 `~/.cc-rust/settings.json`

### 需要新增的 TestStep 变体（可选）

为支持上述部分测试模式，考虑在 `script.rs` 中添加：
- `AssertOutputContains(String)` — 检查累积文本输出（不仅仅是屏幕）
- `CleanupMemory(String)` — 测试后移除测试用记忆项

### 文件组织

每个计划对应 `crates/claude-code-rs/tests/pty_tui_e2e/` 中的一个新测试文件：
```
commands_core_info.rs      ← 计划 01
commands_session.rs        ← 计划 02
commands_auth.rs           ← 计划 03
commands_git.rs            ← 计划 04
commands_permissions.rs    ← 计划 05
commands_mcp_plugin.rs     ← 计划 06
commands_agent_team.rs     ← 计划 07
commands_kairos.rs         ← 计划 08
commands_memory_skills_hooks.rs ← 计划 09
commands_query.rs          ← 计划 10
commands_aliases.rs        ← 计划 11
```

每个新文件需要在 `main.rs` 中声明对应的 `mod`。

## 预估测试数量

| 计划 | 离线测试 | 在线测试 | 合计 |
|------|----------|----------|------|
| 01 | 22 | 0 | 22 |
| 02 | 13 | 4 | 17 |
| 03 | 1 | 0 | 1 |
| 04 | 5 | 3 | 8 |
| 05 | 14 | 0 | 14 |
| 06 | 18 | 0 | 18 |
| 07 | 14 | 2 | 16 |
| 08 | 18 | 0 | 18 |
| 09 | 16 | 0 | 16 |
| 10 | 2 | 3 | 5 |
| 11 | 7 | 0 | 7 |
| **合计** | **130** | **12** | **142** |

## 关键设计决策

1. **先确保不崩溃**：每个命令至少有一个 `AssertNoPanic` 测试
2. **其次验证输出内容**：在可能的情况下，断言特定的输出文本
3. **最后测试副作用**：修改状态的测试（配置、记忆、权限）使用会话级作用域的变更
4. **别名测试成本低**：批量执行以最大化每个测试的覆盖率
5. **功能门控优雅降级**：KAIROS 命令即使在门控关闭时也不应崩溃
