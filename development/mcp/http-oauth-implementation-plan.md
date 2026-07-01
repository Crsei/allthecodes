# MCP Streamable HTTP / OAuth 实施计划

日期：2026-06-30
状态更新：2026-07-01

## 结论

allthecodes 已经实现了 MCP Streamable HTTP 和手动 OAuth 的核心基础，并补齐了大部分 HTTP 鉴权、header 合并与 session 恢复兼容性；但还没有达到 Codex 当前完整 OAuth 控制面和 token store 形态。

当前已落地（已接入构建）：

- Streamable HTTP transport，支持 JSON POST、`Mcp-Session-Id`、GET SSE、DELETE session termination：`crates/allthecodes-mcp/src/client/streamable_http.rs`
- HTTP URL/header 校验，包含静态 header、保留 header 拒绝、HTTPS/localhost/127.0.0.1 约束：`crates/allthecodes-mcp/src/client/http_utils.rs`
- MCP server 配置已包含 `type` / `url` / `headers` / `oauth` / `bearer_token_env_var` / `env_http_headers` / `oauth_resource` / `auth`，并接入 CLI、IPC 和 Web CRUD 的基本读写。
- HTTP header 合并已覆盖静态 header、环境变量 header、bearer token env var 和 OAuth token，且 `Authorization` 采用大小写不敏感覆盖逻辑。
- HTTP 401/403 会映射到 `McpAuthNeededError`，并解析 `WWW-Authenticate` challenge / `insufficient_scope`。
- Streamable HTTP 已有 active session 404 恢复、reinitialize + initialized notification + operation rerun，以及 `initialize` / `notifications/initialized` / `tools/list` 的 transient retry。
- OAuth metadata discovery、PKCE、token exchange、refresh、`resource` 参数、pending state 存储和 token file store 已存在：`crates/allthecodes-mcp/src/auth.rs`
- 斜杠命令 `/mcp auth start|complete|status|clear` 已存在，但仍是手动复制 code 的流程：`crates/allthecodes-commands/src/mcp/auth.rs`
- IPC 命令/事件 `StartAuth`、`CompleteAuth`、`ClearAuth`、`QueryAuth`、`AuthStarted`、`AuthStatus` 已存在：`crates/allthecodes-ipc-protocol/src/subsystem_events.rs`
- Web REST 的 MCP server CRUD 可以读写基础 OAuth 配置并做敏感字段脱敏：`crates/allthecodes-web/src/handlers/mcp_servers.rs`
- 2026-07-01 验证：`cargo test -p allthecodes-mcp --lib` 通过（80 passed）。

当前未完成：

- OAuth 登录仍是手动复制 code；还没有 Codex 的 loopback callback、browser launch、后台等待 callback、自动 complete 和 async completion 形态。
- 尚未实现 Codex 风格的 `mcpServer/oauth/login` 和 `mcpServer/oauthLogin/completed` 控制面；IPC 也没有 OAuth 自动完成事件。
- Web REST 只有 server CRUD，没有 `/api/mcp-servers/{name}/oauth/start|complete|status|clear`。
- token store 仍是 `~/.allthecodes/mcp-oauth.json` 文件模式；缺少 `mcp_oauth_credentials_store` / `credentials_store` 配置、keyring/auto/file 选择、keyring 服务名接入和文件权限 0600 强化。
- auth status 还不是 Codex 的 `unsupported` / `not_logged_in` / `bearer_token` / `oauth` 完整状态语义，也缺少已知 GitHub 无 OAuth 支持等用户提示。
- Phase 2 的实现已基本落地，但测试覆盖仍需补齐：DELETE 404/405、GET active-session 404 过期、`initialize` / `notifications/initialized` transient retry、CLI/Web/IPC 新字段 serde 回归。
- 尚未跑完整 acceptance：`allthecodes-ipc-protocol`、`allthecodes-commands`、`allthecodes-web`、workspace release build，以及 Web/OAuth E2E。

当前阶段判断：

- Phase 0：MCP crate regression baseline 基本就位。
- Phase 1：配置 schema / CLI / IPC / Web CRUD 大体完成。
- Phase 2：HTTP transport / auth header / auth error / session recovery 大体完成，剩余测试补齐。
- Phase 3：只落地了手动 OAuth 与文件 token store 基础；自动 OAuth login 和 token store 模式尚未落地。
- Phase 4：CLI/IPC 手动控制面部分完成；Web REST OAuth 控制面和 async completion 未完成。
- Phase 5：只验证了 MCP crate 单元测试；完整 acceptance 未完成。

