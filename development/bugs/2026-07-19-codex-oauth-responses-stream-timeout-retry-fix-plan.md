# Codex OAuth Responses 长流超时与断流恢复修复计划

日期：2026-07-19

状态：Active (Reopened)

2026-07-20 复核：本文件是该问题的唯一权威计划。原始实施使用
`worktree/codex-stream-recovery-cli-contract`，artifact 固定为
`development/worktree-workflow-artifacts/2026-07-20-codex-stream-recovery-cli-contract.html`。
后续工具状态、会话恢复和收尾卡死修复使用
`worktree/codex-cli-session-tool-state-hardening`，artifact 固定为
`development/worktree-workflow-artifacts/2026-07-20-codex-cli-session-tool-state-hardening.html`。不创建同主题
sibling plan。

### 2026-07-20 实施记录

实现提交从 `ffe0870c` 到 `f95e920e`，证据 artifact 为 `12eae743`。最终实现包括 provider
recovery policy、Codex Responses 原始 SSE frame idle、`response.completed` 唯一成功边界、typed
stream failure、同模型 reconnect、completed 前工具零执行、重复 tool ID 拒绝、partial tombstone、三类
recovery 计数与 SDK/TUI/session report 可观测性，以及 print/json stdin 和 stderr 契约。每次 Codex
请求建立前还会重新走现有 OAuth resolver；刷新失败分类为不可重试的 `AuthenticationFailed`，不泄露旧
token。

主分支验证通过 `cargo fmt --all --check`、workspace clippy `-D warnings`、配置/API/engine/session/Web/CLI/UI
定向测试和 `cargo build --workspace --release`。可控本地 SSE 验收证明：第一次完整 tool block 后 EOF，
第二次 completed，第三次最终完成；只出现一次 stream reconnect、一个 tool result 和三次 POST。CLI 进程级
smoke 覆盖 positional、stdin、JSON stdin、空输入和 max-turns stderr。

真实 OAuth 默认配置验收确认新 release 使用 `openai-codex / codex / gpt-5.6-sol`，且所有 timeout/stall
环境覆盖均未设置；session `cee7800a-ccd1-4bc8-b4f5-9511658e8d8b` 的前三轮模型调用成功。该次银河任务随后
停在工具结果持久化后的工具收口/刷新路径，另一次无工具尝试停在 query 启动前，因此没有把它们误记为
“新 release 单次真实 SSE 超过 120 秒”的通过证据。历史成功 session
`0812dfa1-bb43-4553-adcb-bdc87b84ca6c` 的网页由 Playwright 重新验收为 HTTP 200、交互正常、
console/page/failed-request 全为 0；长流协议边界由缩放自动化和可控 SSE fixture 覆盖。完整命令、计数、
限制和 release provenance 均记录在 artifact。

### 2026-07-20 重开证据

原始 Codex Responses 长流修复仍有效，但真实 CLI 验收暴露了后续工具与会话状态缺陷，
`PROVIDER-002` 因此重开：

1. Session `5a1a8694-a5d5-4723-bf6e-255364f291c4`：`TaskUpdate` 已完成，但 tool result 未在进程
   退出前可靠刷入 canonical rollout/session，query 停在工具后的刷新/收尾窗口。
2. Session `155e5992-0936-474c-bc4b-a93f39597383`：resume 向 provider 发送了缺少匹配 tool output
   的历史并收到 HTTP 400；该 synthetic API error 因含文本被误判为成功，污染 stdout 且进程 exit 0。
3. Session `d4fe751e-becd-482a-8855-367219b20609`：未设置 timeout/stall 环境变量时，同一
   `gpt-5.6-sol` 模型调用约 218 秒后成功，证明长流 timeout/completion 修复本身有效。
4. Session `ce228c8e-0f76-41ca-a9ee-985c082d4b33`：同一 submit 共 26 个 tool turn，尾部
   反复 `Read -> Edit`；`Read` 的已读状态没有进入后续 `Edit` 所见的 session-owned cache，
   最终两次 Edit 均错误返回 `File has not been read yet`。

本轮不回退已完成的 SSE/retry 契约；重开范围仅补齐工具结果耐久性、resume 协议修复、
跨 turn 文件状态、工具错误循环保护以及非交互退出语义。

问题域：OpenAI Codex OAuth、Responses API、SSE、超时、重试、工具执行幂等性

