# Development Docs Index

> 最后更新: 2026-06-01

本文档是 `development/` 目录的统一索引。所有文档按主题分类，便于快速定位。

---

## 目录结构

```
development/
├── README.md        ← 本文件：统一索引
├── reference/       ← 活跃的技术参考资料、架构文档、项目对比
└── archive/         ← 历史文档、已完成计划、调试笔记、UI 分析
    ├── plan/            ← 活跃/进行中的计划
    ├── planz/           ← 近期优化计划
    ├── old_archive/     ← 已归档的旧文档（冻结记录）
    ├── utils/           ← 功能模块参考文档
    ├── ui/              ← UI 对比、分析和设计文档
    ├── testing/         ← 测试指南和策略
    ├── debug/           ← 调试日志和修复记录
    ├── cache/           ← 缓存实现相关
    ├── claude-code-configuration/ ← Claude Code 配置说明
    ├── code-split/      ← 代码拆分计划
    ├── mvp-optimization-plans/ ← MVP 阶段优化
    ├── agent-handoff/   ← Agent 交接文档
    ├── scripts/         ← OMX / subagent 任务脚本
    ├── delete/          ← 已删除代码记录
    ├── http/            ← HTTP / User-Agent 相关
    ├── web/             ← Web 相关计划
    └── old_archive/     ← 已归档的旧文档（冻结记录）
```

---

## 一、技术参考 — `reference/`

活跃的架构文档、集成指南和跨项目对比。

### 架构与集成

| 文档 | 说明 |
|------|------|
| [bootstrap-ipc-daemon-relationship.md](reference/bootstrap-ipc-daemon-relationship.md) | Bootstrap → IPC → Daemon 三层关系与启动流程 |
| [webui-adapters-and-cc-ipc-structure.md](reference/webui-adapters-and-cc-ipc-structure.md) | WebUI 适配器与 IPC 结构 |
| [claude-code-bun-entrypoints-ui-structure.md](reference/claude-code-bun-entrypoints-ui-structure.md) | Bun 入口点与 UI 结构 |
| [headless-ipc-protocol.md](reference/headless-ipc-protocol.md) | Headless IPC 协议定义 |
| [rust-vs-claude-code-bun-communication-message-mapping.md](reference/rust-vs-claude-code-bun-communication-message-mapping.md) | Rust vs Bun 消息映射 |
| [STORAGE.md](reference/STORAGE.md) | 存储路径与数据布局 |

### Daemon 与 Remote Control

| 文档 | 说明 |
|------|------|
| [DAEMON_OPERATIONS.md](reference/DAEMON_OPERATIONS.md) | Daemon 运维操作指南 |
| [REMOTE_CONTROL_GATEWAY.md](reference/REMOTE_CONTROL_GATEWAY.md) | Remote Control Gateway 架构 |
| [remote-control-current-state.md](reference/remote-control-current-state.md) | Remote Control 当前状态 |
| [agent-extension-installation.md](reference/agent-extension-installation.md) | Agent 扩展安装 |

### Provider 配置

| 文档 | 说明 |
|------|------|
| [cloud-providers.md](reference/cloud-providers.md) | AWS Bedrock / GCP Vertex AI 配置指南 |
| [codex-backend.md](reference/codex-backend.md) | Codex 后端配置参考 |

### Crate 迁移

| 文档 | 说明 |
|------|------|
| [CRATE_MIGRATION_GUIDE.md](reference/CRATE_MIGRATION_GUIDE.md) | Crate 迁移总指南 |
| [CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md](reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md) | Phase 0 所有权守卫矩阵 |
| [CRATE_MIGRATION_TARGET_STATE.md](reference/CRATE_MIGRATION_TARGET_STATE.md) | Crate 迁移目标状态 |
| [CRATE_DEPENDENCY_TARGETS.md](reference/CRATE_DEPENDENCY_TARGETS.md) | Crate 依赖目标 |

### MCP / Browser / Computer Use

| 文档 | 说明 |
|------|------|
| [browser-mcp-config.md](reference/browser-mcp-config.md) | Browser MCP 配置 |
| [chrome-native-host.md](reference/chrome-native-host.md) | Chrome Native Host 配置 |
| [computer-use-mcp-config.md](reference/computer-use-mcp-config.md) | Computer Use MCP 配置 |

### 分析报告

| 文档 | 说明 |
|------|------|
| [Codex_SDK_Features.md](reference/Codex_SDK_Features.md) | Codex SDK 功能对比 |
| [tool-comparison-3projects.md](reference/tool-comparison-3projects.md) | 三个项目的工具对比 |
| [teammem-analysis.md](reference/teammem-analysis.md) | Team Memory 分析 |
| [snapshot-testing-guide.md](reference/snapshot-testing-guide.md) | Snapshot 测试指南 |
| [anthropic_coding.md](reference/anthropic_coding.md) | Anthropic Compatible Coding 参考 |

### Anthropic 参考

| 文档 | 说明 |
|------|------|
| [api/overview.md](reference/anthropic/api/overview.md) | Anthropic API 总览 |
| [prompt_cache/overview.md](reference/anthropic/prompt_cache/overview.md) | Prompt Cache 参考 |
| [changlog.md](reference/anthropic/changlog.md) | Anthropic 变更日志 |
| [what's_changed.md](reference/anthropic/what's_changed.md) | Anthropic 变更说明 |

---

## 二、活跃计划 — `archive/plan/` & `archive/planz/`

当前进行中的执行计划和路线图。

### 引擎与架构

| 文档 | 说明 |
|------|------|
| [allthecodes-engine-optimization-plan-2026-05-29.md](archive/plan/allthecodes-engine-optimization-plan-2026-05-29.md) | 引擎优化：迁移遗留、warnings、stubs 收束 |
| [allthecodes-rename-migration-plan-2026-05-24.md](archive/plan/allthecodes-rename-migration-plan-2026-05-24.md) | 全量改名迁移（Claude Code → allthecodes） |
| [cc-daemon-migration-plan-2026-05-13.md](archive/plan/cc-daemon-migration-plan-2026-05-13.md) | cc-daemon 迁移执行 |
| [root-src-library-migration-plan-2026-05-15.md](archive/plan/root-src-library-migration-plan-2026-05-15.md) | Root crate 源码迁移到 workspace library |
| [ipc-refactor-plan.md](archive/plan/ipc-refactor-plan.md) | IPC 重构计划 |
| [workspace-decycle-plan-2026-05-14.md](archive/plan/workspace-decycle-plan-2026-05-14.md) | Workspace 循环依赖消除 |

