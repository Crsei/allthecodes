# Anthropic API 与 Coding-Compatible Provider 风险治理计划

创建时间：2026-05-17
执行模式：分阶段执行计划

## 目标

将 cc-rust 的 Anthropic 协议支持提升为可生产使用的契约，覆盖三个不同场景：

- 通过 `https://api.anthropic.com` 直接访问 Anthropic API。
- Cloud Anthropic 传输层，如 Bedrock、Vertex，以及未来的 Foundry。
- 第三方或内部的 “coding plan” 部署，通过 `ANTHROPIC_BASE_URL` 暴露 Anthropic 兼容的 Messages API 形态，并使用 token 式鉴权，类似于 `docs/reference/anthropic_coding.md`。

该计划有意不把 Anthropic 兼容的 coding 部署视为 OpenAI 兼容 provider。它们共享 Anthropic Messages 请求与流式结构，因此需要 Anthropic 协议处理，并结合各自 provider 的鉴权、beta、缓存、模型与能力规则。

## 参考资料

- `docs/reference/anthropic/api/overview.md`
- `docs/reference/anthropic/prompt_cache/overview.md`
- `docs/reference/anthropic_coding.md`
- `docs/mvp-optimization-plans/MVP-001-api-providers-plan.md`
- `docs/IMPLEMENTATION_GAPS.md`
- `claude-code-bun/src/services/api/claude.ts`
- `claude-code-bun/src/utils/api.ts`
- `claude-code-bun/src/utils/betas.ts`
- `claude-code-bun/src/services/api/promptCacheBreakDetection.ts`
- `claude-code-bun/src/utils/forkedAgent.ts`
- `claude-code-bun/src/utils/model/model.ts`
- `claude-code-bun/packages/@ant/model-provider/src/providers/*/modelMapping.ts`

## 当前风险

- `cc-auth` 能区分 API key 与 bearer token，但 `ApiProvider::Anthropic` 仅存储一个 `api_key` 字符串。结果是 `ANTHROPIC_AUTH_TOKEN` 和 OAuth access token 可能被当作 `x-api-key` 发送，而不是 `Authorization: Bearer ...`。
- 直接 Anthropic API、自定义 Anthropic 兼容基地址、Bedrock、Vertex 与 Foundry 还未针对 beta 头、prompt 缓存模式、token 计数、请求限制和鉴权类型做精确能力拆分。
- `ANTHROPIC_BASE_URL` 可以路由到第三方 Anthropic 兼容端点，但项目未对该模式提供清晰的 “coding plan” 契约。
- 模型默认配置仍在部分位置引用上游家族名。cc-rust 的公共模型策略应使用 `SOTA`、`MOTA`、`FOTA`，并在内部通过这些别名映射到显式 provider model ID。
- Prompt caching 会发送 beta 头并记录缓存使用，但请求体还未输出顶层或块级 `cache_control`。
- 计划原先把官方文档中的顶层 automatic caching 放得过靠前；Bun 主路径实际使用显式 cache breakpoints：系统提示分块、消息级单 marker、可选 scope/ttl、以及 fork 场景的 marker 位移。cc-rust 若以 Bun parity 为目标，应先实现这个显式机制，再考虑顶层 automatic caching。
- `skip_cache_write` 不能简单解释为“省略所有缓存写 marker”。Bun 的 `skipCacheWrite` 用于 fire-and-forget fork，会把消息级 `cache_control` 从最后一条消息移动到倒数第二条消息，指向父请求共享前缀，避免 fork tail 写入新的 cache entry。
- 流式解析目前会忽略 Anthropic SSE 的 `error` 事件。
- 真实 provider 与 mock provider 的覆盖率仍落后于能力矩阵。

## 目标契约

### 鉴权

