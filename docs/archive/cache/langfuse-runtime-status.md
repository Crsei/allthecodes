# Langfuse 运行状态说明

> 最后更新: 2026-06-01

本文记录当前 allthecodes 仓库中 Langfuse / OpenTelemetry 代码的运行状态。结论先行：Langfuse 代码已经能在 `telemetry` feature 下编译并接入主流程，但默认构建不会启用，因此普通运行时是 no-op。

## 当前结论

当前状态可以概括为：

- 默认 feature 为空，普通 `cargo build --workspace --release` 不启用 Langfuse。
- 未启用 `telemetry` feature 时，Langfuse helper 走 stub，所有 trace/span 创建函数都返回 `None`，不会上报。
- 启用 `telemetry` feature 后，启动阶段会初始化 Langfuse exporter，并把 `tracing_opentelemetry` layer 挂到 tracing subscriber。
- 主 submit、generation、tool、tool batch、subagent 等调用点已经接入 Langfuse helper。
- 退出时会调用 `shutdown_langfuse()`，触发 provider shutdown / flush。
- 已验证命令 `cargo check -p allthecodes --features telemetry` 通过。

因此，Langfuse 不是“默认可用”，但也不是纯占位。它是 opt-in telemetry 路径。

## 启用方式

构建时需要显式打开 feature：

```bash
cargo build -p allthecodes --release --features telemetry
```

运行时至少需要设置：

```bash
LANGFUSE_PUBLIC_KEY=...
LANGFUSE_SECRET_KEY=...
```

可选配置：

```bash
LANGFUSE_BASE_URL=...        # 或 LANGFUSE_HOST
LANGFUSE_EXPORT_MODE=batched # 默认 batched，也支持 immediate
LANGFUSE_FLUSH_AT=20
LANGFUSE_FLUSH_INTERVAL=10
LANGFUSE_TIMEOUT=5
LANGFUSE_TRACING_ENVIRONMENT=development
LANGFUSE_USER_ID=...
```

如果缺少 `LANGFUSE_PUBLIC_KEY` 或 `LANGFUSE_SECRET_KEY`，`init_langfuse()` 返回 `Ok(None)`，程序继续运行但不启用 Langfuse。

### 通过 settings.json 配置

也可以把 Langfuse 变量写入 `~/.allthecodes/settings.json` 或项目 `.allthecodes/settings.json` 的 `env` 字段：

```json
{
  "env": {
    "LANGFUSE_PUBLIC_KEY": "pk-...",
    "LANGFUSE_SECRET_KEY": "sk-...",
    "LANGFUSE_BASE_URL": "https://cloud.langfuse.com",
    "LANGFUSE_TRACING_ENVIRONMENT": "development"
  }
}
```

启动阶段会在 `init_tracing()` / Langfuse 初始化前预加载 `settings.env`。优先级是 shell 环境变量和 `.env` 优先，`settings.json` 只补缺，不覆盖已有同名变量。该配置方式仍要求使用 `--features telemetry` 构建的二进制。

## Feature 闭合情况

根 CLI crate 的 `telemetry` feature 定义在 `crates/allthecodes/Cargo.toml`：

```toml
telemetry = [
    "dep:tracing-opentelemetry",
    "dep:opentelemetry",
    "dep:opentelemetry_sdk",
    "dep:opentelemetry-langfuse",
    "allthecodes-engine/telemetry",
    "allthecodes-services/telemetry",
    "allthecodes-startup/telemetry",
]
```

这会同时启用：

- `allthecodes-startup/telemetry`：启动时初始化 OTel layer。
- `allthecodes-services/telemetry`：真实 Langfuse exporter 与 telemetry bridge。
- `allthecodes-engine/telemetry`：engine 内 trace / span helper 的真实实现。

默认 feature 是：

```toml
default = []
```

所以默认构建不会拉入 OTel/Langfuse 依赖。

## 启动初始化链路

启动日志初始化位于 `crates/allthecodes-startup/src/logging.rs`。

