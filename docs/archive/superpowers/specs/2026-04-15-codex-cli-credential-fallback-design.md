# Codex CLI 凭证复用设计

> 让通过 Codex CLI 登录过的用户无需再次执行 `/login`，cc-rust 自动读取 `~/.codex/auth.json`。

---

## 1. 目标

- 自动检测：`resolve_codex_auth_token()` 在现有来源耗尽后，fallback 到 Codex CLI 的 `auth.json`
- 手动导入命令：`/login codex-cli` 验证 `~/.codex/auth.json` 是否可用并输出状态
- 不复制凭证：运行时直接读取 `~/.codex/auth.json`，不预复制到 cc-rust 的 `credentials.json`
- 刷新隔离：过期 token 自动刷新后存到 cc-rust 的 `credentials.json`，不回写 `~/.codex/auth.json`
- client_id 提取：使用 Codex CLI 内置的 client_id (`app_EMoamEEZ73f0CkXaXp7hrann`) 进行 token 刷新

## 2. 涉及文件

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `src/auth/codex_cli.rs` | **新增** | Codex CLI auth.json 解析模块 |
| `src/auth/mod.rs` | 修改 | 扩展 `resolve_codex_auth_token()` 优先级链 |
| `src/auth/oauth/client.rs` | 修改 | 新增 `refresh_token_with_client_id()` |
| `src/commands/login.rs` | 修改 | 新增 `/login codex-cli` 入口 |
| `src/auth/token.rs` | 无变更 | 复用现有 `StoredToken` / `save_token()` |

## 3. 新模块：`src/auth/codex_cli.rs`

### 3.1 常量

```rust
/// Codex CLI 内置 OAuth client_id（从 codex.exe 二进制提取）
pub const CODEX_CLI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// Codex CLI 配置目录名
const CODEX_CLI_DIR_NAME: &str = ".codex";

/// auth.json 文件名
const CODEX_CLI_AUTH_FILE: &str = "auth.json";

/// 环境变量：覆盖 Codex CLI 配置目录位置
const CODEX_HOME_ENV: &str = "CODEX_HOME";
```

### 3.2 反序列化结构

```rust
/// ~/.codex/auth.json 顶层结构
#[derive(Deserialize)]
struct CodexCliAuthFile {
    auth_mode: String,
    tokens: Option<CodexCliTokens>,
}

/// tokens 字段
#[derive(Deserialize)]
struct CodexCliTokens {
    access_token: String,
    refresh_token: Option<String>,
    account_id: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}
```

### 3.3 公开类型与函数

```rust
/// 从 Codex CLI auth.json 解析出的结构化凭证
pub struct CodexCliCredential {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub account_id: Option<String>,
    pub client_id: String,         // 固定为 CODEX_CLI_CLIENT_ID
    pub expires_at: Option<i64>,   // 从 JWT exp claim 解码
}

/// 返回 Codex CLI auth.json 的路径（如果存在）
///
/// 优先级：CODEX_HOME env > ~/.codex/
pub fn codex_cli_auth_path() -> Option<PathBuf>;

/// 读取并解析 Codex CLI 凭证
///
/// 验证规则：
/// - auth_mode == "chatgpt"
/// - tokens.access_token 非空
///
/// 返回 None：文件不存在、格式不对、auth_mode 不是 chatgpt
pub fn read_codex_cli_credential() -> Option<CodexCliCredential>;
```

### 3.4 JWT 过期时间提取

对 `access_token` 做无签名验证的 JWT payload 解码：

```rust
/// 从 JWT 的 payload 段提取 `exp` 字段（Unix timestamp）
///
/// 不做签名验证（与 openclaw 行为一致）。
/// 解码失败返回 None（token 将被视为未过期，由服务端 401 触发刷新）。
fn decode_jwt_exp(token: &str) -> Option<i64>;
```

步骤：按 `.` 分割 → 取第二段 → base64url 解码 → JSON 解析 → 读 `"exp"` 字段。

### 3.5 过期判断

