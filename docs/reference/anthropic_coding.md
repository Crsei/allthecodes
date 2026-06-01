# Anthropic-Compatible Coding API

This page documents cc-rust's Anthropic protocol modes for coding workloads:

- Direct Anthropic API at `https://api.anthropic.com`.
- Anthropic-compatible coding endpoints selected with `ANTHROPIC_BASE_URL`.
- Cloud Anthropic transports such as Bedrock and Vertex, which keep their own
  auth and model mapping layers.

Authority references:

- Anthropic Messages API notes: [anthropic/api/overview.md](anthropic/api/overview.md)
- Prompt cache notes: [anthropic/prompt_cache/overview.md](anthropic/prompt_cache/overview.md)

`ANTHROPIC_BASE_URL` routes the Anthropic Messages API client to an
Anthropic-compatible endpoint. In compatible mode, configure provider-native
model IDs explicitly through SOTA/MOTA/FOTA defaults or set `ANTHROPIC_MODEL`
to a full provider model ID; cc-rust will not silently fall back to official
Claude model IDs for third-party endpoints.

## Direct Anthropic

Direct Anthropic supports both API-key auth and bearer-token auth:

```json
{
  "env": {
    "ANTHROPIC_API_KEY": "sk-ant-***",
    "ANTHROPIC_MODEL": "MOTA"
  }
}
```

Use `ANTHROPIC_API_KEY` for `x-api-key` auth. Use `ANTHROPIC_AUTH_TOKEN` for
`Authorization: Bearer ...` auth, including OAuth-style access tokens. Provider
diagnostics and smoke scripts must not print either value or raw auth headers.

Direct Anthropic resolves neutral aliases to official defaults when no override
is set:

| Alias | Env override | Direct Anthropic fallback |
| --- | --- | --- |
| `SOTA` | `ANTHROPIC_DEFAULT_SOTA_MODEL` | `claude-opus-4-7` |
| `MOTA` | `ANTHROPIC_DEFAULT_MOTA_MODEL` | `claude-sonnet-4-6` |
| `FOTA` | `ANTHROPIC_DEFAULT_FOTA_MODEL` | `claude-haiku-4-5-20251001` |

Legacy fallback env vars are still read only when the new env var is unset:
`ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_DEFAULT_SONNET_MODEL`, and
`ANTHROPIC_DEFAULT_HAIKU_MODEL`. Their use emits a warning without printing
the configured model value.

## Compatible Anthropic

When `ANTHROPIC_BASE_URL` points away from `https://api.anthropic.com`,
compatible coding mode is active. The endpoint is treated as Anthropic
Messages/SSE, not OpenAI `/chat/completions`.

Aliases must resolve through provider-specific env vars, or `ANTHROPIC_MODEL`
must be a full provider model ID. This prevents a third-party provider from
receiving first-party Claude defaults by accident.

```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-***",
    "ANTHROPIC_BASE_URL": "https://api.example.com/anthropic",
    "ANTHROPIC_MODEL": "MOTA",
    "ANTHROPIC_DEFAULT_SOTA_MODEL": "provider-coding-pro",
    "ANTHROPIC_DEFAULT_MOTA_MODEL": "provider-coding",
    "ANTHROPIC_DEFAULT_FOTA_MODEL": "provider-coding-fast"
  }
}
```

Explicit model IDs always win over alias defaults:

```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-***",
    "ANTHROPIC_BASE_URL": "https://api.example.com/anthropic",
    "ANTHROPIC_MODEL": "provider-coding"
  }
}
```

The compatible capability profile is conservative: Anthropic-format tools and
explicit prompt-cache markers are allowed, but first-party-only features such
as global prompt-cache scope, 1h TTL, thinking, and advisor mode are not sent
unless a future provider capability explicitly enables them.

## Bedrock And Vertex

Bedrock and Vertex are not generic `ANTHROPIC_BASE_URL` compatible endpoints.
They have native transports and provider auth:

