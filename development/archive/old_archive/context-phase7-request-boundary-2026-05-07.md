# Context Request Boundary - 2026-05-07

This note closes the ambiguity between the local context estimate and the
provider request size used for auto-compact decisions.

## Boundaries

1. Local message estimate

   `cc-utils::tokens::estimate_messages_tokens()` is a synchronous fallback
   estimate over local `Message` values. The compact pipeline may use it before
   and after local transforms, but the scheduling value is the post-pipeline
   estimate. Token savings counters are diagnostics only and must not be
   subtracted from the post-pipeline estimate again.

2. Provider exact-count preflight

   Exact counting is an async engine/API concern. It only runs when the
   post-pipeline heuristic is near the auto-compact threshold and the provider
   supports count-tokens. The count request is built from the same turn boundary
   as the final provider call:

   - post-pipeline messages
   - current system prompt
   - refreshed tool schema snapshot
   - selected model
   - thinking/effort settings
   - advisor model when supported by the provider request path
   - max-output override carried by the query state

   Provider adapters may strip generation-only fields from the count-tokens
   HTTP body as required by that API, but the source request object remains the
   same request shape as the model call.

3. Final provider request

   The query loop refreshes tools before auto-compact and reuses that tool
   snapshot for the subsequent model call. This keeps exact-count preflight and
   final request construction aligned. If tool refresh fails, both paths use the
   existing tool list.

## Fallback Cases

- If the provider has no exact token-count endpoint, the post-pipeline
  heuristic decides whether auto-compact triggers.
- If exact counting fails, the heuristic decision is kept and the request
  proceeds.
- Model fallback retries happen after a model-call failure; they reuse the same
  message/system/tool boundary but may use the fallback model's context window
  on the retry path.