### 测试

| 文档 | 说明 |
|------|------|
| [command-e2e-test-plan-00-overview.md](archive/plan/command-e2e-test-plan-00-overview.md) | E2E 测试总览：68 命令覆盖计划 |
| [command-e2e-test-plan-01-core-info.md](archive/plan/command-e2e-test-plan-01-core-info.md) | E2E：核心信息命令 |
| [command-e2e-test-plan-02-session-context.md](archive/plan/command-e2e-test-plan-02-session-context.md) | E2E：会话与上下文 |
| [command-e2e-test-plan-03-auth-model.md](archive/plan/command-e2e-test-plan-03-auth-model.md) | E2E：认证与模型 |
| [command-e2e-test-plan-04-git.md](archive/plan/command-e2e-test-plan-04-git.md) | E2E：Git 命令 |
| [command-e2e-test-plan-05-permissions-sandbox.md](archive/plan/command-e2e-test-plan-05-permissions-sandbox.md) | E2E：权限与沙箱 |
| [command-e2e-test-plan-06-mcp-plugin.md](archive/plan/command-e2e-test-plan-06-mcp-plugin.md) | E2E：MCP 与插件 |
| [command-e2e-test-plan-07-agent-team.md](archive/plan/command-e2e-test-plan-07-agent-team.md) | E2E：Agent 与团队 |
| [command-e2e-test-plan-08-kairos-feature-gated.md](archive/plan/command-e2e-test-plan-08-kairos-feature-gated.md) | E2E：Kairos 特性门控 |
| [command-e2e-test-plan-09-memory-skills-hooks.md](archive/plan/command-e2e-test-plan-09-memory-skills-hooks.md) | E2E：Memory、Skills、Hooks |
| [command-e2e-test-plan-10-query-review.md](archive/plan/command-e2e-test-plan-10-query-review.md) | E2E：Query 与 Review |
| [command-e2e-test-plan-11-alias-batch.md](archive/plan/command-e2e-test-plan-11-alias-batch.md) | E2E：别名与批处理 |
| [real-memoryfile-skill-mcp-plugin-lsp-agent-teams-e2e-plan-2026-05-24.md](archive/plan/real-memoryfile-skill-mcp-plugin-lsp-agent-teams-e2e-plan-2026-05-24.md) | Memory/Skill/MCP/Plugin/LSP/Teams E2E 验证 |
| [2026-05-17-mcp-skill-plugin-real-project-test-plan.md](archive/plan/2026-05-17-mcp-skill-plugin-real-project-test-plan.md) | MCP/Skill/Plugin 真实项目测试 |

### UI 与 Ratatui

| 文档 | 说明 |
|------|------|
| [ratatui-ui-parity-omx-execution-plan-2026-05-08.md](archive/plan/ratatui-ui-parity-omx-execution-plan-2026-05-08.md) | Ratatui UI Parity OMX 执行 |
| [ratatui-ui-parity-untracked-gap-plan-2026-05-08.md](archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md) | Ratatui 未跟踪缺口补齐 |
| [generic-selectable-command-surface-plan-2026-05-08.md](archive/plan/generic-selectable-command-surface-plan-2026-05-08.md) | 可复用 SelectableList 组件 |
| [plugin-ui-port-to-rust-plan-2026-05-08.md](archive/plan/plugin-ui-port-to-rust-plan-2026-05-08.md) | 插件 UI 移植到 Rust |

### Remote Control

| 文档 | 说明 |
|------|------|
| [remote-control-gateway-execution-plan-2026-05-08.md](archive/plan/remote-control-gateway-execution-plan-2026-05-08.md) | Remote Control Gateway 执行 |
| [remote-control-gateway-omx-execution-plan-2026-05-08.md](archive/plan/remote-control-gateway-omx-execution-plan-2026-05-08.md) | Remote Control Gateway OMX 执行 |
| [remote-channel-phase1-telegram-lark-plan-2026-05-08.md](archive/plan/remote-channel-phase1-telegram-lark-plan-2026-05-08.md) | Telegram/Lark 通道 Phase 1 |

### 安全与防御

| 文档 | 说明 |
|------|------|
| [p1-defensive-fail-fast-execution-plan-2026-05-07.md](archive/plan/p1-defensive-fail-fast-execution-plan-2026-05-07.md) | P1 防御性代码执行 |
| [cfg-test-production-wiring-plan-2026-05-21.md](archive/plan/cfg-test-production-wiring-plan-2026-05-21.md) | `#[cfg(test)]` 审计与移除 |
| [anthropic-api-coding-compat-risk-plan-2026-05-17.md](archive/plan/anthropic-api-coding-compat-risk-plan-2026-05-17.md) | Anthropic API 兼容性风险 |

### 其他活跃计划

| 文档 | 说明 |
|------|------|
| [bun-docs-documentation-plan.md](archive/plan/bun-docs-documentation-plan.md) | Bun 文档计划 |
| [computer-use-implementation-checklist.md](archive/plan/computer-use-implementation-checklist.md) | Computer Use 实现清单 |
| [daemon-usability-plan.md](archive/plan/daemon-usability-plan.md) | Daemon 可用性改进 |
| [traceable-logging-plan.md](archive/plan/traceable-logging-plan.md) | 可追踪日志 |
| [ui-test-target-dead-code-subagent-plan-2026-05-20.md](archive/plan/ui-test-target-dead-code-subagent-plan-2026-05-20.md) | UI 测试目标与死代码清理 |
| [utils-full-build-parallel-development-plan-2026-05-19.md](archive/plan/utils-full-build-parallel-development-plan-2026-05-19.md) | Utils 全量构建并行开发 |
| [workspace-all-targets-warning-budget-plan-2026-05-20.md](archive/plan/workspace-all-targets-warning-budget-plan-2026-05-20.md) | Workspace warning 预算管理 |

