# 2026-04-15 Codex Backend + OAuth 变更记录

## 概要

本次更新完成两项核心改造：

1. `codex` backend 从本地 `codex exec` 子进程桥接，重构为 provider/API 调用路径（OpenAI Codex provider）。
2. `/login` 扩展 OpenAI Codex OAuth 登录流程（新增 `/login 4`）。

---

## 1) Codex backend 调用链重构

### 变更前

- `backend=codex` 走本地 `codex exec --json` 子进程。
- QueryEngine 中存在专门的 codex 分支和事件桥接逻辑。

### 变更后

- `backend=codex` 统一走 `ApiClient + query loop` 路径。
- 新增 provider：`openai-codex`（OpenAI-compatible 协议）。
- endpoint 对 codex 使用：`/conversation`（非 `/chat/completions`）。
- 移除旧的 `codex exec` 事件桥接实现，`src/engine/codex_exec.rs` 仅保留 backend 常量与归一化逻辑。

### 关键配置

- `CC_BACKEND=codex`
- `OPENAI_CODEX_AUTH_TOKEN`（或通过 `/login 4` 写入本地 OAuth 凭据）
- 可选：
  - `OPENAI_CODEX_BASE_URL`（默认 `https://chatgpt.com/backend-api`）
  - `OPENAI_CODEX_MODEL`（默认 `gpt-5.4`）

---

## 2) /login 扩展 OpenAI Codex OAuth

### 新增能力

- `/login` 新增选项：`[4] OpenAI Codex OAuth (ChatGPT subscription)`。
- 支持 `/login codex`（等价于 `/login 4`）。
- `/login-code` 支持处理 OpenAI Codex OAuth 流程。
- `/login-code` 可直接粘贴完整回调 URL，自动提取 `code` 参数。

### OAuth 配置（OpenAI Codex）

- 必填：
  - `OPENAI_CODEX_OAUTH_CLIENT_ID`
- 可选：
  - `OPENAI_CODEX_OAUTH_AUTH_URL`（默认 `https://auth.openai.com/authorize`）
  - `OPENAI_CODEX_OAUTH_TOKEN_URL`（默认 `https://auth.openai.com/oauth/token`）
  - `OPENAI_CODEX_OAUTH_REDIRECT_URI`（默认 `http://localhost:1455/callback`）
  - `OPENAI_CODEX_OAUTH_SCOPES`（默认 `openid profile offline_access`）

### 鉴权解析优先级（codex）

1. `OPENAI_CODEX_AUTH_TOKEN` 环境变量
2. `~/.cc-rust/credentials.json` 中 `oauth_method=openai_codex` 的 token（含自动 refresh）

---

## 3) 命令使用方式

1. 设置 `OPENAI_CODEX_OAUTH_CLIENT_ID`
2. 执行 `/login 4`
3. 浏览器完成授权后执行：
   - `/login-code <code>`
   - 或 `/login-code <完整回调URL>`
4. 使用 `backend=codex` 启动，即可直接使用 Codex provider。

---

## 4) 验证

本次改动完成后已通过：

- `cargo fmt`
- `cargo check`
- `cargo test auth::oauth::config::tests`
- `cargo test commands::login::tests`
- `cargo test commands::login_code::tests`
- `cargo test commands::tests`
- `cargo test api::client::tests -- --test-threads=1`
- `cargo test auth::tests`

---

## 5) 主要涉及文件

- `src/auth/oauth/config.rs`
- `src/auth/oauth/client.rs`
- `src/auth/mod.rs`
- `src/api/client/mod.rs`
- `src/commands/login.rs`
- `src/commands/login_code.rs`
- `src/commands/mod.rs`
- `src/auth/oauth/mod.rs`