| 来源 | Provider 模式 | Header |
| --- | --- | --- |
| `ANTHROPIC_API_KEY` | 直接 Anthropic API key | `x-api-key` |
| Keychain `cc-rust/api-key` | 直接 Anthropic API key | `x-api-key` |
| `ANTHROPIC_AUTH_TOKEN` | Bearer/coding-compatible token | `Authorization: Bearer ...` |
| OAuth access token | Bearer token | `Authorization: Bearer ...` |
| Bedrock | Bedrock 鉴权适配器 | Bedrock bearer 或 SigV4 |
| Vertex | Vertex 鉴权适配器 | Google bearer token |

### 模型默认值

cc-rust 统一模型层级：

- `SOTA`：最高能力层。
- `MOTA`：平衡默认层。
- `FOTA`：快速层。

计划中的标准环境变量：

- `ANTHROPIC_DEFAULT_SOTA_MODEL`
- `ANTHROPIC_DEFAULT_MOTA_MODEL`
- `ANTHROPIC_DEFAULT_FOTA_MODEL`

兼容迁移的回退变量：

- `ANTHROPIC_DEFAULT_OPUS_MODEL` 映射到 `SOTA`。
- `ANTHROPIC_DEFAULT_SONNET_MODEL` 映射到 `MOTA`。
- `ANTHROPIC_DEFAULT_HAIKU_MODEL` 映射到 `FOTA`。

这些回退变量应保留以支持迁移，但文档与新增示例应使用 SOTA/MOTA/FOTA 名称。移除的公开别名 `opus`、`sonnet`、`haiku` 必须在用户/配置入口处被拒绝。

Prompt-cache disable 变量也应同步迁移：

- 新增 `DISABLE_PROMPT_CACHING_SOTA`、`DISABLE_PROMPT_CACHING_MOTA`、`DISABLE_PROMPT_CACHING_FOTA`。
- 兼容读取 `DISABLE_PROMPT_CACHING_OPUS`、`DISABLE_PROMPT_CACHING_SONNET`、`DISABLE_PROMPT_CACHING_HAIKU`，并按 SOTA/MOTA/FOTA 映射输出迁移诊断。
- `DISABLE_PROMPT_CACHING` 继续作为全局最高优先级开关。

### Anthropic-Compatible Coding 模式

当配置了自定义 Anthropic 协议端点时，coding-compatible 模式生效，例如：

```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "provider-token",
    "ANTHROPIC_BASE_URL": "https://provider.example.com/anthropic",
    "ANTHROPIC_DEFAULT_SOTA_MODEL": "provider-coding-pro",
    "ANTHROPIC_DEFAULT_MOTA_MODEL": "provider-coding-main",
    "ANTHROPIC_DEFAULT_FOTA_MODEL": "provider-coding-fast",
    "ANTHROPIC_MODEL": "MOTA"
  }
}
```

如果 provider 期望 `/v1/messages` 与 Anthropic SSE 事件，应按 Anthropic 协议处理，不应走 `/chat/completions`。

### Prompt Cache Bun 对齐契约

cc-rust 的第一目标不是直接照搬官方顶层 automatic caching，而是先对齐 Bun 当前主路径中的显式 breakpoint 机制：

- `CacheControl` 结构支持：
  - `type: "ephemeral"`
  - 可选 `ttl: "1h"`
  - 可选 `scope: "global"`
- `scope: "global"` 需要 `prompt-caching-scope-2026-01-05` beta，并只在 direct first-party Anthropic 模式开启。Foundry、Bedrock、Vertex、自定义 Anthropic-compatible coding endpoint 默认不得发送 global scope。
- Bun 的 `cacheScope='org'` 实际序列化为普通 `{ "type": "ephemeral" }`；只有 global scope 会额外序列化 `scope`。
- 系统提示应拆成稳定块：
  - attribution/billing header 不加 cache marker。
  - CLI system prompt prefix 和其余静态块按 provider 能力加 marker。
  - first-party global cache 模式下以动态边界拆出 static/global 与 dynamic/non-cache block。
  - 存在 MCP tools 时禁用 system-prompt global cache，避免 per-user tool schema 污染全局 cache。
