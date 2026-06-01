​
Langfuse 监控集成
实现状态：已完成，通过环境变量启用 依赖：@langfuse/otel、@langfuse/tracing、@opentelemetry/sdk-trace-base
​
一、功能概述
Langfuse 是一个开源的 LLM 可观测性平台，用于追踪、监控和调试 AI 应用的请求链路。CCB 通过 OpenTelemetry (OTel) 桥接层将 Langfuse 集成到查询流程中，实现：
LLM 调用追踪 — 记录每次 API 请求的模型、Provider、输入/输出、Token 用量
工具执行追踪 — 记录每个工具调用的名称、输入、输出、耗时和错误
多 Agent 追踪 — 主 Agent 和子 Agent 各自独立的 Trace 链路
数据脱敏 — 自动遮蔽敏感信息（API Key、文件内容、Shell 输出等）
​
二、启用方式
Langfuse 是开源项目，你可以 自部署（Docker / Kubernetes），也可以使用官方提供的 Langfuse Cloud 免费测试。注册后在 Project Settings → API Keys 页面获取密钥。
核心只需要三个环境变量：
环境变量	说明
LANGFUSE_PUBLIC_KEY	Langfuse 公钥（必填）
LANGFUSE_SECRET_KEY	Langfuse 密钥（必填）
LANGFUSE_BASE_URL	服务地址，默认 https://cloud.langfuse.com；自部署时改为你的地址（必填）
未配置时所有追踪函数为 no-op，零开销。
​
通过 settings.json 配置（推荐）
在 `~/.allthecodes/settings.json` 或项目 `.allthecodes/settings.json` 的 `env` 字段中添加，这样每次启动自动生效：
{
  "env": {
    "LANGFUSE_PUBLIC_KEY": "pk-xxx",
    "LANGFUSE_SECRET_KEY": "sk-xxx",
    "LANGFUSE_BASE_URL": "https://cloud.langfuse.com"
  }
}
启动时会在初始化 tracing / Langfuse 之前预加载 `settings.env`，因此这些变量可用于 Langfuse exporter 初始化。优先级为 shell 环境变量和 `.env` 优先，`settings.json` 只填补尚未设置的变量，不会覆盖已有值。

注意：仍需使用带 `telemetry` feature 的二进制，例如 `cargo build -p allthecodes --release --features telemetry`。默认构建不会启用 Langfuse。
​
其他可选参数
环境变量	默认值	说明
LANGFUSE_TRACING_ENVIRONMENT	development	环境标签，用于 Langfuse 面板筛选
LANGFUSE_FLUSH_AT	20	批量发送的 span 数量阈值
LANGFUSE_FLUSH_INTERVAL	10	定时刷新间隔（秒）
LANGFUSE_EXPORT_MODE	batched	导出模式：batched（批量）或 immediate（即时）
LANGFUSE_TIMEOUT	5	请求超时（秒）
​
四、架构
​
4.1 模块结构
当前 Rust 实现中，纯逻辑已收敛到公共 crate，运行时 OTel wiring 仍按 engine/services 边界保留：

crates/allthecodes-langfuse/src/
├── convert.rs        # 内部 Message 类型 → Langfuse OpenAI 兼容格式转换
└── sanitize.rs       # 数据脱敏（敏感字段、文件路径、工具输出）

crates/allthecodes-engine/src/services/langfuse/
├── client.rs         # engine feature 下的 Langfuse 初始化/关闭
├── tracing.rs        # engine query loop 的 Trace/Span 创建
├── convert.rs        # 对公共 convert 的 Tools 适配
└── sanitize.rs       # 对公共 sanitize 的 re-export

crates/allthecodes-services/src/langfuse/
├── client.rs         # startup 使用的 Langfuse 初始化/关闭
├── tracing.rs        # services telemetry bridge
├── convert.rs        # 对公共 convert 的 Tools 适配
└── sanitize.rs       # 对公共 sanitize 的 re-export 和 telemetry redaction 适配
​
4.2 追踪层级
Trace (Agent Span)                    ← createTrace() / createSubagentTrace()
  ├── Generation (LLM 调用)           ← recordLLMObservation()
  ├── Tool Observation (工具调用)      ← recordToolObservation()
  ├── Tool Observation (工具调用)      ← recordToolObservation()
  └── ...
​
4.3 数据流
query.ts  ──→  createTrace()           # 每个 query turn 创建根 trace
  │
  ├── claude.ts  ──→  recordLLMObservation()   # API 调用完成后记录 LLM 观察
  │
  ├── toolExecution.ts  ──→  recordToolObservation()  # 每个工具执行记录
  │
  └── query.ts  ──→  endTrace()         # turn 结束时关闭 trace

