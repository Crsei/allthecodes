# Codex Backend

cc-rust 的 `backend=codex` 现在走 OpenAI Codex provider/API 路径，而不是旧的本地
`codex exec --json` 子进程桥接。

## Enable

Use `.cc-rust/settings.json`:

```json
{
  "backend": "codex",
  "model": "gpt-5.4"
}
```

Or use environment variables:

```env
CC_BACKEND=codex
CLAUDE_BACKEND=codex
CLAUDE_MODEL=gpt-5.4
```

## Auth Resolution

按当前实现，Codex token 的解析顺序为：

1. `OPENAI_CODEX_AUTH_TOKEN`
2. `~/.cc-rust/credentials.json` 中由 `/login 4` 写入的 OAuth 凭据
3. `~/.codex/auth.json`（Codex CLI 登录态 fallback）

## Behavior

- `backend=codex` 会被规范化为 OpenAI Codex provider（`openai-codex`）
- QueryEngine、工具执行、权限与 UI 事件流仍然走 cc-rust 自己的主流程
- 只有模型请求与鉴权来源切换到 Codex backend
- ChatGPT OAuth 模式使用 Codex Responses endpoint：
  `https://chatgpt.com/backend-api/codex/responses`
- 可选环境变量：
  - `OPENAI_CODEX_BASE_URL`（默认 `https://chatgpt.com/backend-api`）
  - `OPENAI_CODEX_MODEL`（默认 `gpt-5.4`）

## Historical Note

旧的“如何基于外部资料重构 Codex 接入方式”笔记已归档到
[`archive/implemented/codex-agent.md`](archive/implemented/codex-agent.md)。
