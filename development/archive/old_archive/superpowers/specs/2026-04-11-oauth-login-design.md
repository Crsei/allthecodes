# OAuth Login Design Spec

> Date: 2026-04-11 | Branch: `rust-lite`

## Overview

为 cc-rust 实现 OAuth 2.0 Authorization Code + PKCE 登录流程，支持 Claude.ai (Bearer Token) 和 Console (API Key) 两种模式。当前仅实现手动模式（打印 URL + 粘贴 code），保留浏览器自动打开接口。

## 1. OAuth Flow

```
/login → 选择方式: [1] API Key  [2] Claude.ai  [3] Console
  → 生成 PKCE (code_verifier + code_challenge + state)
  → 构造 authorization URL (含 scopes, redirect_uri=manual callback URL)
  → 打印 URL → 用户在浏览器中授权 → 粘贴 auth code
  → POST token endpoint 交换 access_token + refresh_token
  → 存入 ~/.cc-rust/credentials.json
  → (Console) 额外 POST create_api_key → 存入 keychain
```

**Claude.ai vs Console 差异**:

| | Claude.ai | Console |
|---|-----------|---------|
| Scopes | `user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload` | `org:create_api_key user:profile` |
| 登录后 | access_token 直接当 Bearer 用 | 用 access_token 调 create_api_key，得到 API Key 存 keychain |

## 2. File Structure

```
src/auth/oauth/
├── mod.rs        # run_oauth_flow() 编排入口
│                 # pub enum OAuthMethod { ClaudeAi, Console }
│                 # pub async fn open_browser(url: &str) -> Result<()>  ← 保留接口
│
├── config.rs     # 端点常量 + authorization_url() 构造
│
├── pkce.rs       # generate_code_verifier/challenge/state + base64url
│
└── client.rs     # exchange_code() / refresh_token() / create_api_key()
```

**现有文件改动**:

| 文件 | 改动 |
|------|------|
| `src/auth/mod.rs` | `resolve_auth()` 增加 OAuth token 分支；`oauth_login/refresh/logout` 调真实逻辑 |
| `src/auth/token.rs` | 移除 `#![allow(dead_code)]`；`StoredToken` 增加 `scopes` + `oauth_method` 字段 |
| `src/commands/login.rs` | 三选一菜单，调 `oauth::run_oauth_flow()` |
| `src/api/client.rs` | `from_auth()` 处理 `AuthMethod::OAuthToken` |

## 3. Constants (config.rs)

```rust
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTH_URL: &str = "https://platform.claude.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const CREATE_API_KEY_URL: &str = "https://api.anthropic.com/api/oauth/claude_cli/create_api_key";
pub const MANUAL_REDIRECT_URL: &str = "https://platform.claude.com/oauth/code/callback";

pub const CLAUDE_AI_SCOPES: &[&str] = &[
    "user:profile", "user:inference", "user:sessions:claude_code",
    "user:mcp_servers", "user:file_upload",
];
pub const CONSOLE_SCOPES: &[&str] = &["org:create_api_key", "user:profile"];
```

## 4. PKCE (pkce.rs)

- `generate_code_verifier()`: 32 random bytes → base64url (无 padding)
- `generate_code_challenge(verifier)`: SHA-256(verifier) → base64url
- `generate_state()`: 32 random bytes → base64url
- `base64url_encode(bytes)`: standard base64, 替换 `+` → `-`, `/` → `_`, 删除 `=`

依赖: `sha2`, `base64`, `rand` (需新增 `rand` 到 Cargo.toml，或用 `getrandom`)

## 5. Authorization URL (config.rs)

`authorization_url(method, code_challenge, state)` 构造:

```
{AUTH_URL}?
  code=true&
  client_id={CLIENT_ID}&
  response_type=code&
  redirect_uri={MANUAL_REDIRECT_URL}&
  scope={scopes, 空格分隔}&
  code_challenge={code_challenge}&
  code_challenge_method=S256&
  state={state}
```

## 6. Token Exchange (client.rs)

### exchange_code()

```
POST {TOKEN_URL}
Content-Type: application/json

{
  "grant_type": "authorization_code",
  "code": "{auth_code}",
  "redirect_uri": "{MANUAL_REDIRECT_URL}",
  "client_id": "{CLIENT_ID}",
  "code_verifier": "{code_verifier}",
  "state": "{state}"
}
```

Timeout: 15s

### refresh_token()

```
POST {TOKEN_URL}
Content-Type: application/json

{
  "grant_type": "refresh_token",
  "refresh_token": "{refresh_token}",
  "client_id": "{CLIENT_ID}",
  "scope": "{scopes, 空格分隔}"
}
```

### create_api_key()

```
POST {CREATE_API_KEY_URL}
Authorization: Bearer {access_token}
Content-Type: application/json
```

Response: `{ "raw_key": "sk-ant-..." }`

## 7. Data Structures