- 消息历史应保持每次请求最多一个 message-level `cache_control` marker：
  - 普通请求标记最后一条消息的最后一个可缓存 content block。
  - assistant thinking、redacted thinking、connector text 等不可缓存 block 不应被选作 marker。
  - `skipCacheWrite=true` 时标记倒数第二条消息，保留父前缀 cache 命中并避免 fork tail 写入。
- 需要遵守 Anthropic 4 个 cache-control breakpoint 限制；系统块、工具块、消息块和 classifier/side-query 的 marker 必须统一预算。
- 1h TTL 必须会话级稳定：
  - 资格、allowlist 或 provider opt-in 只在会话中首次求值后锁定。
  - 中途 overage/GrowthBook/settings 变化不得让 `ttl` 在 5m 与 1h 间来回切换。
- Beta headers 也要 cache-safe：
  - 动态 beta 一旦因主线程请求打开，应 sticky-on 到 `/clear` 或 `/compact`。
  - Bedrock 需要区分 header betas 与 extra body params betas。
  - Vertex count-tokens 只允许白名单 betas。
- fork/subagent 要保存 cache-safe params：system prompt、user/system context、tools、model、父消息前缀、thinking config。fork 不得随意改 `max_tokens`/thinking budget，否则会破坏 cache key。
- OpenAI/Gemini/Grok 适配层应剥离 Anthropic-only `cache_control`、`cache_reference`、`cache_edits` 等字段，除非该 provider 显式支持 Anthropic 格式。

## Phase 0 - 基线与测试桩

目标：在变更 provider 契约前冻结当前行为。

工作项：

- 梳理现有测试，位置：
  - `crates/cc-api/src/api/client/tests.rs`
  - `crates/cc-api/src/api/streaming.rs`
  - `crates/cc-auth/src/lib.rs`
  - `crates/cc-models/src/aliases.rs`
  - `crates/cc-engine/src/lifecycle/helpers.rs`
- 新增 mock HTTP fixtures，用于：
  - Anthropic API key headers。
  - Anthropic bearer headers。
  - 自定义 `ANTHROPIC_BASE_URL` 请求 URL。
  - Count-token 请求头。
  - 流式 `error` SSE 事件。
  - Prompt-cache 系统块拆分、消息级单 marker、`skipCacheWrite` marker 位移、1h TTL、global scope、beta sticky latch 与 provider gating。
- 从 Bun 侧提取最小 request-body fixture：
  - 普通主线程请求。
  - 带 MCP tools 的请求。
  - fire-and-forget fork 请求。
  - first-party global cache 请求。
  - Bedrock/Vertex 请求。
  - OpenAI/Gemini/Grok 转换后的去 Anthropic-only 字段请求。
- 记录实现前的现有失败项或编译阻塞项。

验证：

- `cargo test -p cc-auth --lib`
- `cargo test -p cc-models --lib`
- `cargo test -p cc-api api::client -- --nocapture`
- `cargo test -p cc-api api::streaming -- --nocapture`

退出标准：

- 后续阶段存在失败或被忽略的回归测试，能够证明风险点。
- 基线文档记录当前 Header、模型、缓存与流行为。

## Phase 1 - 端到端保持 Anthropic 鉴权类型

目标：对 API key 与 bearer token 正确发送官方 Anthropic 鉴权头。

工作项：

- 将 `ApiProvider::Anthropic { api_key, base_url }` 改为可保存鉴权类型的结构，例如 `AnthropicAuth::ApiKey(String)` 与 `AnthropicAuth::Bearer(String)`。
- 在 `from_auth_result` 中构建 `ApiClient` 时保留 `AuthMethod` 的类型。
- 更新非流式 Messages、流式 Messages 及 count-token 请求构建器，统一使用 Anthropic header builder。
- 将 `anthropic-version` 与按能力选择的 `anthropic-beta` 放在同一 helper 中，确保 header snapshot 测试覆盖所有 Anthropic 请求路径。
- 确保请求快照会脱敏并不持久化凭据值。

