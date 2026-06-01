---
title: "Debug 模式"
description: "使用 tracing、RUST_LOG 和调试工具对 allthecodes Rust 运行时进行诊断和调试。"
keywords: ["debug", "调试", "tracing", "RUST_LOG", "诊断"]
---

## 概述

allthecodes 的调试能力基于 Rust 生态的 `tracing` 框架，通过 `RUST_LOG` 环境变量控制日志级别和过滤。与 TypeScript 原版通过 VS Code attach 到 Bun inspect 服务的调试方式不同，Rust 端口原生支持结构化日志和性能追踪。

## 日志系统

### 基础用法

```bash
# 设置日志级别（默认只显示 error）
RUST_LOG=info cargo run

# 按模块过滤
RUST_LOG=allthecodes_engine=debug cargo run

# 显示所有调试日志
RUST_LOG=debug cargo run

# 组合过滤
RUST_LOG=allthecodes_engine=info,allthecodes_daemon=debug cargo run
```

### 日志级别

遵循标准 `tracing` 级别：
- `error` — 错误事件
- `warn` — 警告事件
- `info` — 信息事件（默认级别）
- `debug` — 调试事件
- `trace` — 追踪事件（最详细）

### 结构化日志

所有日志使用 `tracing` 宏输出，支持结构化字段：

```rust
tracing::info!(
    url = %url,
    port = config.port,
    "dashboard companion ready"
);
```

结构化字段可以被日志后端（如 `tracing-subscriber`、`tokio-console`）解析和过滤。

## 调试技术

### RUST_LOG 策略

推荐的分层调试策略：

```bash
# 1. 追踪特定 crate
RUST_LOG=allthecodes_safety=debug cargo test

# 2. 追踪特定的模块路径
RUST_LOG=allthecodes_engine::lifecycle=trace cargo run

# 3. 追踪 IPC 通信
RUST_LOG=allthecodes_ipc=debug cargo run

# 4. 全量追踪（输出量巨大）
RUST_LOG=trace cargo run 2>&1 | head -1000
```

### 测试调试

```bash
# 显示测试中的日志输出
RUST_LOG=debug cargo test -- --nocapture

# 运行特定测试并查看日志
RUST_LOG=allthecodes_safety=debug cargo test classifier -- --nocapture
```

### 断言与快照测试

关键模块使用 `insta` 快照测试来验证输出形状：

```rust
// classifier.rs 中的 Prompt 形状测试
insta::assert_snapshot!(
    prompt.user.lines().take(19).collect::<Vec<_>>().join("\n"),
    @"..."
);
```

## 配置验证

调试配置问题时，可以检查 `allthecodes-config` crate 的加载逻辑：

```bash
# 查看配置加载的详细日志
RUST_LOG=allthecodes_config=debug cargo run

# 验证 settings.json 解析
RUST_LOG=allthecodes_config::settings=trace cargo run
```

## 性能追踪

对于性能问题，可以使用以下方法：

```bash
# 启用 tokio 任务追踪
RUST_LOG=tokio=trace cargo run

# 追踪 QueryEngine 生命周期
RUST_LOG=allthecodes_engine::lifecycle=trace cargo run

# 追踪工具执行
RUST_LOG=allthecodes_engine::tool_runtime=debug cargo run
```

## 常见问题诊断

| 症状 | 调试方法 |
|------|----------|
| 工具执行失败 | `RUST_LOG=allthecodes_engine::tool_runtime=debug` |
| 权限判断异常 | `RUST_LOG=allthecodes_safety=debug` |
| 守护进程连接失败 | `RUST_LOG=allthecodes_daemon=debug` |
| 配置加载错误 | `RUST_LOG=allthecodes_config=debug` |
| IPC 通信异常 | `RUST_LOG=allthecodes_ipc=debug` |
| LLM 调用异常 | `RUST_LOG=allthecodes_engine::query=debug` |

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-config/src/settings/load.rs` | 配置加载逻辑（含 debug 日志） |
| `crates/allthecodes-config/src/validation.rs` | 配置验证 |
| `crates/allthecodes-config/src/validation_tips.rs` | 验证提示 |
| `crates/allthecodes-engine/src/lifecycle/mod.rs` | QueryEngine 生命周期（核心调试目标） |
| `crates/allthecodes-engine/src/query/` | 查询循环（LLM 交互追踪） |
| `crates/allthecodes-engine/src/tool_runtime/` | 工具执行运行时 |
| `crates/allthecodes-daemon/src/` | 守护进程组件 |
| `crates/allthecodes-ipc/src/` | IPC 通信组件 |