### 近期优化 — `archive/planz/`

| 文档 | 说明 |
|------|------|
| [cohesive-optimization-plan-2026-05-30.json](archive/planz/cohesive-optimization-plan-2026-05-30.json) | 凝聚性优化计划（JSON） |

---

## 三、已完成计划（已归档）— `archive/old_archive/plan/`

已实现或评估完成的计划，移至此处冻结。

| 文档 | 说明 |
|------|------|
| [bash-shell-parity-migration-plan-2026-05-18.md](archive/old_archive/plan/bash-shell-parity-migration-plan-2026-05-18.md) | Bash Shell parity 差距评估 |
| [cc-engine-migration-plan-2026-05-13.md](archive/old_archive/plan/cc-engine-migration-plan-2026-05-13.md) | cc-engine 迁移（主要完成） |
| [claude-code-bun-gap-plan.md](archive/old_archive/plan/claude-code-bun-gap-plan.md) | Claude Code Bun 差距评估 |
| [command-settings-ui-coverage-audit-2026-05-08.md](archive/old_archive/plan/command-settings-ui-coverage-audit-2026-05-08.md) | 命令 UI 覆盖审计 |
| [comprehensive-gap-analysis-2026-05-31.md](archive/old_archive/plan/comprehensive-gap-analysis-2026-05-31.md) | 全面差距分析报告 |
| [core-utilities-migration-plan.md](archive/old_archive/plan/core-utilities-migration-plan.md) | Core utilities 迁移策略 |
| [crate-migration-phase-0-inventory-2026-05-14.md](archive/old_archive/plan/crate-migration-phase-0-inventory-2026-05-14.md) | Crate 迁移 Phase 0 基线清单 |
| [crate-migration-phase-plan-2026-05-14.md](archive/old_archive/plan/crate-migration-phase-plan-2026-05-14.md) | Crate 迁移 Phase 0-12 执行 |
| [gap-analysis-vs-claude-code-2026-03-27-2026-05-29.md](archive/old_archive/plan/gap-analysis-vs-claude-code-2026-03-27-2026-05-29.md) | 与上游 Claude Code 差距分析 |
| [langfuse-improvement-code-review-2026-06-01.md](archive/old_archive/plan/langfuse-improvement-code-review-2026-06-01.md) | Langfuse 改进代码审查 |
| [missing-tools-adaptation-plan-2026-05-28.md](archive/old_archive/plan/missing-tools-adaptation-plan-2026-05-28.md) | 缺失工具适配快照 |
| [phase5-missing-tools-optimization-plan-2026-05-30.md](archive/old_archive/plan/phase5-missing-tools-optimization-plan-2026-05-30.md) | Phase 5 工具优化（首版已实现） |
| [remote-control-gateway-implementation-report-2026-05-08.md](archive/old_archive/plan/remote-control-gateway-implementation-report-2026-05-08.md) | Remote Control Gateway 实施报告 |
| [session-export-implementation-guide.md](archive/old_archive/plan/session-export-implementation-guide.md) | Session 导出实现参考 |

---

## 四、功能模块参考 — `archive/utils/`

各功能模块的详细设计文档和参考。

| 文档 | 说明 |
|------|------|
| [overview.md](archive/utils/overview.md) | Utils 模块总览 |
| [bash-shell.md](archive/utils/bash-shell.md) | Bash Shell 集成 |
| [core-utilities.md](archive/utils/core-utilities.md) | 核心工具函数 |
| [computer-use.md](archive/utils/computer-use.md) | Computer Use 集成 |
| [hooks.md](archive/utils/hooks.md) | Hook 系统 |
| [misc.md](archive/utils/misc.md) | 杂项工具 |
| [permissions-classifier.md](archive/utils/permissions-classifier.md) | 权限分类器 |
| [plugins-marketplace.md](archive/utils/plugins-marketplace.md) | 插件市场 |
| [settings-mdm.md](archive/utils/settings-mdm.md) | 设置 MDM |
| [suggestions-input.md](archive/utils/suggestions-input.md) | 输入建议 |
| [teams-swarm.md](archive/utils/teams-swarm.md) | 团队与 Swarm |
| [telemetry-observability.md](archive/utils/telemetry-observability.md) | 遥测与可观测性 |
| [tui-panel-output-classification.md](archive/utils/tui-panel-output-classification.md) | TUI 面板输出分类 |

---

## 五、UI 分析与设计 — `archive/ui/`

UI 对比分析、组件设计、渲染子系统评估。

### UI 基础结构

| 文档 | 说明 |
|------|------|
| [claude-code-bun-ui-structure.md](archive/ui/claude-code-bun-ui-structure.md) | Bun 版 UI 结构 |
| [claude-code-rs-ui-structure.md](archive/ui/claude-code-rs-ui-structure.md) | Rust 版 UI 结构 |
| [codex-rs-tui-structure.md](archive/ui/codex-rs-tui-structure.md) | Codex Rust TUI 结构 |
| [truncation-summary.md](archive/ui/truncation-summary.md) | 文本截断策略总结 |

### UI Great 对比分析

| 文档 | 说明 |
|------|------|
| [README.md](archive/ui/great/README.md) | 对比分析总目录 |
| [01_message_rendering_comparison.md](archive/ui/great/01_message_rendering_comparison.md) | 消息渲染对比 |
| [02_component_parity_comparison.md](archive/ui/great/02_component_parity_comparison.md) | 组件 parity 对比 |
| [03_rendering_subsystem_comparison.md](archive/ui/great/03_rendering_subsystem_comparison.md) | 渲染子系统对比 |
| [04_permission_input_comparison.md](archive/ui/great/04_permission_input_comparison.md) | 权限输入对比 |
| [05_app_shell_comparison.md](archive/ui/great/05_app_shell_comparison.md) | App 外壳对比 |
| [06_missing_features_summary.md](archive/ui/great/06_missing_features_summary.md) | 缺失功能总结 |
| [ui-parity-update-plan.md](archive/ui/ui-parity-update-plan.md) | UI Parity 差异报告与更新计划（2026-05-01） |