Codex 对照基线：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex`，分支
`feat/disable-websockets`，commit `e9641ad51`。2026-07-20 另以本机已安装的
`codex-cli 0.144.6` 对应官方 tag `rust-v0.144.6`（commit `5d1fbf26c`）复核工具执行时序。

## 1. 目标

- 修复 `gpt-5.6-sol` 等长推理模型在 Codex OAuth 路径中被 60 秒 progress stall 和 120 秒 HTTP
  总超时错误中断的问题。
- 对齐 Codex 当前 Responses 流行为：请求建立重试与已建立流重连分层；不以固定总时长终止仍有机会完成的流；
  仅在 SSE 空闲超过阈值、流在 `response.completed` 前关闭或收到明确服务端错误时进入恢复。
- 将 `response.completed` 建立为 Codex Responses 成功的唯一完成边界，禁止 EOF 被伪装成正常
  `MessageStop`。
- 对可恢复断流执行同模型重连；Codex Responses 在 `response.completed` 前只积累工具块、不启动本地工具，
  从源头消除重连重复副作用，而不是在本任务中引入跨 attempt 的 in-flight drain/ledger。
- 复用已有 `SystemSubtype::ApiError` / `SdkMessage::ApiRetry` / TUI spinner 通道，让用户看到
  `Reconnecting...` / `Retrying (n/N)...`，而不是等待后直接收到终止错误。
- 保留 Anthropic、OpenAI Chat Completions、Google、Bedrock、Vertex 等非 Codex provider 的现有协议语义；
  共享传输层变化必须有针对性回归测试，不能用 Codex 修复改变其它 provider 的完成判定。
- 工具完成后先耐久化 tool result，再进入下一模型 turn；MCP 刷新忙时使用当前快照，
  不得让全局 manager 锁阻塞 query 收尾。
- resume 前修复 orphan tool call/result 协议，但不自动重放已执行工具。
- 将 Read/Edit 安全前置状态收敛到 session-owned cache，并对反复的同类工具验证失败 fail closed。
- 统一 print/JSON/resume/max-turns/provider/tool-loop 的 `SdkResult` 错误与进程退出契约。

## 2. 已确认的现场证据

### 2.1 2026-07-20 最新复核

1. Session `0359e5bd-15cf-43c5-917e-b668f8e35baf` 在第五次模型调用中因 120 秒
   semantic idle 超时失败。
2. 设置 `ALLTHECODES_STREAM_IDLE_TIMEOUT_MS=300000` 后，Session
   `0812dfa1-bb43-4553-adcb-bdc87b84ca6c` 完成真实网页构建，证明 300 秒 idle policy 能覆盖该类
   长推理，而不是依赖降低 reasoning effort。
3. 当前公开 `StreamEvent` 的所有变体都属于有效进度，因此 60 秒 stall watchdog 的 non-progress 分支不可达；
   该 watchdog 对 Codex 没有独立语义价值，应删除，而不是继续调高阈值。
4. `--print` 帮助声称支持 stdin，但当前实现会在没有位置参数时提前拒绝，尚未读取非 TTY stdin。
5. `--resume --max-turns 4` 的 exit 1 是正常的 max-turns 终止，不是 resume 加载失败；真正缺陷是
   print mode 没有把非空 `SdkResult.result` 写到 stderr，导致错误原因静默丢失。

### 2.2 2026-07-19 初始现场

现场 session：`506cbdda-5836-4a4a-ae79-2222d480b9ec`。

1. 首次提交从 `2026-07-18T18:31:53.205Z` 到 `18:34:52.186Z`，总耗时 `180239ms`。
2. 最后一次工具 `Write` 只运行 `45ms`；随后模型请求从 `18:33:35.954Z` 运行到
   `18:34:52.137Z`，请求耗时 `76183ms`，最终报：

   ```text
   stream stalled for 75230ms without progress
   ```

3. 用户输入“继续”后只发生一次模型请求，从 `18:35:28.854Z` 到 `18:37:28.872Z`，
   `120017ms` 时精确失败：

   ```text
   error reading OpenAI response chunk
   ```

4. 同一 session 在失败前已有 8 次 `gpt-5.6-sol` 模型请求成功，OAuth、模型 ID、请求 URL 与基本
   SSE 解析均可工作。
5. 当前 Codex profile 使用 `modelReasoningEffort=high`。75 秒无可见文本不代表连接失效；长推理模型可能在
   `response.created` 之后较长时间才产生 reasoning summary、正文或工具调用。
6. 首次 `worked for 180s` 是整个 submit 的累计耗时，不是工具耗时；错误发生时没有前台工具在阻塞模型流。
7. 现场没有同模型 stream retry。现有 `max_retries=3` 只覆盖 `StreamProvider::stream()` 返回流之前的错误；
   `bytes_stream()` 已建立后的 chunk error 直接进入终止路径。容量 fallback 只识别 529/overloaded/high demand，
   不处理普通断流。

## 3. Codex 当前实现基线

### 3.1 provider 级配置与默认值

`../codex/codex-rs/model-provider-info/src/lib.rs`：

- `DEFAULT_STREAM_IDLE_TIMEOUT_MS = 300_000`；
- `DEFAULT_STREAM_MAX_RETRIES = 5`；
- `DEFAULT_REQUEST_MAX_RETRIES = 4`；
- `stream_idle_timeout_ms` 与 `stream_max_retries` 是 provider 级配置；
- stream/request retry 上限均限制为 100，避免无界配置。

OpenAI provider 没有为 Responses 请求设置总时限。`ModelProviderInfo::to_api_provider()` 只把 provider
配置转换为请求重试策略和 stream idle timeout。

### 3.2 HTTP 请求不携带流总时限

`../codex/codex-rs/codex-api/src/provider.rs` 构造的普通 Responses `Request.timeout` 为 `None`。
`../codex/codex-rs/login/src/auth/default_client.rs` 使用标准 `reqwest::Client::builder()`，没有设置全局
120 秒 total timeout。只有明确需要总时限的非流式端点才在单个 `Request` 上设置 timeout。

这与“无限等待”不同：连接建立错误由 request retry 处理，已经建立的流由独立 SSE idle watchdog 处理。

### 3.3 SSE idle 与完成边界

`../codex/codex-rs/codex-api/src/sse/responses.rs`：

- 每次等待下一条 SSE event 时执行 `timeout(idle_timeout, stream.next())`；
- 任意合法 SSE event 都会结束本次 idle wait；idle 衡量的是传输事件活动，不是“是否出现可见文本”；
- SSE 解析/网络错误产生 `ApiError::Stream`；
- EOF 且尚未收到 `response.completed` 产生
  `stream closed before response.completed`；
- 收到 `response.completed` 后才向上层发送 `Completed` 并正常结束；
- 服务端 rate limit error 可以携带 requested retry delay。

Codex 没有第二个 60 秒“可见内容 progress stall”阈值，因此
`response.created -> 75s reasoning -> output delta` 不会在 delta 到达时反向判死。

### 3.4 两层重试

1. `codex-client` request retry：在 HTTP/SSE handshake 建立前处理网络错误、timeout、5xx 等，使用指数退避
   和 jitter。
2. `core/src/responses_retry.rs` stream retry：握手成功后、`response.completed` 前的可恢复错误最多重连
   5 次，尊重服务端建议 delay，并向客户端发送 `Reconnecting... n/N`。

`core/src/session/turn.rs::run_sampling_request()` 在同一 turn 内重试；首次失败后从 session 当前 history
重新构建 prompt。response item 会先记录，已确认的工具调用及其结果因而可以进入下一次 prompt。

### 3.5 Codex 工具执行时序复核与本项目决策

`codex-cli 0.144.6` 的实际实现不是在 `response.output_item.done` 到达时立即执行工具：

1. `handle_output_item_done()` 先持久化 tool call，再构造一个尚未被轮询的冷 future。
2. `try_run_sampling_request()` 把该 future 放入 `FuturesOrdered`，继续读取 Responses stream；入队本身不会启动
   async tool body。
3. 收到 `response.completed` 时 stream loop 以成功结束；若提前 EOF/transport error，则以错误结束。
4. 两种结果都会在离开 stream loop 后无条件调用 `drain_in_flight()`；工具真正从这里开始执行，结果随后写入
   conversation history。
5. 若前一步是可恢复断流，`run_sampling_request()` 再从更新后的 history 构造 prompt 并重连。

因此，Codex 采用的是“冷 future 队列 + 流结束后 drain + history 续传”的混合方案：它没有在
`response.completed` 前并发执行工具，但会在未完成响应已经断流后执行已闭合的工具。该实现降低了普通自动重连的
重复执行概率，却不是进程崩溃场景下的 durable exactly-once ledger；副作用完成后、tool output 持久化前崩溃仍有
不可证明窗口。

allthecodes 当前 streaming gate 开启时会在 `ContentBlockStop` 处直接 `tokio::spawn` 工具，断流路径只
`abort()` task，无法撤销已经发生的副作用。为降低本次修复复杂度和安全风险，本项目明确选择更保守的 provider
边界：**仅对 Codex Responses 延迟本地工具执行到 `response.completed`；本任务不移植 Codex 的断流后
drain/history ledger。** 延迟代价预计为几十毫秒到数秒，优先换取可证明的“未完成 attempt 零工具副作用”。

### 3.6 Codex 已有测试证据

- `core/tests/suite/stream_no_completed.rs`：第一次 SSE 未发 `response.completed` 就关闭，第二次返回完整流，
  断言发生两次 `/responses` POST 且 turn 成功。
- `core/tests/suite/websocket_fallback.rs`：覆盖 retry 通知与 transport fallback。
- `codex-api/tests/sse_end_to_end.rs`：覆盖 Responses SSE item 和 completed 解析。
- `core/tests/suite/stream_error_allows_next_turn.rs`：失败 turn 必须释放运行状态，下一次提交仍可完成。
- 当前 `stream_no_completed.rs` 的首个未完成事件不包含合法 tool item；Codex 上游没有直接覆盖“合法
  function call 已 done、随后断流”的执行次数断言。本项目必须补上该安全边界测试，不能只依赖源码推断。

## 4. allthecodes 当前差距

| 边界 | allthecodes 当前行为 | Codex 行为 | 风险 |
| --- | --- | --- | --- |
| HTTP total timeout | `ApiClient::try_new()` 给共享 reqwest client 设置 120s total timeout | Responses 请求默认无 total timeout | 正常长推理在 120s 被切断 |
| stream idle | query loop 等待下一条已映射 `StreamEvent` 120s | 等待下一条 SSE event 300s | 被 parser 忽略的 event 不能刷新 allthecodes idle |
| progress stall | 60s；下一条 progress 到达后才反查间隔并报错 | 无第二层可见内容 stall | 75s 后到达的有效 delta 被主动丢弃 |
| EOF | Codex parser 在 EOF 补 `MessageDelta(end_turn)` + `MessageStop` | 未见 `response.completed` 即错误 | 截断回答可能被当成成功 |
| chunk error | `anyhow::Context("error reading OpenAI response chunk")`，错误类型被字符串化 | typed stream error | UI 看不到 timeout/network/early EOF 的真实分类 |
| request retry | 只覆盖 `StreamProvider::stream()` 建立失败，默认 3 retries | provider request retry，默认 4 | 建立前能力接近但配置不可见且错误分类较弱 |
| stream retry | 无同模型重连；只对容量错误跨模型 fallback | 默认 5 次同 turn reconnect | 短暂代理/TLS/SSE 中断直接终止 |
| retry UI | 已有 `ApiRetry` 消息与 TUI spinner，但 query stream retry 未使用 | 主动显示 reconnect 次数 | 用户只能看到长等待和终止错误 |
| 工具幂等 | gate 开启时在 `ContentBlockStop` 直接 `tokio::spawn`；断流只 abort，已发生副作用无法撤销 | `output_item.done` 只入冷 future；completed 或断流后才 drain，记录输出并从新 history 重试 | 直接增加 retry 会重复工具副作用；照搬 drain/history 又会显著扩大状态机 |

### 4.1 重开后的工具/会话差距

| 边界 | 当前问题 | 目标 |
| --- | --- | --- |
| 工具后刷新 | 工具执行后重复 `refresh_tools()`，可等待全局 MCP manager 锁 | 下一模型请求前是唯一正常刷新点；忙时非阻塞使用快照 |
| tool result 耐久性 | 内存消息、rollout 与 session projection 是分散副作用 | canonical rollout 先 append+flush；成功后才继续或降级 projection 错误 |
| resume 完整性 | orphan assistant tool call 可直接发给 provider | 优先用 legacy projection 补回，否则插入确定性 synthetic error，不重放工具 |
| 文件已读状态 | `ToolUseContext` 临时副作用在 turn 之间丢失 | 所有 turn/deferred execution 共享 session-owned `FileStateCache` |
| 错误循环 | 模型可无界重复相同 validation failure | 相同 tool+输入摘要+验证错误第 3 次触发 `tool_error_loop` |
| CLI 成功判定 | 有文本的 API error assistant block 可被当作正常 result | `base_success && !is_api_error_message`；plain stderr/exit 1，JSONL 保留错误但 exit 1 |

## 5. 目标行为契约

### 5.1 Codex Responses timeout

1. OpenAI Codex SSE 请求不得继承 reqwest client 的固定 total timeout。
2. HTTP handshake 继续有独立、有限的 request timeout；该 timeout 只包围 `send()` 到响应头建立，不包围
   `Response::bytes_stream()` 的整个生命周期。
3. Codex stream idle 默认 `300000ms`，可以通过 active auth profile 的 provider 配置覆盖。
4. idle 在 API/SSE 边界按“下一条 SSE event”重置；reasoning、output item、usage、rate limit、未知但合法的
   Responses event 都是传输活动。不能只用 UI 可见 `ContentBlockDelta` 重置。
5. 删除 Codex 路径的 60 秒 semantic progress stall。若其它 provider 暂时保留 legacy stall，必须明确隔离，
   不得再次作用到 `openai-codex`。
6. `response.completed` 是成功终止；EOF、chunk error、idle timeout、`response.failed` 都不能合成正常
   `MessageStop`。

### 5.2 retry 配置

在 `authProfiles.<name>` 增加明确字段，并进入 settings schema、merge、effective runtime 与验证：

```json
{
  "requestMaxRetries": 4,
  "streamMaxRetries": 5,
  "streamIdleTimeoutMs": 300000,
  "requestTimeoutMs": 120000
}
```

- Codex provider 默认采用上面的值；现有配置没有这些字段时自动获得新默认，不要求用户迁移。
- `requestMaxRetries`、`streamMaxRetries` 范围 `0..=100`。
- timeout 必须大于 0，并设置合理上限以避免单位错误；实现时在 schema、validation 与运行时采用同一边界。
- `requestMaxRetries`、`streamMaxRetries` 的统一合法范围是 `0..=100`；`streamIdleTimeoutMs`、
  `requestTimeoutMs` 的统一合法范围是 `1..=3600000`。
- 覆盖优先级固定为 `ALLTHECODES_STREAM_IDLE_TIMEOUT_MS` →
  `CC_RUST_STREAM_IDLE_TIMEOUT_MS` → active profile → provider 默认值。
- 现有 `ALLTHECODES_STREAM_IDLE_TIMEOUT_MS` 可作为兼容/诊断覆盖，但 profile 值应成为正常持久化入口；
  `CC_RUST_*` 只保留历史兼容，不在新文案中推荐。
- `ALLTHECODES_STREAM_STALL_TIMEOUT_MS` 和对应 legacy 变量不再控制 Codex；删除无实际作用的 semantic
  stall watchdog。非 Codex provider 保留现有默认值与完成协议。

### 5.3 request retry 与 stream retry 分离

- request retry 只处理尚未返回可消费 stream 的网络错误、request timeout、429/5xx/529 等；保留当前
  `messages_stream_with_backoff()`，但改用 typed category、provider 配置和有 jitter 的退避。
- stream retry 只处理已建立流后的 transport error、idle timeout、early EOF、可恢复 5xx/overload stream
  error；认证、invalid request、context window、policy、quota 不重试。
- stream retry 使用同一模型和同一 turn，不能偷换 fallback model。容量 fallback 是重试预算耗尽后的独立策略，
  且继续遵守已有签名清理规则。
- retry 计数必须写入 `RequestStartEvent.attempt/is_retry`、runtime record、audit event 和现有 `ApiRetry`
  UI 通道；最终 session report 能区分 request retry、stream retry 和 model fallback。
- 退避采用封顶指数退避和 jitter；若 typed stream error 携带 retry-after，则优先使用服务端建议值。
- 用户取消、goal pause、预算终止期间不得进入下一次 retry；sleep 必须可取消。

### 5.4 partial output 与工具幂等

每个流尝试建立 `StreamAttemptState`（名称可调整），至少跟踪：

- assistant UUID 与 accumulator；
- 是否收到 `response.completed`；
- 已向 UI 发送的 text/reasoning/tool block；
- 已闭合但尚未执行的工具调用及其原始顺序；
- 本次 attempt 是否允许安全重放。

provider 边界与恢复规则：

1. 仅 Codex Responses 使用 completion barrier。`response.output_item.done` 可以形成 UI/accumulator 中的完整
   tool block，但不得调用 `StreamingToolExecutor::add_tool_use()`、`tokio::spawn` 或任何 canonical tool
   execution 入口。
2. 只有收到 `response.completed` 并完成本次 attempt 校验后，才把已积累工具按原顺序交给现有 post-stream
   批处理；现有 concurrency-safe batch 与 serial barrier 语义保持不变。
3. EOF、idle timeout、transport error、decode failure 或 `response.failed` 发生在 completed 前时，本 attempt
   的本地工具执行次数必须为 0。向 UI 发 tombstone 撤销孤立 partial assistant/tool block，再用原请求同模型重试。
4. 只产生 text/reasoning partial 时同样不得伪装为 `end_turn`；失败 attempt 的 partial 内容不得进入最终
   transcript、session replay 或下一次模型上下文。
5. 同一成功 attempt 内相同 `tool_use_id` 最多执行一次；保留轻量去重断言，但不为未启动工具建立跨 attempt
   in-flight ledger。
6. Anthropic、OpenAI Chat Completions、Google、Bedrock、Vertex 等非 Codex provider 不受该 completion
   barrier 影响，原 streaming tool execution 契约保持不变。
7. 若未来要恢复 Codex 的 pre-completed speculative execution，必须作为独立任务设计 durable ledger、崩溃恢复、
   已完成副作用记账和 cancellation output；不得在本修复中隐式扩展。

这部分不能以“当前 streaming tool gate 默认关闭”为理由省略；必须在 gate 开启时证明 Codex Responses 仍受
completion barrier 约束。这样本任务无需处理“已经启动但未完成”的工具，因为该状态在 completed 前按契约不可达。

### 5.5 错误与可观测性

- API 层提供 typed stream failure，例如 `Transport`、`IdleTimeout`、`IncompleteResponse`、
  `ProviderFailed`、`Decode`，保留原始错误链但不包含 token/header secret。
- 用户最终错误至少包含 provider、model、attempt、elapsed、失败类别和可操作提示；不得只显示
  `error reading OpenAI response chunk`。
- audit/session report 记录每次尝试的 duration、retry delay、error category、是否已产生 partial output、
  是否有 started/completed tool。
- TUI 在退避期间显示 `Retrying (n/N)...`，下一条 event 到达后恢复正常 streaming 状态；重试成功不追加
  一条永久红色 API error 消息。
- retry 耗时计入 submit 总耗时，但 UI/日志同时保留每次 request duration，避免再次把 `worked for` 误读为
  工具耗时。

### 5.6 非交互 CLI 契约

- print/json mode 共用一处 prompt 解析：位置参数存在时优先；否则 stdin 非 TTY 时读取并 trim；stdin 为 TTY
  或 trim 后为空时立即给出明确错误并 exit 1。
- `run_print_mode` 继续只把正常助手正文写到 stdout；最终 `SdkResult.is_error=true` 时，把非空
  `SdkResult.result` 写到 stderr 并 exit 1。
- `--resume` 成功加载会话后若命中 `--max-turns N`，stderr 必须显示
  `Reached maximum of N turns`，不得误报成 resume 加载失败。

### 5.7 工具收尾与 MCP 刷新

- 删除工具执行后的重复 `refresh_tools()`；下一轮模型调用前的刷新是唯一正常刷新点。
- MCP manager 忙时使用 `try_lock`，立即沿用当前工具快照；刷新结果显式分为
  `ToolRefreshOutcome::{Fresh, CachedBusy, CachedError}`。
- 记录刷新耗时与安全降级原因，并发布 `tool_results_durable`、`tool_refresh_cached`、
  `next_turn_ready` 事件，消除工具完成后的不可观测空窗。

### 5.8 Tool result 强持久化与 resume

- 将 assistant/tool-result 的内存更新、rollout record 和 session projection 纳入异步
  `SubmitTransaction`。
- 工具完成后先把 result append 并 flush 到 canonical rollout，再允许下一次 provider 请求。
  canonical 写入失败时 fail closed，不得继续或重执行工具；rollout 已落盘后的
  session/transcript projection 失败可降级告警。
- 每个 tool-result user message 立即保存 session，不等待下一条 assistant message。
- resume 前检查每个 assistant tool call 在下一模型请求前恰有一个匹配 result。rollout
  缺失时优先从 legacy session projection 恢复精确结果；仍缺失时插入 `is_error=true`
  的确定性 synthetic result，声明上次结果因中断不可用。两种情况都不自动重放原工具，
  且在 provider 调用前将修复后完整消息投影追加为 rollout snapshot。

### 5.9 Read/Edit 会话状态与循环保护

- 增加内部 `FileStateReceipt`，只含规范化路径、resolved path、内容 hash 和 mtime，不携带文件内容。
- `Read`、`Edit`、`Write`、`HashEdit`、`NotebookEdit` 返回 receipt；canonical tool executor 负责提交到
  session-owned `FileStateCache`。所有 turn 和 deferred tool execution 共享同一 cache handle，删除临时
  `ToolUseContext` 的双重更新。
- Edit 继续执行 hash/stale-read 校验；文件外部修改后必须要求重新 Read。
- submit 级按 tool、输入摘要和 validation error 构建安全指纹；第 3 次相同失败以
  `tool_error_loop` 终止。审计只记录摘要 hash，不记录敏感输入。

### 5.10 SDK 与非交互退出语义

- 最终成功条件是 `base_success && !is_api_error_message`；不再用“有非空文本”将 synthetic API error
  误判为成功。
- plain print mode 不把 `is_api_error_message` assistant block 写入 stdout；最终安全错误仅写 stderr
  并 exit 1。JSON mode 继续输出完整 JSONL，但 error result 必须 exit 1。
- `--continue`、`--resume`、max-turns、provider 400 与 `tool_error_loop` 共用同一 `SdkResult`
  错误语义。不增加公开配置，`SdkResult` wire shape 保持兼容。

## 6. 实施阶段

### 阶段 A：先建立确定性失败测试

1. 在 `allthecodes-api` 增加本地 SSE server/fixture，按 gate 控制 event 时间和连接关闭方式，不访问真实 API。
2. 用 paused Tokio time 或毫秒级测试 policy 证明：
   - `response.created` 后超过 legacy stall 阈值才到 output delta，只要未超过 idle 就成功；
   - 总流时长超过旧 total timeout、期间持续有 SSE event 时成功；
   - 超过 stream idle 时产生 typed `IdleTimeout`；
   - EOF 未发 `response.completed` 时产生 `IncompleteResponse`，不产生成功 `MessageStop`；
   - chunk transport error 保留底层类别。
3. 在 engine mock 中先固定 early EOF/chunk error 会触发同模型第二次 request，第二次 completed 后 turn 成功。
4. 增加 retry exhaustion、用户取消 during backoff、non-retryable auth/invalid request 不重试测试。
5. 增加 completion barrier 测试：合法 tool item done 后、completed 前执行次数保持 0；completed 后执行一次；
   tool item 后直接 EOF/transport error 时首次 attempt 执行次数为 0，重试成功后总执行次数为 1。
6. 固定 retry UI event 的 attempt/max/delay/error category，并证明 session 最终仍只产生一次正常完成状态。

### 阶段 B：配置与传输策略分层

涉及路径：

- `crates/allthecodes-config/src/settings/providers.rs`
- `crates/allthecodes-config/src/settings/load.rs`
- `crates/allthecodes-config/src/settings/schema.rs`
- `crates/allthecodes-config/src/settings/tests.rs`
- `crates/allthecodes-api/src/api/client/types.rs`
- `crates/allthecodes-api/src/api/client/builder.rs`
- `crates/allthecodes-api/src/api/provider_runtime.rs`

步骤：

1. 引入 provider stream policy 类型，集中保存 request timeout/retry 与 stream idle/retry，避免继续散落
   `max_retries: 3` / `timeout_secs: 120`。
2. 将 active profile 的四个字段解析为 typed policy；对 Codex 使用 4/5/300000/120000 默认，对其它 provider
   显式保留当前默认，避免无意改变。
3. 移除 `ApiClient::try_new()` 的 reqwest 全局 `.timeout(...)`。
4. 在每个 request 建立点仅对 `send()`/响应头阶段施加 request timeout；stream body 交给 SSE idle watchdog。
5. 保留 proxy、CA、user-agent 行为，增加代理下长流测试，防止修复绕过现有 `proxyUrl`。
6. 清理或真正接入当前 `ProviderEndpoint.request_timeout/stream_idle_timeout`，禁止出现第二套声明但不生效的
   policy。

### 阶段 C：Codex Responses SSE 完成契约

涉及路径：

- `crates/allthecodes-api/src/api/openai_compat/chat_stream.rs`
- `crates/allthecodes-api/src/api/openai_compat/codex.rs`
- `crates/allthecodes-api/src/api/stream_provider.rs`
- `crates/allthecodes-api/src/api/streaming.rs`

步骤：

1. 在 API 边界按完整 SSE frame 计 idle，而不是在 engine 按映射后的 `StreamEvent` 计时。
2. Codex parser 显式跟踪 created/completed/failed；只有 completed 才发最终 stop。
3. EOF、idle、chunk error 转 typed stream failure；删除 Codex EOF 自动补 `end_turn` 的逻辑。
4. 保留 reasoning summary、output text、function/custom tool、usage 的现有映射，并为合法未知 Responses event
   保持连接活动但不污染上层消息。
5. `response.failed` 提取 code/message/retry delay；明确 retryable 与 terminal 类别。
6. 不把 Chat Completions 的 `[DONE]` 终止规则错误套到 Codex Responses；两个 parser 继续保持协议边界。
7. 向 engine 保留明确的 Codex Responses provider/completion provenance，使 completion barrier 不依赖模型名、
   stop reason 字符串猜测或当前 gate 默认值。

### 阶段 D：同模型 stream retry 与 attempt 状态

涉及路径：

- `crates/allthecodes-api/src/api/client/messages.rs`
- `crates/allthecodes-api/src/api/retry.rs`
- `crates/allthecodes-engine/src/query/recovery.rs`
- `crates/allthecodes-engine/src/query/loop_impl.rs`
- `crates/allthecodes-engine/src/query/loop_helpers.rs`
- `crates/allthecodes-engine/src/query/tests/recovery_tests.rs`
- `crates/allthecodes-engine/src/query/tests/flow_tests.rs`

步骤：

1. 将 request-start retry 和 stream retry 分成两个计数器及记录字段。
2. Codex 路径移除 semantic stall；其它 provider 若保留则通过 policy 明确选择。
3. 在 Codex Responses 路径建立 completion barrier：stream 接收期间只积累 tool block；completed 后才进入现有
   tool batch。不要在 `ContentBlockStop` 处创建 streaming executor task。
4. stream failure 后完成 attempt 收口：确认工具执行计数为 0、partial tombstone、audit finish，然后重放同一请求；
   不实现 streaming tool drain/cancel 或跨 attempt tool result ledger。
5. retry 前刷新 token/OAuth 的逻辑必须走现有 auth resolver；认证失败不消耗 stream retry 预算。
6. stream retry 耗尽后再评估现有 capacity fallback；普通 transport error 不应偷偷换模型。
7. 删除 Codex chunk error 的 partial-success 特例；若 Anthropic generic chunk partial acceptance 仍保留，增加 provider
   边界测试并单独记录其语义。

### 阶段 E：UI、SDK、记录与错误文案

涉及路径：

- `crates/allthecodes-types/src/message.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- `crates/allthecodes/src/ui/tui/engine_events.rs`
- `crates/allthecodes-session/src/record_replay/`
- `crates/allthecodes-observability/`