runAgent.ts  ──→  createSubagentTrace()  # 子 Agent 有独立 trace
​
五、追踪详情
​
5.1 主 Agent Trace
每次 query() 调用（即用户一次对话 turn）创建一个类型为 agent 的根 Span：
名称: agent-run 或 agent-run:<querySource>
元数据: provider、model、agentType: "main"
Session ID: 关联到 Langfuse 的 Session 功能，支持按会话聚合
​
5.2 子 Agent Trace
通过 AgentTool 启动的子 Agent 创建独立 Trace：
名称: agent:<agentType>
元数据: provider、model、agentType、agentId
独立于主 Trace，有自己的 Session 关联
​
5.3 LLM Generation
每次 API 调用记录为一个 generation 类型的 Span：
名称: 按 Provider 映射（如 ChatAnthropic、ChatOpenAI、ChatBedrockAnthropic 等）
记录内容: 输入消息、输出消息、Token 用量（input/output）
时间: OTel span 记录 start/end；TTFT 同时写入 metadata 的 `ttftMs` 和 `completion_start` span event
Provider 名称映射：
Provider	Generation 名称
anthropic	ChatAnthropic
bedrock	ChatBedrockAnthropic
vertex	ChatVertexAnthropic
azure	ChatAzureOpenAI
azure-foundry / microsoft-foundry / foundry	ChatAzureOpenAI
openai	ChatOpenAI
openai-codex	ChatOpenAI
gemini	ChatGoogleGenerativeAI
​
5.4 工具执行
每个工具调用记录为一个 tool 类型的 Span：
名称: 工具名（如 FileEditTool、BashTool）
记录内容: 输入（经脱敏）、输出（经脱敏）、toolUseId
错误标记: isError 标志 + level: ERROR
​
六、数据脱敏
所有上传到 Langfuse 的数据都会经过脱敏处理（sanitize.ts），确保敏感信息不会泄露：
​
6.1 全局脱敏（sanitizeGlobal）
Home 路径替换 — /Users/xxx → ~
敏感字段遮蔽 — 匹配 api_key、token、secret、password、credential、auth_header 等关键字的字段值替换为 [REDACTED]
​
6.2 工具输入脱敏（sanitizeToolInput）
敏感工具（Config、MCP）输入完全遮蔽。
文件工具（Read、Write、Edit、MultiEdit）会对 file_path、path、directory、output_path、input_path 路径字段做 Home 目录替换，并对其他字段执行全局脱敏。
Bash、PowerShell 和其他工具执行全局脱敏。
​
6.3 工具输出脱敏（sanitizeToolOutput）
工具	脱敏策略
Read、Write、Edit、MultiEdit	完全遮蔽，仅保留字符数：[file content redacted, N chars]
Bash、PowerShell	截断至 500 字符
Config、MCP	完全遮蔽
其他工具	原样保留
​
七、消息格式转换
convert.ts 将 CCB 内部的 Message 类型转换为 Langfuse 期望的 OpenAI 兼容格式：
输入: UserMessage | AssistantMessage[] + 可选 system prompt → { role, content }[]
输出: AssistantMessage[] → { role: 'assistant', content }
Content Block 映射:
text → { type: 'text', text }
thinking / redacted_thinking → 字符串 `[thinking redacted]`
tool_use → { type: 'tool_use', id, name, input }
tool_result → { type: 'tool_result', tool_use_id, content }
image → 占位字符串 `[image omitted]`
server_tool_use → 占位字符串 `[server tool use omitted]`

注：Langfuse 的 input/output 格式基于 OpenAI 兼容格式，不支持独立的 `type: thinking` 块；当前实现不会上传 thinking 内容，只保留脱敏占位字符串。
​
八、生命周期
初始化 — `allthecodes_services::langfuse::init_langfuse()` 在 startup logging 中调用，创建 OTel provider/exporter
运行时 — 未启用 telemetry feature 或未配置 key 时走 no-op/stub 路径
关闭 — `allthecodes_services::langfuse::shutdown_langfuse()` 在进程退出时调用，强制 flush 并关闭 provider
​
九、自部署 Langfuse
Langfuse 是开源项目，支持 Docker / Kubernetes 自部署：
docker run -d \
  --name langfuse \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://... \
  langfuse/langfuse:latest
自部署后，将 LANGFUSE_BASE_URL 指向你的实例地址即可。详见 Langfuse 自部署文档。
如果没有自部署需求，可以直接使用 Langfuse Cloud，提供免费额度可用于测试。
​
十、相关文件
文件	说明
crates/allthecodes-langfuse/src/convert.rs	Message 格式转换公共逻辑
crates/allthecodes-langfuse/src/sanitize.rs	数据脱敏公共逻辑
crates/allthecodes-engine/src/services/langfuse/tracing.rs	engine Trace/Span 创建和观察记录
crates/allthecodes-services/src/langfuse/tracing.rs	services telemetry bridge
crates/allthecodes-engine/src/query/loop_impl.rs	主查询流程中的 generation Trace 集成
crates/allthecodes-engine/src/query/loop_helpers.rs	工具执行中的观察记录