### OAuthTokenResponse (client.rs)

```rust
#[derive(Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub scope: String,
    #[serde(default)]
    pub account: Option<OAuthAccount>,
}

#[derive(Deserialize)]
pub struct OAuthAccount {
    pub uuid: Option<String>,
    pub email_address: Option<String>,
}
```

### StoredToken (token.rs, 扩展)

```rust
#[derive(Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub token_type: String,
    pub scopes: Vec<String>,
    pub oauth_method: Option<String>,  // "claude_ai" | "console"
}
```

### AuthMethod (mod.rs, 扩展)

```rust
pub enum AuthMethod {
    ApiKey(String),
    ExternalToken(String),
    OAuthToken { access_token: String, method: String },
    None,
}
```

## 8. resolve_auth() Priority

```
1. ANTHROPIC_API_KEY env           → ApiKey
2. ANTHROPIC_AUTH_TOKEN env        → ExternalToken
3. credentials.json 未过期         → OAuthToken
4. credentials.json 过期 + refresh → OAuthToken (自动刷新)
5. Keychain API Key                → ApiKey
6.                                 → None
```

过期判断: `expires_at - 300 < now` (5 分钟 buffer)

自动刷新失败时: 清除 credentials.json, fall through 到下一级。

## 9. from_auth() 处理 OAuthToken

```rust
AuthMethod::OAuthToken { access_token, method } => {
    match method.as_str() {
        "claude_ai" => {
            // Bearer token 直传
            // 需要扩展 ApiProvider 或新增变体
            Some(Self::new(ApiClientConfig {
                provider: ApiProvider::Anthropic {
                    api_key: access_token,  // 用作 Bearer token
                    base_url: None,
                },
                ..
            }))
        }
        "console" => {
            // Console 模式: 登录时已创建 API Key 存入 keychain
            // 这里 OAuth token 本身不直接使用
            // Fall through 到 keychain 读取
            None  // 让后续 keychain 路径处理
        }
        _ => None,
    }
}
```

注意: Console 模式登录时 `create_api_key` 的结果已存入 keychain，`resolve_auth()` 走到 OAuthToken 分支时实际返回的是 keychain 中的 API Key。但 credentials.json 仍保留用于 refresh。

## 10. /login Command UX

```
Select login method:
  [1] API Key (paste manually)
  [2] Claude.ai OAuth (Pro/Max subscription)
  [3] Console OAuth (API billing)

> 2

Opening Claude.ai authorization...
Please visit this URL to authorize:

  https://platform.claude.com/oauth/authorize?client_id=...&...

After authorizing, paste the code here:
> eyJhb...

Exchanging token... done.
Logged in successfully (Claude.ai).
```

## 11. /logout Extension

- 清除 `~/.cc-rust/credentials.json`
- 清除 keychain API Key
- 打印 "Logged out successfully"

## 12. Error Handling

| 场景 | 处理 |
|------|------|
| 用户粘贴无效 code | 打印 token endpoint 错误, 提示重试 |
| Token exchange 网络超时 | 15s timeout, "Network error, check connection" |
| Token exchange 401 | "Invalid authorization code, please try again" |
| refresh_token 失败 | 清除 credentials.json, 提示 `/login` |
| create_api_key 失败 | 打印错误, 保留 OAuth token |

## 13. Browser Interface (Reserved)

```rust
/// 打开浏览器访问 URL（当前未实现，保留接口）
pub async fn open_browser(_url: &str) -> Result<()> {
    // Future: use `open` crate to launch system browser
    // + spawn localhost HTTP callback server
    Ok(())
}
```

标记为 `pub` 且参数以 `_` 前缀避免 warning，函数体返回 Ok 而非 `unimplemented!()`。

## 14. Dependencies

**新增**:
- `rand = "0.8"` — PKCE random bytes 生成

**已有** (无需改动):
- `sha2`, `base64`, `reqwest`, `serde`, `serde_json`, `chrono`, `url`, `keyring`, `tokio`, `anyhow`

## 15. Tests

### Unit Tests (~10)

| 测试 | 文件 |
|------|------|
| verifier 长度 = 43 chars | `pkce.rs` |
| challenge = SHA256(verifier) base64url | `pkce.rs` |
| base64url 无 `+`, `/`, `=` 字符 | `pkce.rs` |
| state 非空且唯一 | `pkce.rs` |
| authorization_url 包含所有 query params | `config.rs` |
| claude_ai scopes 正确 | `config.rs` |
| console scopes 正确 | `config.rs` |
| token save → load 往返 | `token.rs` |
| is_token_expired 5 分钟 buffer | `token.rs` |
| resolve_auth 优先级: env > file > keychain | `mod.rs` |

### Integration Tests (2, `#[ignore]`)

| 测试 | 内容 |
|------|------|
| exchange 假 code → 401 | 验证请求格式 |
| refresh 假 token → 错误 | 验证请求格式 |