步骤：

1. 优先复用现有 `SystemSubtype::ApiError`、`SdkMessage::ApiRetry`，只在表达不了 retry category 时扩字段，
   不创建平行 UI 通道。
2. retry 通知作为 transient 状态；最终失败才产生持久 API error assistant message。
3. runtime execution record 与 session report 增加 request/stream/fallback 三类 attempt 统计。
4. 错误展示使用完整安全错误链，区分 `idle timeout waiting for SSE`、
   `stream closed before response.completed`、transport timeout、proxy disconnect 和 provider failure。
5. TUI 单元/快照只验证现有 spinner 文案和状态恢复；本任务不改布局，不应为了 retry 引入新的 PTY 快照 churn。

### 阶段 F：文档、artifact 与收口

1. 在 `development/archive/KNOWN_ISSUES.md` 追加用户可感知问题、临时规避和修复状态；落地后更新为已修复并写验证证据。
2. 如旧的 60/120 秒逻辑属于历史简化实现，在 `development/archive/IMPLEMENTATION_GAPS.md` / 对应当前状态索引中完成
   TODO -> full 迁移，不能只改代码不更新状态。
3. 生成
   `development/worktree-workflow-artifacts/2026-07-20-codex-stream-recovery-cli-contract.html`，记录问题、Codex
   对照、改动、commit 和验证结果。