```rust
/// 判断 CodexCliCredential 是否已过期（5 分钟缓冲，与 token.rs 一致）
pub fn is_credential_expired(cred: &CodexCliCredential) -> bool;
```

## 4. 重构 `resolve_codex_auth_token()` 优先级链

**文件：** `src/auth/mod.rs`

### 改动前

```
1. OPENAI_CODEX_AUTH_TOKEN env var
2. cc-rust credentials.json (oauth_method == "openai_codex")
   └─ 过期 → refresh → 存回 credentials.json
   └─ refresh 失败 → 删除 credentials.json → 返回 None
```

### 改动后

```
1. OPENAI_CODEX_AUTH_TOKEN env var                              (不变)
2. cc-rust credentials.json (oauth_method == "openai_codex")    (不变)
   └─ 过期 → refresh → 存回 credentials.json
   └─ refresh 失败 → 删除 credentials.json → fall through 到 3
3. ~/.codex/auth.json (auth_mode == "chatgpt")                  (新增)
   └─ 未过期 → 返回 access_token
   └─ 过期 → refresh (用 CODEX_CLI_CLIENT_ID) → 存到 cc-rust credentials.json → 返回
   └─ refresh 失败 → 返回 None
```

### 关键行为

- 步骤 2 refresh 失败后 **不再短路返回 None**，改为 fall through 到步骤 3
- 步骤 3 的 refresh 使用 `CODEX_CLI_CLIENT_ID`（非 `client_id_for(OpenAiCodex)`）
- 步骤 3 refresh 成功后存到 cc-rust 的 `credentials.json`（`oauth_method: "openai_codex"`），下次启动步骤 2 直接命中

### 新增内部辅助函数

```rust
/// 尝试从 Codex CLI auth.json 获取有效的 access_token。
/// 过期时自动刷新并存到 cc-rust credentials.json。
fn try_resolve_codex_cli() -> Option<String>;
```

## 5. 扩展 `oauth::client::refresh_token`

**文件：** `src/auth/oauth/client.rs`

新增一个函数，允许调用者传入 client_id：

```rust
/// 使用指定的 client_id 刷新 token（用于 Codex CLI 凭证复用）。
pub async fn refresh_token_with_client_id(
    client_id: &str,
    token_url: &str,
    refresh_tok: &str,
    scopes: &[&str],
) -> Result<OAuthTokenResponse>;
```

现有 `refresh_token()` 改为内部调用 `refresh_token_with_client_id()`：

```rust
pub async fn refresh_token(
    method: config::OAuthMethod,
    refresh_tok: &str,
    scopes: &[&str],
) -> Result<OAuthTokenResponse> {
    let client_id = config::client_id_for(method)?;
    let token_url = config::token_url_for(method);
    refresh_token_with_client_id(&client_id, &token_url, refresh_tok, scopes).await
}
```

## 6. `/login codex-cli` 命令

**文件：** `src/commands/login.rs`

### 触发方式

```
/login codex-cli     — 验证 Codex CLI 凭证可用性
```

在现有 `match args` 中新增分支：`"codex-cli" | "5"`。

### 行为

1. 调用 `codex_cli::read_codex_cli_credential()`
2. 文件不存在 → 输出 `"Codex CLI not found. Install Codex CLI and run 'codex' to log in first."`
3. auth_mode 不是 chatgpt → 输出 `"Codex CLI is using API key mode, not OAuth. Use /login 1 to paste your key."`
4. token 有效 → 输出 `"Codex CLI credentials detected and valid. cc-rust will use them automatically."`
5. token 过期但有 refresh_token → 尝试刷新 → 成功则输出确认，失败则提示重新登录

### login_menu 更新

```
[1] API Key (paste manually)
[2] Claude.ai OAuth (Pro/Max subscription)
[3] Console OAuth (API billing)
[4] OpenAI Codex OAuth (ChatGPT subscription)
[5] Import from Codex CLI (~/.codex/auth.json)
```

### /login status 更新

`codex_auth_status_text()` 新增识别来源：

```
"Authenticated: OpenAI Codex OAuth (from Codex CLI ~/.codex/auth.json)"
```

