# Codex Backend

allthecodes 的 `backend=codex` 走 OpenAI Codex provider/API 路径，而不是旧的本地
`codex exec --json` 子进程桥接。

## Enable

Use `.allthecodes/settings.json`:

```json
{
  "backend": "codex",
  "model": "gpt-5.6-sol"
}
```

Or use environment variables:

```env
CC_BACKEND=codex
CC_API_PROVIDER=openai-codex
OPENAI_CODEX_MODEL=gpt-5.6-sol
```

## Auth Resolution

按当前实现，Codex token 的解析顺序为：

1. `OPENAI_CODEX_AUTH_TOKEN`
2. `~/.allthecodes/credentials.json` 中由 `/login codex-oauth` 写入的 OAuth 凭据
3. `~/.codex/auth.json`（Codex CLI 登录态 fallback）

## Behavior

- `backend=codex` 会被规范化为 OpenAI Codex provider（`openai-codex`）
- QueryEngine、工具执行、权限与 UI 事件流仍然走 allthecodes 自己的主流程
- 只有模型请求与鉴权来源切换到 Codex backend
- ChatGPT OAuth 模式使用 Codex Responses endpoint：
  `https://chatgpt.com/backend-api/codex/responses`
- 可选环境变量：
  - `OPENAI_CODEX_BASE_URL`（默认 `https://chatgpt.com/backend-api`）
  - `OPENAI_CODEX_MODEL`（默认 `gpt-5.6-sol`）

## Models

当前 Codex 模型目录按能力从高到低包含：

- `gpt-5.6-sol`：旗舰 agentic coding 模型，也是默认模型
- `gpt-5.6-terra`：日常工作的平衡模型
- `gpt-5.6-luna`：快速、经济的模型
- GPT-5.5 及更早的兼容模型

GPT-5.6 模型支持 `low`、`medium`、`high`、`xhigh` 和 `max` reasoning effort。
Codex 的 `ultra` 是多代理编排模式，不作为普通 `reasoning.effort` 暴露。

## Historical Note

旧的“如何基于外部资料重构 Codex 接入方式”笔记已归档到
[`archive/implemented/codex-agent.md`](../implemented/codex-agent.md)。
