---
title: "功能开关系统 - Feature Flag 设计"
description: "详解 allthecodes 的 Feature Flag 系统：10 个功能开关的枚举定义、环境变量映射、父子依赖规则、全局单例与运行时覆盖机制。"
keywords: ["Feature Flag", "功能开关", "环境变量", "KAIROS", "Agent Teams", "Proactive"]
---

## 概述

`features.rs` 实现了基于环境变量的功能开关系统（Feature Flag System），用于控制 KAIROS 及相关实验性功能的启用状态。

## 功能枚举

`Feature` 枚举定义了 10 个功能变体：

| Feature | 环境变量 | 标签 | 描述 |
|---------|---------|------|------|
| Kairos | `FEATURE_KAIROS` | kairos | 助手模式与 KAIROS 守护进程总开关 |
| KairosBrief | `FEATURE_KAIROS_BRIEF` | kairos_brief | BriefTool 响应模式 |
| KairosChannels | `FEATURE_KAIROS_CHANNELS` | kairos_channels | 连接助手频道 |
| KairosPushNotification | `FEATURE_KAIROS_PUSH_NOTIFICATION` | kairos_push_notification | 助手推送通知命令 |
| KairosGithubWebhooks | `FEATURE_KAIROS_GITHUB_WEBHOOKS` | kairos_github_webhooks | KAIROS GitHub webhook 集成 |
| Proactive | `FEATURE_PROACTIVE` | proactive | 主动 tick 和 sleep 工具 |
| TeamMemory | `FEATURE_TEAMMEM` | team_memory | 团队记忆作用域与守护进程代理 |
| SubagentDashboard | `FEATURE_SUBAGENT_DASHBOARD` | subagent_dashboard | 子代理仪表盘 |
| AgentTeams | `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS` | agent_teams | 实验性 Agent Teams 命令/工具 |
| Coordinator | `ALLTHECODES_COORDINATOR_MODE` | coordinator | 协调器模式提示与编排门控 |

## 环境变量解析

`FeatureFlags::from_env_iter` 从环境变量键值对解析状态。有效的启用值（大小写不敏感、自动 trim）：`"1"`、`"true"`、`"yes"`。其他值或缺失视为禁用。

`AgentTeams` 同时读取 `FEATURE_AGENT_TEAMS` 和 `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS` 两个环境变量。

## 依赖规则

系统强制父子依赖，违反时自动禁用子功能并输出 `tracing::warn!` 警告：

```
Kairos (父)
├── KairosBrief
├── KairosChannels
├── KairosPushNotification
└── KairosGithubWebhooks
```

- Kairos 的子功能在父功能未启用时自动被禁用
- `Proactive` 在 `Kairos` 启用时自动隐含启用
- `Proactive` 也可以独立启用（不依赖 Kairos）

## 数据结构

`FeatureFlags` 结构体为每个功能保存一个 `bool` 字段，提供三个构造方式：

- `from_env()` — 从真实环境变量构造
- `all_enabled()` — 全部启用（用于测试或开发）
- `all_disabled()` / `Default` — 全部禁用

查询方法 `is_enabled(Feature)` 通过 match 返回对应字段的值。

## 全局单例

系统使用 `std::sync::LazyLock` 维护两个全局状态：

- **`FLAGS`** — 进程级静态标志，启动时从环境变量初始化一次，不可变
- **`RUNTIME_OVERRIDE`** — 可选的运行时覆盖，通过 `RwLock` 保护

公共 API：

- `enabled(Feature)` — 查询当前生效的开关状态（优先读取运行时覆盖）
- `current()` — 获取当前 `FeatureFlags`
- `set_runtime_override(flags)` — 设置运行时覆盖（`/experimental` 命令使用）
- `clear_runtime_override()` — 清除运行时覆盖，恢复环境变量值

## 使用场景

Feature Flag 系统主要服务于：

1. **KAIROS 生态**：KAIROS 及其子功能构成一组关联的实验性功能，由单一父开关管理
2. **运行时切换**：通过 `/experimental` 在对话中动态启用/禁用功能，无需重启进程
3. **渐进式发布**：新功能默认禁用，通过环境变量逐步开放给特定用户或场景
4. **开发调试**：`all_enabled()` 用于测试套件确保全功能覆盖

## 测试覆盖

`features.rs` 包含 450+ 行测试，验证了以下场景：

- 默认全部禁用
- `kairos` 启用时 `proactive` 自动开启
- `proactive` 可独立于 `kairos` 启用
- 子功能（`kairos_brief` 等）在父功能未启用时自动禁用
- `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS` 环境变量的读取
- `all_enabled()` 覆盖所有 `FeatureDescriptor`
- 无效值（`"0"`、`"false"`）不被视为启用
- 值自动 trim 且大小写不敏感

## FeatureDescriptor 注册表

每个功能变体对应一个 `FeatureDescriptor`，包含 `feature`（枚举值）、`env_var`（环境变量名）、`label`（标签）、`description`（描述）。这些描述符通过 `feature_descriptors()` 函数导出，用于：

- 工具注册时查询功能的启用状态
- 命令 `/experimental` 展示可切换的功能列表
- 测试验证全功能覆盖

## 启用检查的典型用法

系统中其他模块通过以下方式检查 feature flag：

```rust
// 在工具注册时
fn is_enabled(&self) -> bool {
    features::enabled(Feature::KairosBrief)
}

// 在业务逻辑中
if features::enabled(Feature::Proactive) {
    // 执行主动行为
}
```

这种模式确保功能门控逻辑集中管理，各模块只需引用 `Feature` 枚举值，无需硬编码环境变量名。
