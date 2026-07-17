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

> **目录来源说明（catalog provenance）**：上表与各模型支持的 reasoning
> 档位来自随二进制分发的内置快照（
> `allthecodes_config::settings::providers::codex_capability_entries()`），
> 不是发起方对 OpenAI API 的实时查询结果。换版/新增模型只能通过升级
> 本仓库二进制完成。
> `BUNDLED_CODEX_CATALOG_VERSION` / `BUNDLED_CODEX_CATALOG_UPDATED` 两个常量
> 标注当前快照版本，`/login`、`/model`、`/fast` 在运行时把当前二进制的快照
> 刷新进当前 profile，不会沿用旧 settings 中残留的旧目录。
> TUI effort picker 的来源标签会显示 `Bundled levels`、`Configured levels`
> 或 `Supported levels` 以区分快照源、用户覆盖与未知。

GPT-5.6 模型支持 `low`、`medium`、`high`、`xhigh` 和 `max` reasoning effort。
Codex 的 `ultra` 是多代理编排模式，不作为普通 `reasoning.effort` 暴露。

## Effort / Reasoning 语义

Codex 走 `reasoning.effort` 传输路径（而非 Anthropic 的
`thinking.budget_tokens` 或 `output_config.effort`）。allthecodes 内的解析
优先级（自上而下，命中即停）：

1. **本轮 override**：`SubmitMessageOverrides.effort` 注入到
   `app_state.effort_value`，本轮 wire 优先据此构造
   `reasoning.effort`，下一轮恢复 session/profile 基线
2. **profile baseline**：`authProfiles.<active>.modelReasoningEffort`
3. **bundled default**：`ModelCapabilitySettings.default_reasoning_level`
4. **`auto`**：清除显式值，不发任何档位猜测

变更行为约定：

- `/effort <level>` 对 Codex 路径只写 `authProfiles.<active>.modelReasoningEffort`，
  **不再**向 root `output_config.effort` 写值，避免 Anthropic-only 字段污染 Codex
  profile。
- `/effort auto` 删除 profile 上的显式覆盖，但 **不**在 settings 中写入默认档，
  避免"当前用户没设但被静默持久化为 medium"的漂移。
- 模型切换后，若显式档位不被当前 capability 支持，会回退到 `auto` 并给出可见提示，
  不会静默发送 capability 集合外的档位。
- `reasoning_tokens` 仅作为响应 usage 的事后统计展示，不会反向声明模型上限。

## Historical Note

旧的“如何基于外部资料重构 Codex 接入方式”笔记已归档到
[`archive/implemented/codex-agent.md`](../implemented/codex-agent.md)。