#### Great UI 实施计划

| 文档 | 说明 |
|------|------|
| [plan-01-message-rendering.md](archive/ui/great/plans/plan-01-message-rendering.md) | 消息渲染计划 |
| [plan-02-design-system.md](archive/ui/great/plans/plan-02-design-system.md) | 设计系统计划 |
| [plan-03-input-editor.md](archive/ui/great/plans/plan-03-input-editor.md) | 输入编辑器计划 |
| [plan-04-notification.md](archive/ui/great/plans/plan-04-notification.md) | 通知系统计划 |
| [plan-05-agent-navigation.md](archive/ui/great/plans/plan-05-agent-navigation.md) | Agent 导航计划 |
| [plan-06-syntax-highlighting.md](archive/ui/great/plans/plan-06-syntax-highlighting.md) | 语法高亮计划 |
| [plan-07-permissions-wiring.md](archive/ui/great/plans/plan-07-permissions-wiring.md) | 权限连线计划 |
| [plan-08-dead-code-cleanup.md](archive/ui/great/plans/plan-08-dead-code-cleanup.md) | 死代码清理计划 |

### Better View UI

| 文档 | 说明 |
|------|------|
| [README.md](archive/ui/better-view/README.md) | Better View UI 总目录 |
| [approval-panels.md](archive/ui/better-view/approval-panels.md) | 审批面板设计 |
| [external-flows.md](archive/ui/better-view/external-flows.md) | 外部流程集成 |
| [selector-surfaces.md](archive/ui/better-view/selector-surfaces.md) | 选择器界面 |
| [settings-panels.md](archive/ui/better-view/settings-panels.md) | 设置面板 |
| [wizard-flows.md](archive/ui/better-view/wizard-flows.md) | 向导流程 |

### Command Surfaces

| 文档 | 说明 |
|------|------|
| [README.md](archive/ui/commands/README.md) | 命令界面总目录 |
| [approval-snapshots.md](archive/ui/commands/approval-snapshots.md) | 审批快照 |
| [command-surfaces.md](archive/ui/commands/command-surfaces.md) | 命令界面 |
| [text-and-external.md](archive/ui/commands/text-and-external.md) | 文本与外部集成 |

### Show & Tell

| 文档 | 说明 |
|------|------|
| [tui-display-issues.md](archive/ui/show-off/tui-display-issues.md) | TUI 展示问题 |
| [data-type-visual-distinction.md](archive/ui/show-off/data-type-visual-distinction.md) | 数据类型视觉区分总览 |
| [01-core-data-types.md](archive/ui/show-off/data-type-visual-distinction/01-core-data-types.md) | 核心数据类型 |
| [02-six-layer-distinction.md](archive/ui/show-off/data-type-visual-distinction/02-six-layer-distinction.md) | 六层区分 |
| [03-theme-system.md](archive/ui/show-off/data-type-visual-distinction/03-theme-system.md) | 主题系统 |
| [04-aggregation-optimizations.md](archive/ui/show-off/data-type-visual-distinction/04-aggregation-optimizations.md) | 聚合优化 |
| [05-rendering-pipeline.md](archive/ui/show-off/data-type-visual-distinction/05-rendering-pipeline.md) | 渲染管线 |
| [06-key-files-index.md](archive/ui/show-off/data-type-visual-distinction/06-key-files-index.md) | 关键文件索引 |
| [07-distinction-strategy-summary.md](archive/ui/show-off/data-type-visual-distinction/07-distinction-strategy-summary.md) | 区分策略总结 |

---

## 六、Claude Code 配置 — `archive/claude-code-configuration/`

Claude Code CLI 的配置说明文档。

| 文档 | 说明 |
|------|------|
| [settings.md](archive/claude-code-configuration/settings.md) | 设置总览 |
| [model-configuration.md](archive/claude-code-configuration/model-configuration.md) | 模型配置 |
| [permissions.md](archive/claude-code-configuration/permissions.md) | 权限配置 |
| [sandboxing.md](archive/claude-code-configuration/sandboxing.md) | 沙箱配置 |
| [terminal-configuration.md](archive/claude-code-configuration/terminal-configuration.md) | 终端配置 |
| [customize-keyboard-shortcuts.md](archive/claude-code-configuration/customize-keyboard-shortcuts.md) | 键盘快捷键定制 |
| [customize-status-line.md](archive/claude-code-configuration/customize-status-line.md) | 状态行定制 |
| [output-styles.md](archive/claude-code-configuration/output-styles.md) | 输出样式 |
| [fullscreen-rendering.md](archive/claude-code-configuration/fullscreen-rendering.md) | 全屏渲染 |
| [speed-up-responses-with-fast-mode.md](archive/claude-code-configuration/speed-up-responses-with-fast-mode.md) | Fast Mode |
| [voice-dictation.md](archive/claude-code-configuration/voice-dictation.md) | 语音听写 |

---

## 七、测试 — `archive/testing/`

| 文档 | 说明 |
|------|------|
| [docker-test-guide.md](archive/testing/docker-test-guide.md) | Docker 测试指南 |
| [rust-e2e-test.md](archive/testing/rust-e2e-test.md) | Rust E2E 测试 |
| [pty-e2e-test.md](archive/testing/pty-e2e-test.md) | PTY E2E 测试 |
| [pty-test-language-comparison.md](archive/testing/pty-test-language-comparison.md) | PTY 测试语言对比 |
| [e2e_list.md](archive/testing/e2e_list.md) | E2E 测试列表 |

---

## 八、调试与修复 — `archive/debug/`