4. 实施完成后在本计划顶部追加实施记录；本文件继续作为唯一修复计划，不另建同主题 sibling plan。

### 阶段 G：非交互 CLI 修复

涉及启动参数解析、print/json runner 及其定向测试。集中实现位置参数/stdin 优先级、TTY/空输入错误，以及
`SdkResult.is_error` 的 stderr/exit contract；覆盖位置参数、stdin、JSON stdin、resume 成功和 max-turns 可见错误。

### 阶段 H：非阻塞工具刷新与耐久事务

1. 删除 post-tool 重复刷新，用 typed outcome 固定 fresh/cached-busy/cached-error 三条路径。
2. 引入 `SubmitTransaction`，固定 assistant/tool result 的 rollout-first 顺序和 fail-closed 边界。
3. 增加 durability/refresh/next-turn 事件及持锁不阻塞回归。

### 阶段 I：resume 协议修复

1. 在 session resume 进入 provider 前运行 tool call/result 完整性检查。
2. 分别覆盖 rollout 完整、projection 补回、双方都缺失的 synthetic error 三条路径。
3. 断言修复前后工具执行计数不增加，且修复快照先耐久化再请求 provider。

### 阶段 J：文件状态与工具循环保护

1. 定义 receipt/cache 并由 canonical executor 统一提交，在同一 submit 所有 turn 及 deferred
   execution 间共享。