注意：当前工作区存在未追踪的 `crates/allthecodes-mcp/src/oauth_store.rs` 和 `crates/allthecodes-mcp/src/oauth_login.rs` 草稿，但尚未接入 `lib.rs`，且引用了未公开函数或缺失配置字段；本文不把它们计为已完成实现。

## Codex 参考实现

参考仓库：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex`

关键文件：

- `codex-rs/rmcp-client/src/rmcp_client.rs`：用 `rmcp` 的 Streamable HTTP transport / OAuth 状态接入 MCP。
- `codex-rs/rmcp-client/src/perform_oauth_login.rs`：启动 OAuth login、生成授权 URL、等待 loopback callback。
- `codex-rs/rmcp-client/src/oauth.rs`：MCP OAuth token 读写、keyring/file fallback、刷新后持久化。
- `codex-rs/rmcp-client/src/auth_status.rs`：计算 MCP auth status。
- `codex-rs/rmcp-client/src/utils.rs`：静态 HTTP headers 与 env headers 解析。
- `codex-rs/config/src/mcp_types.rs`：`McpServerTransportConfig::StreamableHttp`、`bearer_token_env_var`、`env_http_headers`、`oauth_resource`、`McpServerOAuthConfig`。
- `codex-rs/app-server-protocol/src/protocol/v2/mcp.rs`：`mcpServer/oauth/login` 请求和 `mcpServer/oauthLogin/completed` 通知。
- `codex-rs/app-server/src/request_processors.rs`：app-server 收到 MCP OAuth login 后异步执行并发完成通知。

## 目标行为

1. `streamable-http` MCP server 能通过静态 header、环境变量 bearer token、env header 或 OAuth token 鉴权。
2. OAuth 登录可从 CLI、TUI/IPC、Web API 触发；用户打开授权 URL 后，loopback callback 自动完成 token exchange。
3. token 存储默认使用 allthecodes 隔离 namespace，优先 keyring，必要时 fallback 到 `~/.allthecodes` 文件；文件权限在 Unix 上为 0600。
4. 401/403、缺 token、scope 不足、GitHub 等不支持 OAuth 的 server 都能给出可操作状态。
5. 所有新增字段和行为不读取或写入 `~/.codex`，不使用 Codex keychain service；仍遵守 allthecodes 的路径隔离。

## 阶段计划

### Phase 0：基线测试与行为冻结

- 给现有 `crates/allthecodes-mcp` 增加最小回归测试：
  - Streamable HTTP `initialize` 捕获 `mcp-session-id`。
  - GET SSE 不支持时接受 404/405。
  - DELETE session 不支持时接受 404/405。
  - 401/403 进入 `auth-needed`。
  - OAuth token refresh 成功后写回 store。
- 目标：先锁住现有实现，后续改动不把已支持能力打坏。

### Phase 1：配置模型补齐

改动范围：

- `crates/allthecodes-mcp/src/lib.rs`
- `crates/allthecodes-types/src/mcp.rs`
- `crates/allthecodes-ipc-protocol/src/subsystem_types.rs`
- `crates/allthecodes-commands/src/mcp/flags.rs`
- `crates/allthecodes-commands/src/mcp/config.rs`
- `crates/allthecodes-web/src/handlers/mcp_servers.rs`

任务：

- 扩展 MCP server 配置：
  - `bearerTokenEnvVar` / `bearer_token_env_var`
  - `envHttpHeaders` / `env_http_headers`
  - `oauthResource` / `oauth_resource`
  - `auth`：先支持 `oauth`，预留 `chatgpt`
- CLI 增加：
  - `--header=K=V`
  - `--bearer-token-env-var=ENV`
  - `--env-http-header=Header=ENV`
  - `--oauth-resource=<url>`
  - `--auth=oauth|chatgpt`
- Web CRUD 和 IPC config entry 要完整 round-trip 新字段。
- 对 headers/env 做 redaction，避免把 Authorization 或环境变量值透出到 UI/日志。

### Phase 2：HTTP transport 兼容性补齐

改动范围：

- `crates/allthecodes-mcp/src/client/http_utils.rs`
- `crates/allthecodes-mcp/src/client/auth_error.rs`
- `crates/allthecodes-mcp/src/client/streamable_http.rs`
- `crates/allthecodes-mcp/src/client/mod.rs`
- `crates/allthecodes-mcp/src/client/client_tests.rs`

任务：

- 统一 header 构建与覆盖语义：
  - static `headers`
  - `env_http_headers` 从环境变量取值
  - `bearer_token_env_var` 解析出的非空 token 生成 `Authorization: Bearer ...`
  - OAuth token 仅在没有显式 Authorization、没有有效 bearer env token 时注入
- header 合并要按 HTTP header 名大小写不敏感地覆盖，不能在构建后按字符串排序：
  - env header 与 static header 同名时，env header 覆盖 static header。
  - bearer env / OAuth 注入的 `Authorization` 覆盖点必须显式，避免同名 header 因大小写差异重复发送。
- 对 `env_http_headers` 使用和静态 headers 相同的保留头校验；同样拒绝 `accept`、`content-type`、`mcp-session-id`、`mcp-protocol-version` 等 transport 管理头。
- `bearer_token_env_var` 行为与 Codex 对齐：
  - 配置了 env var 但变量未设置、为空或非 Unicode 时直接报错，不静默降级到无鉴权。
  - 只有成功解析出非空 token 时才阻止 OAuth token fallback。
- 解析 Streamable HTTP 401/403 的 `WWW-Authenticate`：
  - 401 + `WWW-Authenticate` 映射为 auth required，并保留原始 challenge，方便 CLI/TUI/Web 提示登录。
  - 403 + Bearer `error="insufficient_scope"` 映射为 insufficient scope，并解析可用的 `scope` 参数。
  - 其他 401/403 继续进入 `auth-needed`，但错误类型要能携带 status/header/detail，而不是只保存 status。
- `server 不支持 OAuth` 不从 `WWW-Authenticate` 推断；放到 auth status / metadata discovery 路径处理：
  - OAuth metadata discovery 返回 no authorization support 时状态为 `unsupported`。
  - GitHub 等已知不支持 OAuth 的 server 在状态/错误文案中给 bearer token 配置指引。
- Streamable HTTP session 失效处理与 Codex/rmcp 对齐：
  - POST 带 session id 返回 404 时识别为 session expired，不当作普通 HTTP 404。
  - GET SSE 带 session id 返回 404 时也识别为 session expired；405 才表示 server 不支持 GET SSE。
  - session expired 后通过一个 recovery lock 串行化恢复，重建 transport，重新执行 initialize + initialized notification，再恢复当前 client 状态。
- retry 语义拆开处理：
  - transient HTTP retry 只用于 initialize / initialized notification / `tools/list`，状态码覆盖 408、429、500、502、503、504，延迟使用 Codex 当前 `[250ms, 1000ms]`。
  - session expired 404 是 recovery + rerun 当前 operation，不等同于 transient retry；工具调用是否会被 rerun 必须按 Codex 当前 `run_service_operation` 行为实现并在测试中锁住。
- 保持现有 loopback HTTP 允许、远程 HTTP 禁止的安全策略。
- Phase 2 完成时必须补本阶段测试，不推迟到 Phase 5：
  - static/env/bearer/OAuth header 优先级与大小写覆盖。
  - `bearer_token_env_var` 未设置、空值、非 Unicode 报错。
  - 401 `WWW-Authenticate`、403 `insufficient_scope`。
  - GET 405 no-SSE、GET 404 session expired。
  - POST 404 session recovery。
  - initialize / initialized notification / `tools/list` transient retry。

### Phase 3：OAuth 自动登录与 token store

改动范围：

- `crates/allthecodes-mcp/src/auth.rs`
- 新增可选模块：`crates/allthecodes-mcp/src/oauth_store.rs`、`crates/allthecodes-mcp/src/oauth_login.rs`
- 配置入口：`crates/allthecodes-config`

任务：

- 将当前 `start_authorization` / `complete_authorization` 保留为手动 fallback。
- 新增自动 flow：
  - 生成 PKCE/state。
  - 启动 loopback callback server。
  - 返回授权 URL。
  - 后台等待 callback、校验 state、交换 token、写入 store。
  - timeout 后清理 pending state。
- 支持 `oauth_resource`，授权和 token request 都带 `resource`。
- 增加 `mcp_oauth_credentials_store = "auto" | "file" | "keyring"`：
  - 默认 `auto`。
  - keyring service/namespace 使用 allthecodes，不能使用 Codex。
  - file fallback 仍在 `~/.allthecodes`，写入时 Unix 权限 0600。
- 做一次兼容迁移：已有 `mcp-oauth.json` 可继续读取；写回时转入新 store 结构。

### Phase 4：CLI / IPC / Web 控制面

改动范围：

- `crates/allthecodes-commands/src/mcp/auth.rs`
- `crates/allthecodes-ipc-protocol/src/subsystem_events.rs`
- `crates/allthecodes/src/app_subsystem_handlers/mcp.rs`
- `crates/allthecodes-web/src/handlers/mcp_servers.rs`
- `crates/allthecodes-web/src/handler_registry.rs`

任务：

- CLI：
  - `/mcp auth start <name>` 默认走自动 callback，并显示授权 URL。
  - `/mcp auth complete` 保留给无法接收 callback 的 server。
  - `/mcp auth status` 输出 Codex 风格状态：unsupported / not_logged_in / bearer_token / oauth。
- IPC：
  - 保留现有 `StartAuth` / `CompleteAuth` / `ClearAuth` / `QueryAuth`。
  - 新增 OAuth completed 事件，或扩展 `AuthStatus` 带 `success/error`，用于前端异步完成提示。
- Web REST：
  - `POST /api/mcp-servers/{name}/oauth/start`
  - `POST /api/mcp-servers/{name}/oauth/complete`
  - `GET /api/mcp-servers/{name}/oauth/status`
  - `DELETE /api/mcp-servers/{name}/oauth`
- 如果后续要暴露 Codex app-server protocol，再单独加 `mcpServer/oauth/login` 兼容方法，避免混进现有 REST 语义。

### Phase 5：测试和验收

单元测试：

- 配置 serde：新字段 JSON round-trip，stdio 上拒绝 HTTP-only 字段。
- header 解析：静态 header、env header、bearer token、OAuth token 优先级。
- OAuth store：auto/file/keyring 分支，file mode 0600，旧 `mcp-oauth.json` 兼容读取。
- Auth status：unsupported、not logged in、bearer token、OAuth、expired refreshable。

集成测试：

- 本地 loopback MCP + OAuth metadata server：
  - 未登录返回 401。
  - start OAuth 返回 URL。
  - callback 自动完成 token exchange。
  - reconnect 后带 Authorization。
  - access token 过期后 refresh。
- Streamable HTTP：
  - `initialize`、`notifications/initialized`、`tools/list`。
  - server 支持和不支持 GET SSE 两种分支。
  - session id 失效后的恢复。
  - DELETE session 成功、404、405。

Web/API 测试：

- MCP CRUD round-trip 新字段。
- OAuth start/status/clear API 不泄漏 token。
- 错误响应区分 validation、not found、auth unsupported、OAuth timeout。

构建验收：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-mcp
cargo test -p allthecodes-ipc-protocol
cargo test -p allthecodes-commands
cargo test -p allthecodes-web
cargo build --workspace --release
```

## 建议提交顺序

1. 配置 schema + serde/CLI/Web CRUD round-trip。
2. header resolution + bearer/env header transport 支持。
3. token store mode + 0600 + 旧 store 兼容。
4. loopback OAuth login + async completion event。
5. auth status / `WWW-Authenticate` / scope error 补齐。
6. Streamable HTTP session recovery。
7. Web/TUI polish 和完整 e2e。

## 风险点

- 是否直接引入 `rmcp`：Codex 直接依赖 rmcp，allthecodes 当前是手写 transport。若引入 rmcp 会减少协议维护成本，但会扩大依赖和重构面；建议先用测试锁住行为，再决定是否替换底层 transport。
- 工具调用 recovery：Codex 当前在 session expired 404 后会 reinitialize 并 rerun 当前 operation；这可能让有副作用的 `tools/call` 重复执行。allthecodes 若对齐此行为，必须用测试锁住，并在错误/日志中能区分 transient retry 与 session recovery rerun。
- ChatGPT auth：这是 Codex 的一等分支，但 allthecodes 当前没有对应 MCP first-party provider 语义；先预留配置和状态，除非明确要接 OpenAI first-party MCP，否则不在第一批强行实现。