| 文档 | 说明 |
|------|------|
| [crate-migration-allow-audit-2026-05-16.md](archive/debug/crate-migration-allow-audit-2026-05-16.md) | Crate 迁移 allow 审计 |
| [logging-architecture.md](archive/debug/logging-architecture.md) | 日志架构 |
| [model-default-locations.md](archive/debug/model-default-locations.md) | 模型默认位置 |
| [pty-command-surface-mcp-actions-2026-05-29.md](archive/debug/pty-command-surface-mcp-actions-2026-05-29.md) | PTY Command Surface MCP Actions 调试 |
| [pty_tui_e2e-log-review-2026-05-29.md](archive/debug/pty_tui_e2e-log-review-2026-05-29.md) | PTY TUI E2E 日志审查 |
| [tui-command-init-status-fix-2026-05-22.md](archive/debug/tui-command-init-status-fix-2026-05-22.md) | TUI 命令初始化状态修复 |
| [tui-command-model-deepseek-fix-2026-05-23.md](archive/debug/tui-command-model-deepseek-fix-2026-05-23.md) | TUI 命令模型 DeepSeek 修复 |
| [utils-full-build-review-issues-2026-05-19.md](archive/debug/utils-full-build-review-issues-2026-05-19.md) | Utils 全量构建审查问题 |
| [dead-code-audit.md](archive/debug/dead-code-audit.md) | 死代码审计报告（2026-04-12） |
| [JSON_USAGE_ANALYSIS.md](archive/debug/JSON_USAGE_ANALYSIS.md) | JSON 使用分析 |
| [claude-code-rs-refactor-audit-2026-05-07.md](archive/debug/claude-code-rs-refactor-audit-2026-05-07.md) | 重构审计（屎山评估） |
| [workspace-split-measurements.md](archive/debug/workspace-split-measurements.md) | Workspace 拆分构建耗时测量 |

---

## 九、代码拆分 — `archive/code-split/`

| 文档 | 说明 |
|------|------|
| [api-tests-split-plan.md](archive/code-split/api-tests-split-plan.md) | API 测试拆分 |
| [dangerous-split-plan.md](archive/code-split/dangerous-split-plan.md) | 危险/权限模块拆分 |
| [engine-loop-tests-split-plan.md](archive/code-split/engine-loop-tests-split-plan.md) | Engine 循环测试拆分 |
| [main-rs-split-plan.md](archive/code-split/main-rs-split-plan.md) | main.rs 拆分 |
| [product-mod-split-plan.md](archive/code-split/product-mod-split-plan.md) | product 模块拆分 |
| [query-loop-tests-split-plan.md](archive/code-split/query-loop-tests-split-plan.md) | Query 循环测试拆分 |

---

## 十、缓存与 Langfuse — `archive/cache/`

| 文档 | 说明 |
|------|------|
| [langfuse-ccb.md](archive/cache/langfuse-ccb.md) | Langfuse × CCB |
| [langfuse-improvement-plan.md](archive/cache/langfuse-improvement-plan.md) | Langfuse 改进计划 |
| [langfuse-runtime-status.md](archive/cache/langfuse-runtime-status.md) | Langfuse 运行时状态 |
| [prompt-cache-implementation.md](archive/cache/prompt-cache-implementation.md) | Prompt Cache 实现 |

---

## 十一、MVP 优化 — `archive/mvp-optimization-plans/`

| 文档 | 说明 |
|------|------|
| [MVP-001-api-providers-plan.md](archive/mvp-optimization-plans/MVP-001-api-providers-plan.md) | API 提供商优化 |
| [MVP-007-tool-search-ranking-plan.md](archive/mvp-optimization-plans/MVP-007-tool-search-ranking-plan.md) | Tool Search 排序 |
| [MVP-010-skill-system-package-plan.md](archive/mvp-optimization-plans/MVP-010-skill-system-package-plan.md) | Skill 系统包管理 |
| [mvp-compromise-memory.md](archive/mvp-optimization-plans/mvp-compromise-memory.md) | MVP 阶段 memory 妥协方案 |

---

## 十二、核心参考文档 — `archive/` 顶层

| 文档 | 说明 |
|------|------|
| [USAGE_GUIDE.md](archive/USAGE_GUIDE.md) | 使用指南 |
| [CLI_REFERENCE.md](archive/CLI_REFERENCE.md) | CLI 参考 |
| [COMMAND_REFERENCE.md](archive/COMMAND_REFERENCE.md) | 命令参考 |
| [COMMAND_UI_REFERENCE.md](archive/COMMAND_UI_REFERENCE.md) | 命令 UI 参考 |
| [IMPLEMENTATION_GAPS.md](archive/IMPLEMENTATION_GAPS.md) | 实现缺口记录 |
| [KNOWN_ISSUES.md](archive/KNOWN_ISSUES.md) | 已知问题 |
| [UNUSED_CODE_REPORT.md](archive/UNUSED_CODE_REPORT.md) | 未使用代码报告 |
| [RATATUI_UI_PARITY.md](archive/RATATUI_UI_PARITY.md) | Ratatui UI Parity 状态 |
| [FINAL_RELEASE_PLAN.md](archive/FINAL_RELEASE_PLAN.md) | 最终发布计划 |

---

## 十三、其他归档目录

### Agent Handoff

| 文档 | 说明 |
|------|------|
| [core-utilities-agent-brief.md](archive/agent-handoff/core-utilities-agent-brief.md) | Core Utilities Agent 交接摘要 |

### Deleted Code

| 文档 | 说明 |
|------|------|
| [phase2-deleted-code.md](archive/delete/phase2-deleted-code.md) | Phase 2 已删除代码 |
| [ui-core-item-allow-2026-05-20.md](archive/delete/ui-core-item-allow-2026-05-20.md) | UI core item allow 记录 |
| [ui-file-level-allow-2026-05-20.md](archive/delete/ui-file-level-allow-2026-05-20.md) | UI file-level allow 记录 |
| [ui-item-allow-2026-05-20.md](archive/delete/ui-item-allow-2026-05-20.md) | UI item allow 记录 |
| [ui-warning-cleanup-agents-theme.md](archive/delete/ui-warning-cleanup-agents-theme.md) | UI warning 清理 |

### HTTP / User Agent

| 文档 | 说明 |
|------|------|
| [user-agent-and-custom-agents-gap.md](archive/http/user-agent-and-custom-agents-gap.md) | User Agent 与 Custom Agents 缺口 |

### Scripts

