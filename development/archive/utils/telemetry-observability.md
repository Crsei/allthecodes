# 遥测/可观测性移植缺口

对应: `claude-code-bun/src/utils/telemetry/`  
Rust 对应: `crates/cc-observability/`、`crates/cc-services/src/langfuse/`、`crates/start-up/src/logging.rs`

> 本文按 Full Build 口径记录真实 Rust crate/runtime path。旧 Lite 缩减不能作为边界；如果某项 Bun 行为仍未补齐，应标为缺口并给出 Rust 落点。

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| instrumentation.ts | 831 | OpenTelemetry 初始化设置 |
| sessionTracing.ts | 927 | 会话追踪 API |
| perfettoTracing.ts | 1,120 | Perfetto 高性能追踪 |
| betaSessionTracing.ts | 491 | Beta 会话追踪 |
| events.ts | 75 | OTEL 事件日志 |
| pluginTelemetry.ts | 289 | 插件遥测 |
| bigqueryExporter.ts | 252 | BigQuery 导出器 |
| logger.ts | 26 | OTEL 诊断日志 |
| skillLoadedEvent.ts | 39 | 技能加载事件 |
| **合计** | **~4,050** | |

## Rust 已实现

### `crates/cc-observability/`：运行时审计事件，非 OTel

- `src/event.rs`: 定义 `AuditEvent`、`EventKind`、`Stage`、`Outcome`、`SessionMeta` 等稳定 NDJSON 事件模型。
- `src/context.rs`: `AuditContext` 负责传播 `session_id`、`submit_id`、`turn_id`、`request_id`、`tool_use_id` 等关联 ID，并提供 `with_submit()` / `with_turn()` / `with_request()` / `with_tool_use()`。
- `src/sink.rs`: `AuditSink` 写入 `events.ndjson` / `meta.json` / `artifacts/`，支持 `CC_AUDIT_LOG`、`CC_AUDIT_STREAM_DELTAS`、`CC_AUDIT_REDACTION` 和基础敏感字段脱敏。
- `src/lib.rs`: 只导出 audit 事件模型、上下文和 sink；没有 OpenTelemetry provider/exporter。

### 非测试 runtime audit call sites

- `crates/claude-code-rs/src/main.rs`: 初始化 `AuditSink`，写入 `SessionStart`，运行目录由 `cc_config::paths::runs_dir(session_id)` 决定。
- `crates/claude-code-rs/src/shutdown.rs`: 写入 `SessionEnd` 并 `sync()`。
- `crates/cc-engine/src/lifecycle/submit_message.rs`: 写入 `SubmitReceived`、`SubmitCompleted`。
- `crates/cc-engine/src/query/loop_impl.rs`: 写入 `QueryTurnStart`、`ModelRequestStart`、`ModelRequestError`、`ModelRequestFinish`。
- `crates/cc-engine/src/lifecycle/deps.rs`: 写入 `PermissionRequested`、`PermissionResolved`、`ToolStart`、`ToolFinish`、`ToolError`。

当前 audit 事件枚举还包含 `QueryTurnStop`、`ToolProgress`、`CompactPre/Post`、`StreamStart/Stop/Delta`、`BackgroundAgent*`、`IpcClient*`、`DaemonSse*` 等类型，但未找到对应非测试 runtime 写入点。

### 普通 tracing 日志

- `crates/start-up/src/logging.rs`: 初始化 stderr + daily file tracing subscriber，日志目录来自 `cc_config::paths::logs_dir()`；默认 stderr 为 warn，`--verbose` 或 `RUST_LOG` 可提升等级，文件层为 debug。

### Langfuse / OpenTelemetry 子集：已有代码，但运行时闭合不足

