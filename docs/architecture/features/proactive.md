---
title: "Proactive 模式"
description: "Tick 驱动型自主代理模式，使 CLI 在用户不输入时自主工作：定时唤醒、执行任务、控制节奏。"
keywords: ["proactive", "主动模式", "自主", "autonomous", "tick", "SleepTool"]
---

## 概述

Proactive 模式（主动模式）实现 Tick 驱动的自主代理。CLI 在用户不输入时也能持续工作：通过定时 Tick 唤醒模型执行任务，配合 Sleep 工具控制唤醒节奏。适用于长时间运行的后台任务：等待 CI、监控文件变化、定时检查等。

Feature Flag: `FEATURE_PROACTIVE=1`（与 `FEATURE_KAIROS=1` 共享功能）

## 架构

### Tick 调度器

守护进程的 `tick_loop` 模块实现核心 Tick 循环（`crates/allthecodes-daemon/src/tick.rs`）：

- 默认间隔：**30 秒**
- 跳过条件：
  - 查询正在运行（`is_query_running == true`）
  - 引擎处于睡眠状态（`is_sleeping()` 返回 true）
  - 守护进程级休眠状态激活
- Tick 内容：
  - 当前本地时间
  - 终端焦点状态（`focus: true/false`）
  - 当日日志摘要

### 引擎睡眠控制

QueryEngine 维护睡眠状态（`sleep_until`）：

- `set_sleep_until(instant)` — 设置引擎休眠至指定时刻
- `is_sleeping()` — 检查引擎是否在睡眠中（当前时间 < sleep_until）
- `wake_up()` — 唤醒引擎，清除睡眠状态。在用户消息、Webhook、外部事件到达时自动调用

### 两阶段自主策略

| 阶段 | 触发条件 | 行为 |
|------|----------|------|
| 首次唤醒 | 第一个 Tick | 简短问候，询问工作方向，不主动探索 |
| 后续唤醒 | 后续 Tick，用户已给出方向 | 寻找有用工作：调查、验证、检查、提交 |

### 终端焦点感知

- **`focus: false`**（用户不在看终端）→ 高度自主，执行待处理任务
- **`focus: true`**（用户正在看终端）→ 更协作，大改动前先询问

### 系统提示注入

在 `system_prompt/dynamic_sections.rs` 中，`proactive_mode_section()` 函数在 Feature::Proactive 启用时注入以下指令：

```
- 收到 <tick_tag> 消息，包含用户本地时间和终端焦点
- 首次 Tick：简短问候，不要主动探索
- 后续 Tick：寻找有用工作
- 无工作可做：必须调用 Sleep 工具，禁止输出 "still waiting"
- 不要骚扰用户，如果已提问则等待回复
- 偏向行动：读文件、搜索代码、做改动、提交
- 所有面向用户的输出必须通过 Brief 工具
```

## 数据流

```
activateProactive()
      │
      ▼
Tick 调度器启动（30s 间隔）
      │
      ├── 定时生成 <tick_tag> 消息
      │   ├── 用户本地时间
      │   └── 终端焦点状态
      │
      ▼
引擎处理 Tick
      │
      ├── 有事可做 → 使用工具执行
      ├── 无事可做 → 调用 Sleep 工具
      │
      ▼
引擎进入睡眠
      │
      ├── 用户插入新消息 / 队列有命令 → 立即唤醒
      ├── Proactive 关闭 → 立即中断
      │
      ▼
下一个 Tick 到达
```

## 与 KAIROS 的关系

所有代码检查均为 `feature('PROACTIVE') || feature('KAIROS')`：

- 单独 `FEATURE_PROACTIVE=1` — 获得 proactive 能力
- 单独 `FEATURE_KAIROS=1` — 自动隐含 proactive 能力
- 两者都开 — 相同效果，不重复

## 使用方式

```bash
# 单独启用 Proactive
FEATURE_PROACTIVE=1 cargo run

# 通过 KAIROS 间接启用
FEATURE_KAIROS=1 cargo run

# 组合使用
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo run

# 查看 Proactive 状态
curl http://127.0.0.1:19836/api/status
# 返回: { "proactive": true, "query_running": false, "sleeping": false }
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-daemon/src/tick.rs` | Tick 循环核心实现 |
| `crates/allthecodes-engine/src/lifecycle/mod.rs` | 引擎睡眠控制（sleep_until / is_sleeping / wake_up） |
| `crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs` | Proactive 模式系统提示注入 |
| `crates/allthecodes-engine/src/system_prompt/mod.rs` | Prompt section 注册 |
| `crates/allthecodes-config/src/features.rs` | Feature 门控（FEATURE_PROACTIVE / FEATURE_KAIROS） |
| `crates/allthecodes-daemon/src/state.rs` | 守护进程状态（terminal_focus 等） |
| `crates/allthecodes-daemon/src/process_state.rs` | 守护进程休眠状态持久化 |
| `crates/allthecodes-daemon/src/memory_log.rs` | 每日日志记录 |