当 `telemetry` feature 开启时，`init_tracing(verbose)` 会：

1. 建立 stderr tracing layer。
2. 建立 file tracing layer。
3. 调用 `allthecodes_services::langfuse::init_langfuse()`。
4. 如果返回 tracer，则创建：

```rust
tracing_opentelemetry::layer().with_tracer(tracer)
```

5. 将该 telemetry layer 挂到 subscriber。

如果初始化失败，会向 stderr 输出 warning，然后继续以普通 tracing 运行。

## Langfuse Exporter

真实 exporter 位于 `crates/allthecodes-services/src/langfuse/client.rs`。

启用条件：

```rust
LANGFUSE_PUBLIC_KEY && LANGFUSE_SECRET_KEY
```

host 选择顺序：

1. `LANGFUSE_BASE_URL`
2. `LANGFUSE_HOST`
3. `https://cloud.langfuse.com`

resource 属性：

- `service.name = allthecodes`
- `service.version = <crate version>`
- `deployment.environment = LANGFUSE_TRACING_ENVIRONMENT || development`

export mode：

- `LANGFUSE_EXPORT_MODE=immediate`：simple exporter
- 其他值或未设置：batch exporter

batch 参数：

- `LANGFUSE_FLUSH_AT` 默认 20，最小 1
- `LANGFUSE_FLUSH_INTERVAL` 默认 10 秒，最小 1 秒
- queue size 使用 `max(flush_at * 10, flush_at + 1)`

## 默认 No-op 路径

`crates/allthecodes-services/src/langfuse/mod.rs` 和 `crates/allthecodes-engine/src/services/langfuse/mod.rs` 都采用同样模式：

```rust
#[cfg(feature = "telemetry")]
pub mod client;
#[cfg(feature = "telemetry")]
pub mod tracing;

#[cfg(not(feature = "telemetry"))]
mod stub;
```

未启用 feature 时，导出的是 `stub.rs` 中的实现。

stub 行为：

- `shutdown_langfuse()` 空函数。
- `create_trace(...) -> None`
- `create_subagent_trace(...) -> None`
- `create_generation_span(...) -> None`
- `create_tool_span(...) -> None`
- `create_tool_batch_span(...) -> None`
- `finish_*` 和 `end_*` 空函数。

这意味着业务代码可以无条件调用 Langfuse helper，而普通构建不会产生任何 telemetry 输出。

## Submit Trace 链路

主入口在 `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`。

每次 `submit_message()`：

1. 读取当前 backend / model。
2. 创建 API client。
3. 如果 API client 可用，则取得 provider name。
4. 如果是 subagent，则调用：

```rust
create_subagent_trace(...)
```

5. 否则调用：

```rust
create_trace(...)
```

trace 被放入 `QueryEngineDeps.langfuse_trace`，后续 query loop 和 tool execution 共享这条 trace。

trace metadata 包含：

- provider
- model
- agentType
- querySource
- agentId（subagent）
- sanitized input
- tags
- session id / user id

## Generation Span 链路

模型调用在 query loop 中创建 generation span。

流程：

1. `query/loop_impl.rs` 将请求 messages、system prompt、tools 转成 Langfuse generation input。
2. API call 前调用：

```rust
create_generation_span(trace, model, provider, input)
```

3. API call 失败时调用：

```rust
finish_generation_span(span, None, None, None, Some(error))
```

4. API stream 正常完成后调用：

```rust
finish_generation_span(
    span,
    Some(convert_assistant_output(...)),
    assistant_message.usage.as_ref(),
    ttft_ms,
    None,
)
```

generation span 会记录：

- sanitized input
- sanitized output
- model
- provider
- prompt tokens
- completion tokens
- total tokens
- cache read tokens
- cache creation tokens
- TTFT
- error status（失败时）

其中 cache token 来自 `Usage.cache_read_input_tokens` 与 `Usage.cache_creation_input_tokens`。

## Tool Span 链路

工具执行接入位于 `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`。

每个工具执行时：