目标测试：

- `ANTHROPIC_API_KEY` 发送 `x-api-key`，且不发送 `Authorization`。
- `ANTHROPIC_AUTH_TOKEN` 发送 `Authorization: Bearer ...`，且不发送 `x-api-key`。
- OAuth bearer 鉴权走 bearer 路径。
- Count-token 与流式请求使用相同鉴权类型行为。
- 使用 `ANTHROPIC_API_KEY` 时无效的直接 Anthropic API key 仍应快速失败。

验证：

- `cargo test -p cc-auth --lib`
- `cargo test -p cc-api api::client -- --nocapture`
- `cargo test -p cc-api api::stream_provider -- --nocapture`

退出标准：

- 不得在 Anthropic 协议请求中将 bearer token 序列化为 `x-api-key`。

## Phase 2 - 增加 Anthropic-Compatible Coding Provider 契约

目标：将第三方 Anthropic 格式的 coding 部署作为一等公民的 Anthropic 协议模式支持。

工作项：

- 为自定义 Anthropic 兼容端点新增 provider 能力档案。它应与直接 Anthropic、Bedrock、Vertex、OpenAI-compatible provider 以及尚不支持的 Foundry 区分。
- 当 `ANTHROPIC_BASE_URL` 指向非 Anthropic 官方主机，或引入显式 opt-in 开关时，检测并启用该模式。
- 允许该模式通过 `ANTHROPIC_AUTH_TOKEN` 使用 bearer-token 鉴权。
- 决定是否只在该模式下允许非 `sk-ant-` 的 API key。直接 Anthropic API key 保持格式校验。
- 增加配置/状态诊断，显示 provider 为 `anthropic-compatible`（或类似）而非直接 Anthropic。
- 请求路由到 `{base_url}/v1/messages`，并解析 Anthropic SSE 事件。
- 保持 provider 能力标记显式化：
  - tool use
  - thinking
  - 精确 token 计数
  - 显式 prompt cache
  - 官方顶层 automatic prompt cache（默认关闭，独立能力）
  - advisor
  - 请求大小限制

目标测试：

- `ANTHROPIC_BASE_URL=https://provider.example/anthropic` 路由到 `https://provider.example/anthropic/v1/messages`。
- 自定义兼容模式可在不做 `sk-ant-` 校验的情况下接受 bearer 鉴权。
- OpenAI-compatible provider 仍走 `/chat/completions`。
- 不支持的能力不会出现在 beta 头与请求体中。
- 状态输出可清晰识别该自定义 provider 模式。

验证：

- `cargo test -p cc-api api::client -- --nocapture`
- `cargo test -p cc-api api::providers -- --nocapture`
- `cargo test -p cc-commands login -- --nocapture`

退出标准：

- 用户可用 `ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_BASE_URL` 与 SOTA/MOTA/FOTA 默认模型配置一个 Anthropic-format coding endpoint，且不会发送 OpenAI 风格请求。

## Phase 3 - 将家族默认值迁移到 SOTA/MOTA/FOTA

目标：让模型默认配置与 cc-rust 的中性公共别名一致。

工作项：

- 新增规范化环境变量读取：
  - `ANTHROPIC_DEFAULT_SOTA_MODEL`
  - `ANTHROPIC_DEFAULT_MOTA_MODEL`
  - `ANTHROPIC_DEFAULT_FOTA_MODEL`
- 新增 prompt-cache disable 变量读取：
  - `DISABLE_PROMPT_CACHING_SOTA`
  - `DISABLE_PROMPT_CACHING_MOTA`
  - `DISABLE_PROMPT_CACHING_FOTA`
- 保留上游家族变量的回退支持：
  - `ANTHROPIC_DEFAULT_OPUS_MODEL`
  - `ANTHROPIC_DEFAULT_SONNET_MODEL`
  - `ANTHROPIC_DEFAULT_HAIKU_MODEL`
  - `DISABLE_PROMPT_CACHING_OPUS`
  - `DISABLE_PROMPT_CACHING_SONNET`
  - `DISABLE_PROMPT_CACHING_HAIKU`