- workspace 依赖已有 `tracing-opentelemetry`、`opentelemetry`、`opentelemetry_sdk`、`opentelemetry-langfuse`。
- `crates/claude-code-rs/Cargo.toml` 的 `telemetry` feature 会启用 `cc-services/telemetry` 与 `cc-startup/telemetry`。
- `crates/start-up/src/logging.rs` 在 `telemetry` feature 下调用 `cc_services::langfuse::init_langfuse()`，并挂载 `tracing_opentelemetry::layer()`。
- `crates/cc-services/src/langfuse/client.rs` 支持 `LANGFUSE_PUBLIC_KEY` / `LANGFUSE_SECRET_KEY`、`LANGFUSE_BASE_URL` / `LANGFUSE_HOST`、flush 参数和 batched/immediate exporter。
- `crates/cc-services/src/langfuse/tracing.rs` 定义 trace / subagent / generation / tool / tool_batch span helper，并写入 Langfuse 属性。
- `crates/cc-services/src/langfuse/convert.rs`、`sanitize.rs` 已有 prompt/tool 输入输出转换和截断/脱敏逻辑。

需要注意：真实 engine runtime call sites 当前使用的是 `crates/cc-engine/src/services/langfuse/mod.rs`、`crates/cc-engine/src/services/langfuse/client.rs`、`crates/cc-engine/src/services/langfuse/tracing.rs`、`crates/cc-engine/src/services/langfuse/stub.rs` 中的同名模块，而不是 `cc-services` 模块。`crates/cc-engine/Cargo.toml` 里的 `telemetry = []` 没有声明 OTel 依赖，`crates/claude-code-rs/Cargo.toml` 的 `telemetry` feature 也没有启用 `cc-engine/telemetry`。因此在未显式修复/启用 `cc-engine/telemetry` 的应用构建中，`crates/cc-engine/src/services/langfuse/stub.rs` 会让这些 submit/model/tool span call sites 变成 no-op；这属于“代码部分存在，但 feature/runtime path 未闭合”，不是 Bun 端完整 OTel 会话追踪。

### Session audit export

- `crates/cc-session/src/audit_export.rs`: 可以从已保存 session 或内存 messages 生成可校验 `.audit.json`，包含消息 hash chain、metadata、token/cost 汇总。
- 该模块是 session 导出/合规记录，不是 Bun `sessionTracing.ts` 的 runtime span/context API，也不替代 OTel/Perfetto。

### 插件运行时

- `crates/cc-plugins/src/mod.rs`、`refresh.rs`、`tools.rs` 有插件注册、reload、IPC subsystem event 和插件工具执行包装。
- `crates/claude-code-rs/src/app_subsystem_handlers.rs` 对插件 enable/disable/reload/uninstall 写普通 `tracing::info!`，并返回 `PluginEvent`。
- 未找到 Bun `pluginTelemetry.ts` 对应的 raw/redacted 双列事件、固定盐插件 ID hash、插件安装/执行专用 telemetry sink。插件工具被执行时只会走通用 tool audit path。

## Rust 缺失的主要功能

### 1. OpenTelemetry 完整集成 (831 行) — 部分实现，完整能力缺失

已有 Langfuse trace exporter 子集和 startup OTel layer，但缺少 Bun `instrumentation.ts` 的完整面：

- OTLP / Prometheus / Console / BigQuery 多导出器动态选择。
- metrics `MeterProvider` 与 logs `LoggerProvider` 初始化。
- env / host / OS 等完整资源检测；当前 Langfuse resource 只有 `service.name`、`service.version`、`deployment.environment`。
- 代理、mTLS、证书配置。
- OTel event logger 对应 `events.ts` 的统一事件面。
- `cc-services` 与 `cc-engine` 的 telemetry feature/runtime path 闭合。

### 2. 会话追踪 (927 行) — audit 部分已有，OTel span 版本未闭合

已实现：

- submit/query/model/tool/permission 的 NDJSON audit 事件。
- Langfuse trace/generation/tool helper 代码。

缺失：

- 每个用户输入的 Interaction root span 生命周期与上下文管理。
- Hook spans、blocked_on_user、tool execution 的完整 OTel 属性集。
- `AsyncLocalStorage` 等价的跨 async 边界上下文 API。
- Assistant/user/system/message 级 OTel event 写入。
- `QueryTurnStop`、`ToolProgress`、compaction、stream delta、background agent、IPC/daemon 等已定义事件的 runtime 写入。
- 当前 engine Langfuse call sites 默认走 stub，OTel span 未在应用 feature 下闭合。