1. 从 `QueryEngineDeps.langfuse_trace` 取 root trace。
2. 调用：

```rust
create_tool_span(trace, tool_name, tool_use_id, input, parent_batch_span)
```

3. 成功或失败时调用：

```rust
finish_tool_span(span, tool_name, output, is_error)
```

tool span 会记录：

- tool name
- tool use id
- sanitized tool input
- sanitized tool output
- isError
- error status（失败时）

## Tool Batch Span

并发/串行工具批次在 query loop helper 中可创建 batch span：

```rust
create_tool_batch_span(trace, tool_names, batch_index)
```

每个工具 span 可以挂在 batch span 下面，batch span 记录：

- toolNames
- toolCount
- batchIndex

批次结束后调用 `end_span(batch_span)`。

## Trace 结束

submit 完成后会调用：

```rust
end_trace(trace, output, status)
```

错误路径会传 `TraceStatus::Error`。

程序退出前，root CLI 在 `crates/allthecodes/src/main.rs` 调用：

```rust
allthecodes_services::langfuse::shutdown_langfuse();
```

这会 shutdown `SdkTracerProvider`，让 batched exporter 有机会 flush。

## 两套 Langfuse 模块

仓库中存在两套 Langfuse 模块：

- `crates/allthecodes-services/src/langfuse/`
- `crates/allthecodes-engine/src/services/langfuse/`

当前运行路径中：

- startup 初始化使用 `allthecodes_services::langfuse::init_langfuse()`。
- main 退出使用 `allthecodes_services::langfuse::shutdown_langfuse()`。
- engine submit/query/tool 调用使用 `crate::services::langfuse::*`，即 engine 内模块。

在 `telemetry` feature 同时启用 `allthecodes-services/telemetry` 和 `allthecodes-engine/telemetry` 时，两边真实实现都可编译。由于 tracing subscriber 挂的是 services 初始化出来的 OTel tracer layer，engine 内 helper 创建的 `tracing::info_span!` 也会经过这个 subscriber 被导出。

普通构建时，engine 和 services 两边都走 stub 或未初始化路径，因此不会导出。

## 与普通日志的关系

Langfuse 不是替代本地日志。

本地 tracing 仍然写：

- stderr layer
- file layer

Langfuse 只是 `telemetry` feature 下额外挂接的 OpenTelemetry layer。即使 Langfuse 初始化失败，本地日志仍然工作。

## 当前验证

已在本仓库验证：

```bash
CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo \
RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup \
PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:$PATH \
cargo check -p allthecodes --features telemetry
```