- 定义优先级：
  1. CLI `--model`
  2. config `model`
  3. `ANTHROPIC_MODEL`
  4. SOTA/MOTA/FOTA 环境映射中的选中别名默认值
  5. 内置 cc-rust 别名目标
- 当 `ANTHROPIC_MODEL` 为 `SOTA`、`MOTA` 或 `FOTA` 时，请求前按 provider 模型映射解析。
- 在请求构建前拒绝遗留公共别名 `opus`、`sonnet`、`haiku`。
- 更新：
  - `docs/reference/anthropic_coding.md`
  - `docs/claude-code-configuration/model-configuration.md`
  - `docs/COMMAND_REFERENCE.md`
  - `/model` 与 `/config show` 输出（若提及旧家族默认值）

目标测试：

- `SOTA` 在设置后通过 `ANTHROPIC_DEFAULT_SOTA_MODEL` 解析。
- `MOTA` 在设置后通过 `ANTHROPIC_DEFAULT_MOTA_MODEL` 解析。
- `FOTA` 在设置后通过 `ANTHROPIC_DEFAULT_FOTA_MODEL` 解析。
- 旧 `OPUS/SONNET/HAIKU` 环境变量仍可作为回退工作，并输出迁移提示。
- `DISABLE_PROMPT_CACHING_SOTA/MOTA/FOTA` 分别控制对应 tier。
- 旧 `DISABLE_PROMPT_CACHING_OPUS/SONNET/HAIKU` 仍可作为回退工作，并输出迁移提示。
- `availableModels` 接受别名与 provider 模型 ID（按文档）。

验证：

- `cargo test -p cc-models --lib`
- `cargo test -p cc-api api::client -- --nocapture`
- `cargo test -p cc-commands model -- --nocapture`
- `cargo test -p claude-code-rs --bin claude-code-rs startup -- --nocapture`

退出标准：

- 新文档与示例使用 SOTA/MOTA/FOTA 名称。
- 除非用户显式提供完整 provider 模型 ID，provider 请求不得收到已移除的别名。

## Phase 4 - 实现 Prompt Cache 请求体

目标：将 prompt caching 从“仅头信息支持”改为 Bun parity 的显式 breakpoint 请求体支持。官方顶层 automatic caching 作为后续可选能力跟踪，但不作为本阶段的主路径。

工作项：

- 新增 `CacheControl` 类型：
  - `type: "ephemeral"`
  - 可选 `ttl: "1h"`
  - 可选 `scope: "global"`
- 新增 cache-control 承载点：
  - system text block。
  - message content block。
  - tool schema block，仅在明确需要工具级 breakpoint 时使用。
  - 暂不默认发送顶层 `cache_control`；若后续支持官方 automatic caching，必须作为独立 capability 与 fixture 加入。
- 实现 `build_system_prompt_blocks` 对齐 Bun `splitSysPromptPrefix()`：
  - attribution/billing header 独立成块且不加 cache marker。
  - CLI system prompt prefix 独立成块。
  - 默认模式将剩余 system prompt 合并为稳定块并加普通 `{type:"ephemeral"}`。
  - first-party global cache 模式识别动态边界，将静态内容标记为 `{type:"ephemeral",scope:"global"}`，动态内容不缓存。
  - 当 MCP tools 或 per-user tool section 会污染全局缓存时，禁用 system-prompt global cache，回退普通 org/default scope。
- 实现 message-level breakpoint 对齐 Bun `addCacheBreakpoints()`：
  - 每个请求最多一个 message-level `cache_control` marker。
  - 普通请求把 marker 放在最后一条消息的最后一个可缓存 block。
  - assistant `thinking`、`redacted_thinking`、connector text 等不可缓存 block 不得作为 marker。
  - `skip_cache_write=true` 时把 marker 移到倒数第二条消息；这是 fork 共享父前缀的策略，不是移除所有 marker。
  - 消息少于两条且 `skip_cache_write=true` 时必须有明确 fallback 或禁用该选项。