2. 回归 `Read -> Edit` 一次成功、外部变更后 stale 拒绝和所有文件写工具 receipt 语义。
3. 实现 submit 级失败指纹，三次相同 validation failure 终止为 `tool_error_loop`。

### 阶段 K：SDK/CLI 错误语义与证据收口

1. 收紧 SDK 成功判定，统一 plain/JSON/resume/continue/max-turns/provider/tool-loop 退出契约。
2. 增加进程级 stdout/stderr/exit-code 回归，保持 `SdkResult` wire shape。
3. 生成本轮 artifact，执行自动化与真实 OAuth/Playwright 验收，然后才关闭重开项。

## 7. 验证矩阵

### 7.1 API/配置层

- profile 四字段 JSON round-trip、merge precedence、schema 和 validation。
- Codex 默认 policy 为 request retries 4、stream retries 5、idle 300000ms。
- 自定义 `0` retries 能禁用相应层；超过 cap 被拒绝或稳定 clamp，行为与文档一致。
- reqwest client 不再携带 stream total timeout；request header handshake timeout 仍生效。
- proxyUrl、HTTP_PROXY 与 OAuth header 在 retry 后仍存在，日志不泄露 token。

### 7.2 SSE 层

- created 后长 reasoning silence 小于 idle：成功。
- 多次非可见 Responses event 能刷新 idle。
- 总时长大于 120s但每次 idle 小于阈值：成功；自动化使用缩放 policy，不真实等待 120s。
- idle 超时：一次 typed error。
- EOF without completed：retryable incomplete error。
- completed 后服务端关闭：成功且不重试。
- malformed JSON、response.failed、chunk network error 分类准确。

