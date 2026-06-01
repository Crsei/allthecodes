# Context Phase 1 Token Count Provider Matrix

Date: 2026-05-06
Plan: `.omx/plans/context-implementation-phase-plan-2026-05-06.md`

## Decision

Phase 1 adds an async exact-token-count path outside `cc-utils`.
`cc-utils` remains the synchronous offline heuristic layer; provider-backed
callers can convert exact counts into the same `TokenUsageReport` shape with
`TokenCountMethod::ProviderExact`.

The exact path is currently exposed as diagnostics, gated by
`CC_RUST_EXACT_TOKEN_DIAGNOSTICS=1`, so normal query traffic keeps the existing
latency profile. Auto-compact still uses the deterministic heuristic threshold
until near-threshold exact-count fallback is wired and tested separately.

## Provider Matrix

| Provider | Phase 1 status | Endpoint shape | Notes |
| --- | --- | --- | --- |
| Anthropic | Supported | `POST /v1/messages/count_tokens` | Uses the same shaped messages request body minus generation-only fields (`stream`, `max_tokens`, `advisor_model`). Parses `input_tokens`. |
| Azure Anthropic-compatible | Supported | `POST {endpoint}/v1/messages/count_tokens` | Uses the Anthropic-compatible body/header path against the configured Azure endpoint. |
| Google Gemini | Supported | `POST {base}/models/{model}:countTokens?key=...` | Wraps the normal Gemini `generateContent` body as `generateContentRequest` and parses `totalTokens`. |
| OpenAI / OpenAI-compatible | Heuristic fallback | Not wired for cc-rust's Chat Completions wire path | OpenAI now documents `POST /v1/responses/input_tokens`, but cc-rust's OpenAI-compatible provider still sends Chat Completions requests; using the Responses count endpoint would not be an exact count for the request actually sent. |
| Bedrock | Supported | `POST /model/{modelId}/count-tokens` | Wraps the serialized Bedrock InvokeModel Anthropic body in `input.invokeModel.body` and parses `inputTokens`. |
| Vertex | Supported | `POST /publishers/anthropic/models/count-tokens:rawPredict` | Sends the Anthropic-on-Vertex model ID plus input-bearing fields and parses `input_tokens`. |

## Official References Checked

- Anthropic Message Count Tokens API: https://docs.anthropic.com/en/api/messages-count-tokens
- Anthropic token counting guide: https://docs.anthropic.com/en/docs/build-with-claude/token-counting
- Gemini token counting API: https://ai.google.dev/api/tokens
- OpenAI token concepts and tokenizer guidance: https://platform.openai.com/docs/concepts/tokens
- OpenAI Responses input token count endpoint: https://developers.openai.com/api/reference/resources/responses/subresources/input_tokens/methods/count
- Amazon Bedrock CountTokens API: https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_CountTokens.html
- Amazon Bedrock token counting guide: https://docs.aws.amazon.com/bedrock/latest/userguide/count-tokens.html
- Vertex AI Claude token counting: https://cloud.google.com/vertex-ai/generative-ai/docs/partner-models/claude/count-tokens

## Verification

- `cargo test -p cc-utils test_token_usage_report_from_provider_count_marks_exact`
- `cargo test -p claude-code-rs test_anthropic_count_tokens_body_omits_generation_only_fields`
- `cargo test -p claude-code-rs test_exact_token_count_support_matrix`
- `cargo test -p claude-code-rs test_build_gemini_count_tokens_request_wraps_generate_content_request`
- `cargo test -p claude-code-rs count_tokens`