- 实现 breakpoint 预算：
  - 所有系统块、消息块、工具块和 side-query/classifier marker 合计不得超过 Anthropic 的 4 个 cache-control breakpoint。
  - 超额时应 fail fast 或降级为确定性策略，并记录诊断；不得静默发送可能 400 的请求。
- 实现 1h TTL 会话稳定性：
  - `DISABLE_PROMPT_CACHING` 最高优先级。
  - SOTA/MOTA/FOTA 级别 disable 开关按 Phase 3 生效。
  - 1h TTL eligibility、allowlist、provider opt-in 在会话首次求值后锁定。
  - Bedrock 仅在显式 opt-in 时允许 1h TTL；Vertex 和 custom compatible endpoint 默认不启用 1h TTL，除非 capability 显式声明。
- 实现 beta/header 稳定性：
  - 发送 `scope:"global"` 时必须发送 `prompt-caching-scope-2026-01-05`。
  - dynamic beta header 使用 sticky-on latch，避免 fast/auto/cache-editing 等状态切换破坏 cache key。
  - Bedrock 的部分 beta 放到 extra body params，不放普通 header。
  - Vertex count-tokens 仅发送白名单 betas。
- 实现 fork/subagent cache-safe params：
  - 保存 system prompt、user context、system context、tool context、父消息前缀、model、thinking config。
  - fork 使用 exact tools 和父前缀以保证 byte-identical cache key。
  - fork 不得通过 `max_tokens` 间接改变 thinking budget，除非明确放弃 cache sharing。
- 实现 prompt-cache break detection：
  - 分别 hash 去除 `cache_control` 后的 system/tools 内容、完整 `cache_control` 形态、model、betas、effort、extra body params。
  - 对 cache-control scope/TTL flip、tool schema 变化、model 变化、beta 变化输出可诊断事件。
  - 只跟踪有限数量 query source，避免长会话或大量 subagent 造成内存增长。
- 新增 provider 能力校验：
  - 直接 Anthropic：允许显式缓存；global scope 仅 first-party direct Anthropic。
  - Anthropic-compatible coding 模式：只允许普通显式缓存，除非 provider capability 显式声明 scope/TTL/beta 支持。
  - Bedrock：允许普通显式缓存；beta/extra-body 行为按 Bedrock capability 控制；不启用官方 top-level automatic caching。
  - Vertex：允许普通显式缓存；countTokens beta 白名单独立控制；不启用官方 top-level automatic caching。
  - OpenAI/Gemini/Grok：转换层默认剥离 `cache_control`、`cache_reference`、`cache_edits` 等 Anthropic-only 字段。
  - Foundry：在 adapter 存在前维持不支持。
- 在流式与非流式聚合中保留 cache usage 字段。
- 添加请求快照脱敏，保留 cache-control 的结构形态。

目标测试：

- 系统提示按 attribution/prefix/static/dynamic 规则拆块，且 marker 数量稳定。
- first-party global scope 请求包含 `scope:"global"` 和 `prompt-caching-scope-2026-01-05`。
- MCP tools 存在时 system-prompt global scope 被禁用。
- 普通请求只有一个 message-level marker，并落在最后一条消息的最后一个可缓存 block。
- `skip_cache_write` 将 marker 移到倒数第二条消息，而不是删除所有 marker。
- assistant thinking/redacted thinking/connector text 不会被选为 cache marker。
- 超过 4 个 breakpoint 时 fail fast 或按确定性策略降级。
- 1h TTL 在会话内不会因 overage/allowlist/settings 变化发生 flip。
- Bedrock/Vertex 不发送官方 top-level automatic cache body。
- OpenAI/Gemini/Grok 转换后不携带 Anthropic-only cache 字段。
- prompt-cache break detection 能报告 scope/TTL、tool schema、model、betas、effort、extra body params 变化。
- 会话快照中导出 cache usage。

