# Context Phase 6 Provider Token Parity

Date: 2026-05-06
Plan: `.omx/plans/context-followup-phase-plan-2026-05-06.md`

## Decision

Phase 6 resolves the remaining provider exact-count ambiguity from the context
map.

- Bedrock is now exact-count capable through Bedrock Runtime `CountTokens`.
- Vertex is now exact-count capable through the Anthropic partner
  `count-tokens:rawPredict` endpoint.
- OpenAI / OpenAI-compatible providers remain on heuristic fallback because
  cc-rust currently sends Chat Completions requests, while the official OpenAI
  preflight count endpoint counts Responses API input bodies. Wiring it before
  migrating the OpenAI runtime shape would create a misleading "exact" count.

## Provider Outcomes

| Provider | Outcome | Request shape |
| --- | --- | --- |
| OpenAI / OpenAI-compatible | Documented heuristic fallback | No cc-rust exact-count call is made. The official Responses endpoint is intentionally not reused for the current Chat Completions path. |
| Bedrock | Implemented | `POST /model/{modelId}/count-tokens`; body wraps the serialized Anthropic Bedrock InvokeModel body as `input.invokeModel.body`; response parses `inputTokens`. |
| Vertex | Implemented | `POST /publishers/anthropic/models/count-tokens:rawPredict`; body sends `model`, `messages`, and input-bearing optional fields; response parses `input_tokens`. |

## Verification

- `cargo test -p claude-code-rs count_tokens`
- `cargo test -p claude-code-rs test_exact_token_count_support_matrix`

No live provider credentials are required by these tests.

## Official References

- OpenAI Responses input token count endpoint: https://developers.openai.com/api/reference/resources/responses/subresources/input_tokens/methods/count
- Amazon Bedrock CountTokens API: https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_CountTokens.html
- Amazon Bedrock CountTokens input shape: https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_CountTokensInput.html
- Amazon Bedrock InvokeModel token request shape: https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_InvokeModelTokensRequest.html
- Amazon Bedrock token counting guide: https://docs.aws.amazon.com/bedrock/latest/userguide/count-tokens.html
- Vertex AI Claude token counting: https://cloud.google.com/vertex-ai/generative-ai/docs/partner-models/claude/count-tokens