| 文档 | 说明 |
|------|------|
| [README.md](archive/scripts/README.md) | Scripts 目录说明 |
| [achieve/](archive/scripts/achieve/) | OMX 执行脚本（P1、Ratatui、Remote Control、Workspace） |
| [workspace-crate-extraction-omx-tasks-2026-05-10.txt](archive/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt) | Workspace crate 提取 OMX 任务 |
| [better-view-ui-panels-omx-tasks-2026-05-11.txt](archive/scripts/better-view-ui-panels-omx-tasks-2026-05-11.txt) | Better View UI OMX 任务 |
| [non-workspace-unfinished-standard-task-2026-05-11.md](archive/scripts/non-workspace-unfinished-standard-task-2026-05-11.md) | 非 workspace 未完成任务 |

### Web

| 文档 | 说明 |
|------|------|
| [nextjs-react-xterm-web-plan-2026-05-27.md](archive/web/nextjs-react-xterm-web-plan-2026-05-27.md) | Next.js + React + xterm Web 计划 |

---

## 十四、旧归档 — `archive/old_archive/`

冻结的历史文档，不再活跃更新。

### 实施状态总结

| 文档 | 说明 |
|------|------|
| [COMPLETED_FULL.md](archive/old_archive/COMPLETED_FULL.md) | 已完成模块 — 完整实现 |
| [COMPLETED_SIMPLIFIED.md](archive/old_archive/COMPLETED_SIMPLIFIED.md) | 已完成模块 — 大幅简化实现 |
| [MODULE_SIMPLIFICATION.md](archive/old_archive/MODULE_SIMPLIFICATION.md) | 模块简化率分析 |
| [MIGRATION_PLAN.md](archive/old_archive/MIGRATION_PLAN.md) | 迁移计划 |
| [TECH_DEBT.md](archive/old_archive/TECH_DEBT.md) | 技术债务 |
| [P1_EXECUTION_PLAN.md](archive/old_archive/P1_EXECUTION_PLAN.md) | P1 执行计划 |
| [PYTHON_SDK_PLAN.md](archive/old_archive/PYTHON_SDK_PLAN.md) | Python SDK 计划 |
| [PROMPT_MIGRATION_GUIDE.md](archive/old_archive/PROMPT_MIGRATION_GUIDE.md) | Prompt 迁移指南 |
| [sdk-work-tracker.md](archive/old_archive/sdk-work-tracker.md) | SDK 工作跟踪 |
| [REWRITE_PLAN.md](archive/old_archive/REWRITE_PLAN.md) | 重写计划与 Phase 状态总览 |
| [cc-rust-overview.md](archive/old_archive/cc-rust-overview.md) | cc-rust Lite 项目总览与架构指南 |
| [completed-gap-closures-2026-05-07.md](archive/old_archive/completed-gap-closures-2026-05-07.md) | 已完成的缺口关闭 |
| [resolved-known-issues-2026-05-07.md](archive/old_archive/resolved-known-issues-2026-05-07.md) | 已解决的已知问题 |
| [resolved-model-context-2026-05-07.md](archive/old_archive/resolved-model-context-2026-05-07.md) | 已解决的模型上下文问题 |

### 状态机与规范

| 文档 | 说明 |
|------|------|
| [LIFECYCLE_STATE_MACHINE.md](archive/old_archive/LIFECYCLE_STATE_MACHINE.md) | 生命周期状态机 |
| [TOOL_EXECUTION_STATE_MACHINE.md](archive/old_archive/TOOL_EXECUTION_STATE_MACHINE.md) | 工具执行状态机 |
| [COMPACTION_RETRY_STATE_MACHINE.md](archive/old_archive/COMPACTION_RETRY_STATE_MACHINE.md) | 压缩重试状态机 |
| [QUERY_ENGINE_SESSION_LIFECYCLE.md](archive/old_archive/QUERY_ENGINE_SESSION_LIFECYCLE.md) | Query Engine 会话生命周期 |
| [AGENT_TEAMS_SPEC.md](archive/old_archive/AGENT_TEAMS_SPEC.md) | Agent 团队规范 |
| [STRUCTURE_DIFF.md](archive/old_archive/STRUCTURE_DIFF.md) | 结构差异 |

### 历史 Context Phase 文档

| 文档 | 说明 |
|------|------|
| [context-phase0-decisions-2026-05-06.md](archive/old_archive/context-phase0-decisions-2026-05-06.md) | Context Phase 0 决策 |
| [context-phase1-token-count-provider-matrix-2026-05-06.md](archive/old_archive/context-phase1-token-count-provider-matrix-2026-05-06.md) | Token 计数提供者矩阵 |
| [context-phase2-memory-recall-2026-05-06.md](archive/old_archive/context-phase2-memory-recall-2026-05-06.md) | Memory Recall Phase |
| [context-phase3-partial-compact-2026-05-06.md](archive/old_archive/context-phase3-partial-compact-2026-05-06.md) | 部分压缩 Phase |
| [context-phase4-verification-2026-05-06.md](archive/old_archive/context-phase4-verification-2026-05-06.md) | 验证 Phase |
| [context-phase5-token-budget-exact-fallback-2026-05-06.md](archive/old_archive/context-phase5-token-budget-exact-fallback-2026-05-06.md) | Token 预算精确回退 |
| [context-phase6-provider-token-parity-2026-05-06.md](archive/old_archive/context-phase6-provider-token-parity-2026-05-06.md) | Provider Token Parity |
| [context-phase7-partial-compact-command-roundtrip-2026-05-06.md](archive/old_archive/context-phase7-partial-compact-command-roundtrip-2026-05-06.md) | 部分压缩命令往返 |
| [context-phase7-request-boundary-2026-05-07.md](archive/old_archive/context-phase7-request-boundary-2026-05-07.md) | 请求边界 Phase |
| [context-phase8-model-assisted-memory-recall-2026-05-06.md](archive/old_archive/context-phase8-model-assisted-memory-recall-2026-05-06.md) | 模型辅助 Memory Recall |
| [context-phase9-system-prompt-metadata-contract-2026-05-06.md](archive/old_archive/context-phase9-system-prompt-metadata-contract-2026-05-06.md) | System Prompt 元数据契约 |
| [context-phase10-final-verification-2026-05-06.md](archive/old_archive/context-phase10-final-verification-2026-05-06.md) | 最终验证 Phase |

### 历史 Extensibility Phase 文档