验证：

- `cargo test -p cc-engine lifecycle::helpers -- --nocapture`
- `cargo test -p cc-api api::client -- --nocapture`
- `cargo test -p cc-api api::providers -- --nocapture`
- `cargo test -p cc-api api::openai_compat -- --nocapture`
- `cargo test -p cc-session session_export -- --nocapture`

退出标准：

- prompt-cache 行为在序列化后的请求 fixture 中可见，而不只是 header 中可见。
- Bun 主路径的 system/message marker、scope/TTL、fork `skip_cache_write` 和 provider gating 都有 fixture 覆盖。

## Phase 5 - 流式错误与 Messages 表面硬化

目标：避免 Anthropic 协议偏移及隐藏运行时失败。

工作项：

- 将 Anthropic SSE `error` 事件解析为结构化错误。
- 从错误响应中保留 provider request ID（当 headers 存在时）。
- 对现代 Messages 字段增加类型化支持或明确排除：
  - `metadata`
  - `service_tier`
  - `stop_sequences`
  - `temperature`、`top_p`、`top_k`
  - `container`、`context_management`、`mcp_servers`（仅当项目决定支持这些 managed-agent 风格能力时）
- 将不受支持字段排除在请求体外，而非序列化 provider 不兼容的占位字段。
- 扩展基于 Anthropic 错误 payload 的重试分类。

目标测试：

- SSE `error` 事件会以 provider 错误详情失败流。
- `ping` 继续被忽略。
- 缺少必需 SSE 字段仍应解析失败。
- 可选 Messages 字段仅在设置时才序列化。
- 不支持的 provider 能力会阻止发送不支持字段。

验证：

- `cargo test -p cc-api api::streaming -- --nocapture`
- `cargo test -p cc-api api::client -- --nocapture`
- `cargo test -p cc-engine lifecycle::deps -- --nocapture`

退出标准：

- Anthropic 协议错误不能再消失为空的流事件。

## Phase 6 - Provider Smoke Matrix 与文档闭环

目标：缩小能力宣告与真实行为之间的差距。

工作项：

- 为直接 Anthropic、Anthropic-compatible coding、Bedrock、Vertex、以及不支持的 Foundry 增加 mock-server 测试。
- 增加凭据门控的真实 smoke 脚本（可选）：
  - 直接 Anthropic API key
  - 直接 Anthropic bearer token（若可用）
  - 自定义 Anthropic-compatible endpoint
  - Bedrock
  - Vertex
- 在以下文档中按 provider 通过情况更新说明：
  - `docs/IMPLEMENTATION_GAPS.md`
  - `docs/WORK_STATUS.md`
  - `docs/KNOWN_ISSUES.md`
  - `docs/archive/COMPLETED_FULL.md`
- 将有意不支持的 provider 行为标记为 `Intentional`，而不是静默缺失。

验证：

- `cargo build --workspace --release`
- 各阶段的目标 provider 单元测试
- 仅在存在凭据时运行真实 smoke 脚本

退出标准：

- provider 能力矩阵、文档与测试结果一致。
- Anthropic-compatible coding 配置有完整、可测试的路径，并使用 SOTA/MOTA/FOTA 默认值。

## 建议实施顺序

1. Phase 1 鉴权类型修复
2. Phase 2 Anthropic-compatible coding provider 契约
3. Phase 3 SOTA/MOTA/FOTA 模型 env 落地
4. Phase 4 prompt-cache 请求体支持
5. Phase 5 流式错误与可选 Messages 字段硬化
6. Phase 6 smoke matrix 与文档闭环

鉴权正确性应优先，因为它影响每一条 Anthropic 协议请求，并可能同时影响直接 Anthropic API 与兼容 coding 部署。SOTA/MOTA/FOTA 模型默认值应在大范围 coding-mode 文档前落地，确保新示例不再传播旧的 OPUS/SONNET/HAIKU 变量。