| Provider | Selection | Auth | Model mapping |
| --- | --- | --- | --- |
| Bedrock | `CLAUDE_CODE_USE_BEDROCK=1` | `AWS_BEARER_TOKEN_BEDROCK` or AWS credentials | first-party Claude IDs map to Bedrock model IDs with region prefixes when known; Bedrock/ARN IDs pass through |
| Vertex | `CLAUDE_CODE_USE_VERTEX=1` | `CLAUDE_CODE_VERTEX_ACCESS_TOKEN`, service account, or ADC-style credentials | first-party Claude IDs map to Vertex IDs when known; Vertex `@` or `projects/...` IDs pass through |

Both providers strip request-body fields that belong only to first-party
Anthropic generation when their transport requires it, and both keep prompt
cache support behind provider capability checks.

## Prompt Cache Knobs

The current implemented prompt-cache path uses explicit Anthropic
`cache_control` markers. It does not implement top-level automatic prompt
caching as the default request shape.

Implemented knobs:

| Env | Effect |
| --- | --- |
| unset `CC_RUST_PROMPT_CACHE_TTL` / `CC_RUST_PROMPT_CACHE_GLOBAL` | keep cache markers as default 5-minute `{ "type": "ephemeral" }` markers |
| `CC_RUST_PROMPT_CACHE_TTL=1h` | add `ttl: "1h"` only when the selected provider capability allows 1h TTL |
| `CC_RUST_PROMPT_CACHE_GLOBAL=1` | add `scope: "global"` only for direct official Anthropic when capability allows global scope |
| `CC_RUST_PROMPT_CACHE_BREAK_DETECTION=1` | enable debug diagnostics for cache marker placement decisions |

Global scope is direct-Anthropic-only today. Compatible Anthropic endpoints,
Bedrock, Vertex, OpenAI-compatible, Gemini, and similar adapters must not
receive unsupported Anthropic-only cache fields unless their capability profile
explicitly allows them.

Legacy prompt-cache disable variables from upstream planning
(`DISABLE_PROMPT_CACHING_OPUS`, `DISABLE_PROMPT_CACHING_SONNET`,
`DISABLE_PROMPT_CACHING_HAIKU`) are not the documented cc-rust control surface.
Use SOTA/MOTA/FOTA model defaults and provider capability gating instead.

## Smoke Matrix

Run the mock/provider-contract matrix without credentials:

```bash
scripts/provider_smoke_matrix.py mock
```

List real smoke requirements:

```bash
scripts/provider_smoke_matrix.py list
```

Run optional real smoke checks:

```bash
scripts/provider_smoke_matrix.py real
```

The real mode skips any row whose required environment is missing. It runs each
case with an isolated `CC_RUST_HOME`, redacts known secret values from captured
output, and strips raw `Authorization: Bearer ...` / `x-api-key: ...` headers
before printing failures.

Covered rows:

| Row | Mock coverage | Real smoke gate |
| --- | --- | --- |
| Direct Anthropic API key | client env/auth fixture | `ANTHROPIC_API_KEY` |
| Direct Anthropic bearer token | bearer header fixture | `ANTHROPIC_AUTH_TOKEN` |
| Anthropic-compatible bearer + custom base URL | compatible endpoint fixture | `ANTHROPIC_AUTH_TOKEN` + `ANTHROPIC_BASE_URL` + compatible `ANTHROPIC_MODEL` or tier defaults |
| Bedrock model mapping | `cc-models` Bedrock mapping fixture | `AWS_BEARER_TOKEN_BEDROCK` or AWS access key/secret |
| Vertex model mapping | `cc-models` Vertex mapping fixture | Vertex token/service-account env + `GOOGLE_CLOUD_PROJECT` |
| Prompt cache disabled for non-Anthropic fields | recursive strip fixture | provider selected with cache capability disabled |
| Prompt cache enabled with TTL/global gates | cache policy fixture | direct Anthropic plus `CC_RUST_PROMPT_CACHE_TTL=1h` and `CC_RUST_PROMPT_CACHE_GLOBAL=1` |