区分于已有的：
```
"Authenticated: OpenAI Codex OAuth (env OPENAI_CODEX_AUTH_TOKEN)"
"Authenticated: OpenAI Codex OAuth (stored credentials)"
```

## 7. 数据流总览

```
/login codex-cli                    resolve_codex_auth_token()
       │                                      │
       ▼                                      ▼
codex_cli::read_codex_cli_credential()   1. env OPENAI_CODEX_AUTH_TOKEN?
       │                                      │ no
       ▼                                      ▼
~/.codex/auth.json                       2. cc-rust credentials.json?
       │                                      │ no / refresh failed
       ├── auth_mode != chatgpt → fail        ▼
       ├── token valid → "OK"            3. codex_cli::read_codex_cli_credential()
       └── token expired                      │
           │                                  ├── valid → return access_token
           ▼                                  ├── expired → refresh_token_with_client_id()
     refresh_token_with_client_id()           │     │
           │                                  │     ├── ok → save to credentials.json → return
           ├── ok → save to credentials.json  │     └── err → return None
           └── err → suggest /login 4         └── not found → return None
```

## 8. 错误处理

| 场景 | 行为 |
|------|------|
| `~/.codex/auth.json` 不存在 | `read_codex_cli_credential()` 返回 `None`，静默跳过 |
| `auth.json` JSON 格式损坏 | `tracing::debug!` 记录，返回 `None` |
| `auth_mode` 不是 `"chatgpt"` | 返回 `None` |
| `tokens` 字段缺失或 `access_token` 为空 | 返回 `None` |
| JWT payload 解码失败 | `expires_at = None`，视为未过期，由服务端 401 处理 |
| refresh 网络失败 | `tracing::warn!` 记录，返回 `None`（不删除 `~/.codex/auth.json`） |
| refresh 返回 4xx | `tracing::warn!` 记录，返回 `None` |

**核心原则：** 永远不修改或删除 `~/.codex/auth.json`。所有错误场景静默降级（对自动检测路径），或输出友好提示（对 `/login codex-cli` 命令）。

## 9. 测试计划

### 单元测试 (`src/auth/codex_cli.rs`)

| 测试 | 说明 |
|------|------|
| `test_parse_valid_auth_json` | 正常 auth.json → 返回 CodexCliCredential |
| `test_parse_api_key_mode_returns_none` | auth_mode != "chatgpt" → None |
| `test_parse_missing_tokens_returns_none` | tokens 字段缺失 → None |
| `test_parse_empty_access_token_returns_none` | access_token 为空 → None |
| `test_decode_jwt_exp_valid` | 正常 JWT → 提取 exp |
| `test_decode_jwt_exp_invalid` | 非 JWT 字符串 → None |
| `test_is_credential_expired` | 过期/未过期边界测试 |
| `test_codex_cli_auth_path_with_env` | CODEX_HOME 设置时路径正确 |
| `test_codex_cli_auth_path_without_env` | 回退到 ~/.codex/ |

### 单元测试 (`src/auth/oauth/client.rs`)

| 测试 | 说明 |
|------|------|
| `test_refresh_token_delegates_to_with_client_id` | `refresh_token()` 内部调用 `refresh_token_with_client_id()` |

### 单元测试 (`src/auth/mod.rs`)

| 测试 | 说明 |
|------|------|
| `test_resolve_codex_falls_through_to_codex_cli` | credentials.json 为空时尝试 codex_cli |

### 单元测试 (`src/commands/login.rs`)

| 测试 | 说明 |
|------|------|
| `test_login_menu_contains_codex_cli_option` | 菜单包含 [5] |
| `test_login_codex_cli_no_file` | 无 auth.json → 提示安装 |

### E2E 测试

| 测试 | 说明 |
|------|------|
| `test_codex_cli_credential_auto_detection` | 放置模拟 auth.json → 启动 → 验证 `resolve_codex_auth_token()` 返回有效 token |
| `test_codex_cli_expired_token_refresh` | 放置过期 auth.json → 验证刷新流程（mock HTTP） |