### 3. Perfetto 追踪 (1,120 行) — 完全缺失

- 未找到 `perfetto` 依赖、trace packet writer、trace 文件输出、interaction/LLM/tool/user-input span 映射。
- 未找到 ConfigScreenshots 或类似配置截图追踪。

### 4. Beta 会话追踪 (491 行) — 完全缺失

- 未找到 beta session tracing 模块。
- 未实现详细属性追踪、内容截断隐私策略、PII 标记和 beta 增强 telemetry 与常规 session tracing 的合流。

### 5. BigQuery 导出器 (252 行) — 完全缺失

- 未找到 BigQuery client/dependency、导出器实现、批量写入或失败重试路径。
- 现有 `cc-session/src/audit_export.rs` 只写本地 audit JSON，不是 BigQuery 指标/事件导出。

### 6. 插件遥测 (289 行) — 完全缺失

- 未实现 raw + redacted 双列隐私模式。
- 未实现固定盐插件 ID hash。
- 未实现插件加载、安装、启用/禁用、工具调用、错误诊断的专用 telemetry event schema。

## 主要缺口的 Rust 落点

| 缺口 | 需要修改/新增的 Rust 文件 |
|------|---------------------------|
| OTel exporter/instrumentation 总入口 | 修改 `Cargo.toml`、`crates/claude-code-rs/Cargo.toml`、`crates/start-up/Cargo.toml`、`crates/cc-services/Cargo.toml`；修改 `crates/start-up/src/logging.rs`；新增 `crates/cc-services/src/telemetry/mod.rs`、`crates/cc-services/src/telemetry/instrumentation.rs`；修改 `crates/cc-services/src/lib.rs` 导出 telemetry 模块。 |
| `cc-services` / `cc-engine` telemetry feature 闭合 | 修改 `crates/claude-code-rs/Cargo.toml` 启用正确 engine/service feature；修改 `crates/cc-engine/Cargo.toml` 的 `telemetry` feature 或移除 engine 内镜像实现；修改 `crates/cc-engine/src/services/langfuse/mod.rs`、`client.rs`、`tracing.rs`、`stub.rs`，或改为统一调用 `cc_services::langfuse`；同步修改 `crates/cc-engine/src/lifecycle/submit_message.rs`、`crates/cc-engine/src/lifecycle/deps.rs`、`crates/cc-engine/src/query/loop_impl.rs`、`crates/cc-engine/src/query/loop_helpers.rs`。 |
| 会话追踪 API / Interaction span | 新增 `crates/cc-observability/src/session_tracing.rs` 或 `crates/cc-services/src/telemetry/session_tracing.rs`；修改 `crates/cc-observability/src/lib.rs`；修改 `crates/cc-engine/src/lifecycle/submit_message.rs`、`crates/cc-engine/src/query/loop_impl.rs`、`crates/cc-engine/src/lifecycle/deps.rs`；如需更底层 API 请求属性，修改 `crates/cc-api/src/api/client/mod.rs`、`crates/cc-api/src/api/streaming.rs`。 |
| Hook span / permission span 补齐 | 修改 `crates/cc-engine/src/lifecycle/deps.rs`；修改 `crates/cc-tools/src/hooks/execution.rs`、`crates/cc-tools/src/hooks/pre_tool.rs`、`crates/cc-tools/src/hooks/post_tool.rs`、`crates/cc-tools/src/hooks/http_hook.rs`。 |
| audit 已定义但未写入的事件 | 修改 `crates/cc-engine/src/query/loop_impl.rs` 写 `QueryTurnStop` / stream 事件；修改 `crates/cc-engine/src/lifecycle/deps.rs` 写 `ToolProgress`；修改 compaction 落点 `crates/cc-compact/src/pipeline.rs`、`crates/cc-compact/src/compaction.rs` 或 engine 调用处；修改 background agent 落点 `crates/cc-engine/src/agent/mod.rs`、`crates/cc-engine/src/agent/supervisor.rs`；修改 IPC/daemon 落点 `crates/cc-ipc/src/headless.rs`、`crates/cc-daemon/src/sse.rs`、`crates/cc-daemon/src/server.rs`。 |
| Perfetto tracing | 新增 `crates/cc-observability/src/perfetto.rs` 或 `crates/cc-services/src/telemetry/perfetto.rs`；修改 `crates/cc-observability/src/lib.rs` 或 `crates/cc-services/src/telemetry/mod.rs`；修改 `crates/cc-config/src/paths.rs` 增加 trace 输出路径；修改 `crates/cc-engine/src/lifecycle/submit_message.rs`、`crates/cc-engine/src/query/loop_impl.rs`、`crates/cc-engine/src/lifecycle/deps.rs` 写入 interaction/model/tool/user-input trace packet。 |
| Beta session tracing | 新增 `crates/cc-observability/src/beta_session_tracing.rs` 或 `crates/cc-services/src/telemetry/beta_session_tracing.rs`；新增 `crates/cc-services/src/telemetry/privacy.rs`；修改 submit/model/tool/hook call sites：`crates/cc-engine/src/lifecycle/submit_message.rs`、`crates/cc-engine/src/query/loop_impl.rs`、`crates/cc-engine/src/lifecycle/deps.rs`。 |
| BigQuery exporter | 新增 `crates/cc-services/src/telemetry/bigquery_exporter.rs`；修改 `crates/cc-services/src/telemetry/instrumentation.rs` 注册 exporter；修改 `crates/cc-services/Cargo.toml` 引入 BigQuery/GCP auth 依赖；必要时修改 `crates/cc-config/src/settings.rs` 增加配置项。 |
| Plugin telemetry | 新增 `crates/cc-plugins/src/telemetry.rs`；修改 `crates/cc-plugins/src/mod.rs`、`loader.rs`、`refresh.rs`、`tools.rs`；修改 `crates/claude-code-rs/src/app_subsystem_handlers.rs` 记录插件 IPC 操作；如复用 hash，需新增稳定 salted hash helper（不要直接用 `crates/cc-utils/src/hash.rs` 的 `hash_content()`，它是非加密且面向本地缓存）。 |