| 文档 | 说明 |
|------|------|
| [extensibility-phase0-baseline-scope-2026-05-06.md](archive/old_archive/extensibility-phase0-baseline-scope-2026-05-06.md) | Extensibility 基线范围 |
| [extensibility-phase1-mcp-lifecycle-2026-05-06.md](archive/old_archive/extensibility-phase1-mcp-lifecycle-2026-05-06.md) | MCP 生命周期 |
| [extensibility-phase2-remote-https-sse-2026-05-06.md](archive/old_archive/extensibility-phase2-remote-https-sse-2026-05-06.md) | Remote HTTPS SSE |
| [extensibility-phase3-mcp-oauth-2026-05-06.md](archive/old_archive/extensibility-phase3-mcp-oauth-2026-05-06.md) | MCP OAuth |
| [extensibility-phase4-mcp-streamable-http-2026-05-06.md](archive/old_archive/extensibility-phase4-mcp-streamable-http-2026-05-06.md) | MCP Streamable HTTP |
| [extensibility-phase5-custom-agent-safety-2026-05-06.md](archive/old_archive/extensibility-phase5-custom-agent-safety-2026-05-06.md) | 自定义 Agent 安全 |
| [extensibility-phase6-integration-closure-2026-05-06.md](archive/old_archive/extensibility-phase6-integration-closure-2026-05-06.md) | 集成收尾 |

### 历史 Tools Phase 文档

| 文档 | 说明 |
|------|------|
| [tools-phase0-baseline-2026-05-06.md](archive/old_archive/tools-phase0-baseline-2026-05-06.md) | Tools 基线 |
| [tools-phase1-task-list-storage-2026-05-06.md](archive/old_archive/tools-phase1-task-list-storage-2026-05-06.md) | Task List 存储 |
| [tools-phase2-task-list-lock-2026-05-06.md](archive/old_archive/tools-phase2-task-list-lock-2026-05-06.md) | Task List 锁 |
| [tools-phase3-task-v2-schema-2026-05-06.md](archive/old_archive/tools-phase3-task-v2-schema-2026-05-06.md) | Task v2 Schema |
| [tools-phase4-teammate-unassign-2026-05-06.md](archive/old_archive/tools-phase4-teammate-unassign-2026-05-06.md) | Teammate 取消分配 |
| [tools-phase5-web-provider-diff-2026-05-06.md](archive/old_archive/tools-phase5-web-provider-diff-2026-05-06.md) | Web Provider Diff |
| [tools-phase6-final-verification-2026-05-06.md](archive/old_archive/tools-phase6-final-verification-2026-05-06.md) | Tools 最终验证 |
| [tools-tasks-00-inventory-2026-05-11.md](archive/old_archive/tools-tasks-00-inventory-2026-05-11.md) | Tasks 库存 |

### 历史 Checkpoint 文档

| 文档 | 说明 |
|------|------|
| [commands-00-checkpoint-2026-05-11.md](archive/old_archive/commands-00-checkpoint-2026-05-11.md) | 命令系统 Checkpoint |
| [ipc-00-checkpoint-2026-05-11.md](archive/old_archive/ipc-00-checkpoint-2026-05-11.md) | IPC Checkpoint |
| [ui-00-checkpoint-2026-05-12.md](archive/old_archive/ui-00-checkpoint-2026-05-12.md) | UI Checkpoint |
| [cc-daemon-phase0-baseline-2026-05-13.md](archive/old_archive/cc-daemon-phase0-baseline-2026-05-13.md) | cc-daemon 基线 |
| [workspace-crate-extraction-execution-tracker-2026-05-10.md](archive/old_archive/workspace-crate-extraction-execution-tracker-2026-05-10.md) | Workspace Crate 提取跟踪 |
| [ratatui-ui-parity-omx-execution-report-2026-05-08.md](archive/old_archive/ratatui-ui-parity-omx-execution-report-2026-05-08.md) | Ratatui Parity 执行报告 |

### 历史 Issues

| 文档 | 说明 |
|------|------|
| [2026-04-19-acp-agent-protocol.md](archive/old_archive/issues/2026-04-19-acp-agent-protocol.md) | ACP Agent 协议 |
| [2026-04-20-architecture-docs-rewrite.md](archive/old_archive/issues/2026-04-20-architecture-docs-rewrite.md) | 架构文档重写 |
| [2026-04-20-tui-codex-refactor.md](archive/old_archive/issues/2026-04-20-tui-codex-refactor.md) | TUI Codex 重构 |
| [2026-04-21-frontend-refactor-notes.md](archive/old_archive/issues/2026-04-21-frontend-refactor-notes.md) | 前端重构笔记 |
| [2026-05-07-code-review-findings.md](archive/old_archive/issues/2026-05-07-code-review-findings.md) | 代码审查发现 |
| [codex-review-review1.md](archive/old_archive/issues/codex-review-review1.md) | Codex Review 1 |

### 历史 Code Split

| 文档 | 说明 |
|------|------|
| [readme.md](archive/old_archive/code-split/readme.md) | Code Split 总目录 |
| [client-mod-refactor-plan.md](archive/old_archive/code-split/client-mod-refactor-plan.md) | Client 模块重构 |
| [dangerous-refactor-plan.md](archive/old_archive/code-split/dangerous-refactor-plan.md) | 危险模块重构 |
| [memdir-refactor-plan.md](archive/old_archive/code-split/memdir-refactor-plan.md) | Memdir 重构 |
| [openai_compat-refactor-plan.md](archive/old_archive/code-split/openai_compat-refactor-plan.md) | OpenAI Compat 重构 |
| [submit_message-refactor-plan.md](archive/old_archive/code-split/submit_message-refactor-plan.md) | Submit Message 重构 |
| [system_prompt-refactor-plan.md](archive/old_archive/code-split/system_prompt-refactor-plan.md) | System Prompt 重构 |

### 历史已实现