结果：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.07s
```

该验证说明 telemetry feature 当前可以编译通过。它不等价于已经向真实 Langfuse 实例成功上报，因为本次未配置 Langfuse key，也未执行真实会话请求。

## 如何确认线上是否真的上报

1. 使用 `--features telemetry` 构建二进制。
2. 设置 `LANGFUSE_PUBLIC_KEY` 和 `LANGFUSE_SECRET_KEY`。
3. 如使用自建 Langfuse，设置 `LANGFUSE_BASE_URL`。
4. 运行一次真实 prompt，使模型调用完成。
5. 退出程序或等待 batch flush interval。
6. 在 Langfuse UI 中搜索 session id / trace name。

trace name 当前大致为：

- 主会话：`agent-run:<query_source>`
- subagent：`agent:<agent_type>`
- generation observation：provider 相关名称
- tool observation：工具名

## 已知边界

- 默认构建不会启用 Langfuse。
- 没有 key 时不会初始化 exporter。
- 当前验证只覆盖编译，不覆盖真实网络上报。
- exporter 失败只警告，不阻塞主程序。
- sanitize / convert 纯逻辑已收敛到 `crates/allthecodes-langfuse/`；services 和 engine 仍各自保留 runtime tracing/client/stub 边界。
- telemetry 会拉入 OTel、tonic、prost 等较重依赖，因此默认关闭是有意设计。

## 运维检查清单

### 启用前确认

- [ ] build 命令包含 `--features telemetry`。
- [ ] `LANGFUSE_PUBLIC_KEY` 已设置且非空。
- [ ] `LANGFUSE_SECRET_KEY` 已设置且非空。
- [ ] 自部署时 `LANGFUSE_BASE_URL` 指向正确实例。
- [ ] 如使用 `settings.json` 配置，确认变量位于 `env` 字段，且没有被 shell 或 `.env` 中的同名变量覆盖。

### 启动确认

- [ ] 启动日志无 `failed to initialize Langfuse tracing` 警告。
- [ ] telemetry feature 构建无新增 warning。

### 运行时确认

- [ ] Langfuse UI 中可搜索到 trace 名称：`agent-run` 或 `agent-run:<query_source>`。
- [ ] Generation span 名称正确，例如 `ChatAnthropic`、`ChatOpenAI`、`ChatAzureOpenAI`。
- [ ] Tool span 显示工具名称、tool use id、脱敏后的输入/输出和 `isError`。
- [ ] Token 用量数据正确，包括 input、output、cache read、cache creation tokens。
- [ ] Session ID 可用于聚合查看同一会话。

### 退出确认

- [ ] 程序退出时无 `failed to shutdown langfuse` 警告。
- [ ] batched exporter 场景下已等待 flush interval，或正常走 `shutdown_langfuse()`。

## Langfuse Dashboard 使用指南

- 按 Session 聚合：在 Langfuse UI 的 Session 筛选器输入 session id。
- 按 Trace Name 过滤：搜索 `agent-run` 或 `agent:` 前缀。
- 按 Tag 过滤：Tags 包括 `allthecodes`、`submit`、`subagent`、agent type 等。
- 查看 TTFT：在 Generation span 的 Metadata JSON 中查看 `ttftMs`，或查看 span event `completion_start` 的 `ttft_ms` 字段。
- 查看工具错误：筛选 tool observation，检查 `isError=true` 和 span status。

## 异常排查流程

1. 确认 feature 已开启：

   ```bash
   CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo \
   RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup \
   PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:$PATH \
   cargo check -p allthecodes --features telemetry
   ```

2. 确认 key 已设置但不要打印密钥内容：

   ```bash
   test -n "$LANGFUSE_PUBLIC_KEY" && test -n "$LANGFUSE_SECRET_KEY"
   ```

3. 检查 exporter 初始化：启动日志中搜索 `Langfuse tracing` 或 `failed to initialize Langfuse tracing`。
4. 网络连通性：自部署时执行 `curl -v "$LANGFUSE_BASE_URL/api/public/health"`。
5. 退出 flush：确认主程序仍调用 `allthecodes_services::langfuse::shutdown_langfuse()`。
6. 检查 batch 参数：确认 `LANGFUSE_FLUSH_AT` / `LANGFUSE_FLUSH_INTERVAL` 与实际请求频率匹配。

## 自部署 Langfuse（Docker Compose 最小配置）

```yaml
version: "3.8"

services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: langfuse
      POSTGRES_USER: langfuse
      POSTGRES_PASSWORD: changeme
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U langfuse"]
      interval: 5s
      timeout: 5s
      retries: 10

  langfuse:
    image: langfuse/langfuse:latest
    ports:
      - "3000:3000"
    environment:
      DATABASE_URL: postgresql://langfuse:changeme@postgres:5432/langfuse
      NEXTAUTH_SECRET: change-this-random-secret
      NEXTAUTH_URL: http://localhost:3000
      SALT: change-this-random-salt
    depends_on:
      postgres:
        condition: service_healthy

volumes:
  pgdata:
```

启动后：

1. 打开 `http://localhost:3000` 并注册管理员账号。
2. 在 Project Settings -> API Keys 获取 `LANGFUSE_PUBLIC_KEY` / `LANGFUSE_SECRET_KEY`。
3. 设置 `LANGFUSE_BASE_URL=http://localhost:3000`。
4. 使用 `--features telemetry` 构建并运行 allthecodes。