## Rust vs Bun 遥测策略差异

Bun 版本提供生产级 OpenTelemetry、Perfetto、BigQuery、插件遥测和 beta session tracing。Rust 当前有两条不完整路径：

- 本地 runtime audit：`cc-observability` + engine/main/shutdown call sites，能写本地 NDJSON 审计事实。
- Langfuse/OTel 子集：`cc-services/src/langfuse` + `start-up/src/logging.rs` 已有代码，但 engine call sites 与 feature 归属未闭合。

因此不能再把 Rust 状态描述为“只有基础事件类型”，也不能把 OpenTelemetry 写成“完全缺失”。准确状态是：本地 audit 已有可运行子集；Langfuse/OTel 有实现碎片但 runtime feature 未闭合；Perfetto、BigQuery、Beta tracing、插件专用 telemetry 仍为缺口。

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| 事件类型 | 完整 | `crates/cc-observability/src/event.rs` | ✅ 部分 |
| 事件接收 | 完整 | `AuditSink` 写本地 NDJSON | ✅ 部分 |
| runtime audit call sites | 完整 tracing 体系 | session/submit/query/model/tool/permission 已写入 | ✅ 部分 |
| OTel 初始化 | 完整多导出器 | Langfuse exporter + OTel layer 子集 | ⚠️ 部分，feature/runtime 未闭合 |
| 会话追踪 | 完整 Interaction/LLM/Tool/Hook spans | audit 事件 + Langfuse helper 碎片 | ⚠️ 部分 |
| Perfetto | 完整 | 无 | ❌ 缺失 |
| Beta 追踪 | 完整 | 无 | ❌ 缺失 |
| BigQuery | 完整 | 无 | ❌ 缺失 |
| 插件遥测 | 完整 | 只有插件 registry/subsystem/logging；无专用 telemetry | ❌ 缺失 |