| 文档 | 说明 |
|------|------|
| [changelog-2026-04-10.md](archive/old_archive/implemented/changelog-2026-04-10.md) | 2026-04-10 变更日志 |
| [changelog-2026-04-15-codex-oauth-login.md](archive/old_archive/implemented/changelog-2026-04-15-codex-oauth-login.md) | Codex OAuth 登录 |
| [codex-agent.md](archive/old_archive/implemented/codex-agent.md) | Codex Agent |
| [daily-report-2026-04-11.md](archive/old_archive/implemented/daily-report-2026-04-11.md) | 日报 2026-04-11 |
| [daily-report-2026-04-15.md](archive/old_archive/implemented/daily-report-2026-04-15.md) | 日报 2026-04-15 |
| [ink-terminal-vs-opentui.md](archive/old_archive/implemented/ink-terminal-vs-opentui.md) | Ink Terminal vs OpenTUI |
| [ui-parity-implementation-note-2026-05-02.md](archive/old_archive/implemented/ui-parity-implementation-note-2026-05-02.md) | UI Parity 实施记录 |
| [ui-skeleton-surfaces-2026-05-03.md](archive/old_archive/implemented/ui-skeleton-surfaces-2026-05-03.md) | UI 骨架界面 |

### 历史 Superpowers

| 计划 | 说明 |
|------|------|
| [2026-04-09-pty-commands-and-multi-turn.md](archive/old_archive/superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md) | PTY 命令与多轮交互 |
| [2026-04-10-background-agents.md](archive/old_archive/superpowers/plans/2026-04-10-background-agents.md) | 后台 Agent 计划 |
| [2026-04-10-git-context-system-prompt.md](archive/old_archive/superpowers/plans/2026-04-10-git-context-system-prompt.md) | Git 上下文 System Prompt |
| [2026-04-10-hooks-system.md](archive/old_archive/superpowers/plans/2026-04-10-hooks-system.md) | Hook 系统计划 |
| [2026-04-10-lsp-service-implementation.md](archive/old_archive/superpowers/plans/2026-04-10-lsp-service-implementation.md) | LSP 服务实现 |
| [2026-04-10-web-search-cache.md](archive/old_archive/superpowers/plans/2026-04-10-web-search-cache.md) | Web Search 缓存 |
| [2026-04-11-kairos-implementation.md](archive/old_archive/superpowers/plans/2026-04-11-kairos-implementation.md) | Kairos 实现 |
| [2026-04-11-oauth-login.md](archive/old_archive/superpowers/plans/2026-04-11-oauth-login.md) | OAuth 登录 |
| [2026-04-11-team-memory.md](archive/old_archive/superpowers/plans/2026-04-11-team-memory.md) | Team Memory |
| [2026-04-11-team-memory-sync.md](archive/old_archive/superpowers/plans/2026-04-11-team-memory-sync.md) | Team Memory 同步 |
| [2026-04-12-tools-commands-test-coverage.md](archive/old_archive/superpowers/plans/2026-04-12-tools-commands-test-coverage.md) | 工具命令测试覆盖 |
| [2026-04-15-agent-ipc-extensions.md](archive/old_archive/superpowers/plans/2026-04-15-agent-ipc-extensions.md) | Agent IPC 扩展 |
| [2026-04-15-ipc-subsystem-extensions.md](archive/old_archive/superpowers/plans/2026-04-15-ipc-subsystem-extensions.md) | IPC 子系统扩展 |
| [2026-04-18-phase1-runtime-storage-unification.md](archive/old_archive/superpowers/plans/2026-04-18-phase1-runtime-storage-unification.md) | Runtime Storage 统一 |

| 设计文档 | 说明 |
|------|------|
| [2026-04-10-lsp-service-implementation-design.md](archive/old_archive/superpowers/specs/2026-04-10-lsp-service-implementation-design.md) | LSP 服务设计 |
| [2026-04-10-p0-security-hardening-design.md](archive/old_archive/superpowers/specs/2026-04-10-p0-security-hardening-design.md) | P0 安全加固设计 |
| [2026-04-11-kairos-design.md](archive/old_archive/superpowers/specs/2026-04-11-kairos-design.md) | Kairos 设计 |
| [2026-04-11-oauth-login-design.md](archive/old_archive/superpowers/specs/2026-04-11-oauth-login-design.md) | OAuth 登录设计 |
| [2026-04-11-team-memory-design.md](archive/old_archive/superpowers/specs/2026-04-11-team-memory-design.md) | Team Memory 设计 |
| [2026-04-11-team-memory-sync-design.md](archive/old_archive/superpowers/specs/2026-04-11-team-memory-sync-design.md) | Team Memory Sync 设计 |
| [2026-04-13-subagent-testing-dashboard-design.md](archive/old_archive/superpowers/specs/2026-04-13-subagent-testing-dashboard-design.md) | Subagent 测试面板设计 |
| [2026-04-15-agent-ipc-extensions-design.md](archive/old_archive/superpowers/specs/2026-04-15-agent-ipc-extensions-design.md) | Agent IPC 扩展设计 |
| [2026-04-15-codex-cli-credential-fallback-design.md](archive/old_archive/superpowers/specs/2026-04-15-codex-cli-credential-fallback-design.md) | Codex CLI 凭据回退设计 |
| [2026-04-15-ink-ui-experiment-design.md](archive/old_archive/superpowers/specs/2026-04-15-ink-ui-experiment-design.md) | Ink UI 实验设计 |
| [2026-04-15-ipc-subsystem-extensions-design.md](archive/old_archive/superpowers/specs/2026-04-15-ipc-subsystem-extensions-design.md) | IPC 子系统扩展设计 |
| [2026-04-16-web-chat-ui-design.md](archive/old_archive/superpowers/specs/2026-04-16-web-chat-ui-design.md) | Web Chat UI 设计 |
| [2026-04-18-phase1-runtime-storage-unification-design.md](archive/old_archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md) | Runtime Storage 统一设计 |
| [2026-04-20-workspace-split-design.md](archive/old_archive/superpowers/specs/2026-04-20-workspace-split-design.md) | Workspace 拆分设计 |

---

## 维护说明

- **新活跃参考文档** → 放入 `reference/` 目录
- **新进行中计划** → 放入 `archive/plan/` 目录
- **已完成/冻结文档** → 移入 `archive/old_archive/` 对应子目录
- 链接使用相对路径，确保目录可独立移动