### 7.3 engine/retry 层

- early close -> 同模型 retry -> success。
- chunk error -> backoff -> success。
- max retries exhaustion -> 单一最终错误，attempt 数准确。
- auth/invalid/context/quota -> 0 stream retry。
- cancel during request/backoff -> 立即停止，无下一次 POST。
- partial text/reasoning 被 tombstone，不进入下一次 context。
- Codex 合法 tool item done、尚未 completed -> 工具执行次数为 0；completed 后执行次数变为 1。
- Codex tool item 后 early EOF/chunk error -> 失败 attempt 工具执行次数为 0；重试 completed 后总执行次数为 1。
- completed 后多个工具仍遵守并发 batch、serial barrier、原始顺序与同 attempt tool ID 去重。
- retry 成功后的 transcript/replay 不含孤立失败 attempt 或失败 attempt 的 tool block。
- 非 Codex provider 的 streaming tool execution 行为保持现状。
- 失败 turn 释放 busy 状态，下一次“继续”可正常提交。

### 7.4 分层命令

实施时遵守仓库测试 SOP，不以全仓 PTY 作为第一道门：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-codex-stream-recovery
export PATH="$CARGO_HOME/bin:$PATH"

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p allthecodes-config --lib settings
cargo test -p allthecodes-api --lib
cargo test -p allthecodes-engine --lib query::tests::recovery_tests
cargo test -p allthecodes-engine --lib query::tests::flow_tests
cargo test --workspace --exclude allthecodes --lib
cargo build -p allthecodes --release
cargo build --workspace --release
git diff --check
```

只有本任务实际改变 PTY 可见交互且非 PTY 测试不足时，最后再运行：

```bash
cargo test -p allthecodes --test pty_tui_e2e -- --test-threads=1
```

全仓 `cargo test --workspace` 不作为第一轮验证，完整运行最多 2 次；若失败，按 crate 定位并记录证据。

### 7.5 真实 OAuth smoke

自动化通过后，用当前 OAuth profile 做一次受控 smoke：

1. `gpt-5.6-sol` + `high` reasoning；
2. 生成持续超过 120 秒但仍有 SSE 活动的任务，确认不再被 total timeout 杀死；
3. 在可控本地代理中断一次连接，确认出现 retry UI 且同一 turn 恢复；
4. 检查 session events：attempt、duration、retry category 正确，无 token/header；
5. 工具 smoke 使用只读、可计数 fixture，确认 completed 前执行次数为 0、retry 后总执行次数为 1，不用真实
   破坏性工具验证幂等性。

### 7.6 重开项验证

- Engine：工具结果后持有 MCP manager 锁不得阻塞下一 turn；必须继续使用缓存工具。
- Durability：在 tool result 已执行、下一 assistant 前模拟崩溃，rollout 必须已含 result。
- Resume：完整 rollout、projection 补回、synthetic error 三路径都无 orphan，且工具执行数不增加。
- File tools：同 submit 跨 turn `Read -> Edit` 成功且只修改一次；外部修改后 Edit 拒绝；
  三次相同 validation failure 触发 guard。
- CLI：provider 400、resume 错误、max-turns 与 tool loop 均覆盖 plain stdout/stderr/exit code 和
  JSONL error exit 1。
- 回归：保留既有 Codex SSE、stream retry、completion barrier 及非 Codex provider 测试。

主分支自动化通过后，使用 `gpt-5.6-sol` 且不设置 timeout/stall 环境变量完成：

1. 空目录自主完成银河系网页构建和浏览器验收，不限制 Task/Read/Edit 等正常工具；
2. CLI 完成一次 favicon `Read -> Edit`，最多 3 个工具 turn 且只修改一次；
3. 中断一个工具已完成但尚未进入下一模型调用的受控 session，resume 无 400 且不重复执行工具；
4. Playwright 确认 HTTP 200、桌面/移动交互、键盘焦点、reduced-motion 及
   console/page/failed-request 全为 0。

## 8. 实施提交与 worktree 工作流

### 8.1 原始长流修复（已完成）

1. 本计划先在主分支 `allthecodes` 单独提交。
2. 从包含本计划的主分支 HEAD 创建：

   ```bash
   git worktree add -b worktree/codex-stream-recovery-cli-contract \
     .worktrees/codex-stream-recovery-cli-contract allthecodes
   ```

3. worktree 使用独立 `CARGO_TARGET_DIR=.../.tmp/atc-codex-stream-recovery`。
4. 推荐提交拆分：
   - `Define Codex stream recovery policy`
   - `Enforce Codex Responses completion semantics`
   - `Retry interrupted Codex streams safely`
   - `Fix non-interactive prompt and error output`
   - `Document Codex recovery evidence`
5. 每次只暂存本阶段明确路径；HTML artifact 跟随最后一个实施提交。
6. 完成后回主树执行 `git merge --ff-only worktree/codex-stream-recovery-cli-contract`，验证、推送
   `origin allthecodes`，再移除 worktree 和分支。

### 8.2 工具状态、resume 与 CLI 收尾加固（本轮）

1. 先在主分支单独提交本计划重开和 `PROVIDER-002` 状态更新。
2. 从该 HEAD 创建：

   ```bash
   git worktree add -b worktree/codex-cli-session-tool-state-hardening \
     .worktrees/codex-cli-session-tool-state-hardening allthecodes
   ```

3. 所有代码、测试、文档与 artifact 修改/提交只在该 worktree 内完成；worktree 内禁止运行任何
   Rust 构建、测试、clippy 或 Rust 测试二进制。
4. artifact 固定为
   `development/worktree-workflow-artifacts/2026-07-20-codex-cli-session-tool-state-hardening.html`。
5. 建议提交顺序：
   - `Make tool refresh non-blocking`
   - `Persist tool results before continuation`
   - `Repair resumable tool histories`
   - `Preserve file read state across turns`
   - `Enforce non-interactive failure exits`
   - `Record real CLI recovery evidence`
6. 回主分支 `git merge --ff-only worktree/codex-cli-session-tool-state-hardening`，再按 SOP 执行 Rust 验证。
   任何失败都回同一 worktree 修复、commit、再次 fast-forward 和复验。
7. 全部通过后，主分支单独将本计划改回 `Implemented`、`PROVIDER-002` 改回 `Fixed`；
   推送 `origin allthecodes` 后才移除 worktree 和分支。
8. 不触碰或暂存主工作树现有四个 UI 改动、`.spec/` 和
   `development/tui/2026-07-20-tui-panel-gap-audit-vs-claude-code-bun.md`；实施前后核对状态和指纹。

## 9. 完成标准

### 9.1 原始长流修复（已完成）

- [x] 现场两种错误都有确定性回归测试，修复前失败、修复后通过。
- [x] Codex Responses 不再受 120 秒 HTTP total timeout 约束。
- [x] Codex 不再使用 60 秒 semantic progress stall。
- [x] 300 秒默认 idle 与 5 次 stream retry 可由 active profile 配置。
- [x] EOF without `response.completed` 不会成功结束。
- [x] request retry、stream retry、capacity fallback 三层语义和计数分离。
- [x] partial text/reasoning、streaming tool 开关两种状态均有测试。
- [x] Codex Responses 在 `response.completed` 前不会启动本地工具；断流 attempt 工具执行次数为 0。
- [x] completed 后工具只执行一次，原有并发 batch、serial barrier 与结果顺序不回退。
- [x] retry 状态对 TUI/SDK 可见，最终错误包含具体类别与安全错误链。
- [x] 非 Codex provider 回归测试通过。
- [x] 分层验证、release build、真实 OAuth smoke 证据写入 artifact 和本计划实施记录。
- [x] `development/archive/KNOWN_ISSUES.md` 与历史 gap/current-status 状态同步。
- [x] fast-forward 合并、推送和 worktree 清理完成。
- [x] print/json 从非 TTY stdin 读取 prompt，空输入明确失败，print mode 的最终错误写入 stderr。

### 9.2 重开项收口条件

- [ ] post-tool 重复刷新已删除，MCP manager 忙/错误时非阻塞降级并有 typed outcome/事件。
- [ ] canonical rollout 在下一 provider 请求前已 flush tool result；rollout 失败 fail closed。
- [ ] resume 三条修复路径均保持 tool call/result 完整且不重放工具。
- [ ] session-owned file cache 支持跨 turn `Read -> Edit`，同时拒绝外部变更后的 stale edit。
- [ ] 三次相同 validation failure 终止为 `tool_error_loop`，审计不记录敏感输入。
- [ ] plain/JSON/resume/continue/max-turns/provider 400/tool loop 共用错误 `SdkResult` 语义并满足
  stdout/stderr/exit-code 契约。
- [ ] 分层 Rust 验证、release build、真实 OAuth 三步验收和 Playwright 矩阵通过。
- [ ] artifact 记录完整证据，计划改回 `Implemented`，`PROVIDER-002` 改回 `Fixed`。
- [ ] fast-forward 合并、推送、指纹复核和 worktree 清理完成。

## 10. 明确不接受的“修复”

- 只把 `timeout_secs: 120` 改成 600 或更大。
- 只建议用户把 reasoning effort 从 high 调低。
- 只提高 `ALLTHECODES_STREAM_STALL_TIMEOUT_MS`，继续保留到达 progress 时反向判死的逻辑。
- 在 EOF 时继续合成 `end_turn` / `MessageStop`。
- 对所有错误无差别重试，包含 auth、invalid request、quota、policy 或用户取消。
- 在断流后重放已经执行过的工具，或通过 abort 丢掉已完成工具结果。
- Codex Responses 在 `response.completed` 前继续 `tokio::spawn` 工具，再用 task abort 冒充副作用回滚。
- 为保留几十毫秒到数秒的 speculative tool latency，在本任务中引入跨 attempt in-flight ledger。
- 以 fallback model 代替同模型 stream reconnect。
- 仅验证短响应成功，不覆盖 >120 秒总时长、idle、early EOF 和 completed 前工具零执行四类关键边界。
