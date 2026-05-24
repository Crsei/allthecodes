# Anthropic Prompt Caching Reference

Source: https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching
Resolved source: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
Captured: 2026-05-17

This is a local integration summary for cc-rust. It is not a full copy of the
Anthropic documentation. Use the source URL as the authority when API behavior
changes.

## Core Behavior

Prompt caching lets a request reuse a previously processed prompt prefix. The
cacheable prefix is evaluated in this order:

1. `tools`
2. `system`
3. `messages`

The cached region includes all content up to and including the selected cache
breakpoint. This is useful for long system prompts, tool definitions, large
static context, examples, and long multi-turn conversations.

## Two Caching Modes

Automatic caching:

- Add top-level `cache_control` to the request body.
- The API selects the last cacheable block and moves the breakpoint forward as
  the conversation grows.
- Best fit for ordinary multi-turn conversations.
- Uses one of the available breakpoint slots.
- Currently documented as available on the direct Claude API, Claude Platform
  on AWS, and Microsoft Foundry beta.
- The documentation says Bedrock and Vertex AI do not support automatic
  caching.

Explicit cache breakpoints:

- Add `cache_control` directly to individual content blocks.
- Best fit when cc-rust needs precise control over system prompts, tools,
  memory blocks, or imported context.
- The request should keep stable, reusable content early in the prompt so the
  prefix cache has a long reusable span.

cc-rust implementation implication:

- Provider capabilities should distinguish `prompt_cache_explicit` from
  `prompt_cache_automatic`.
- `skip_cache_write` cannot be a no-op. It must either omit cache-control
  fields or select a read-only/no-write strategy if Anthropic exposes one.
- Automatic caching must be disabled for Bedrock and Vertex unless their docs
  later add support.

## TTL And Pricing Semantics

The default cache lifetime is 5 minutes. Anthropic also documents an optional
1-hour TTL at a higher write cost. The pricing model uses multipliers over base
input token price:

- 5-minute cache writes: 1.25x.
- 1-hour cache writes: 2x.
- Cache reads: 0.1x.

cc-rust implementation implication:

- `CacheControl` should include `ttl` as an optional enum, not an arbitrary
  unchecked string.
- Pricing and usage summaries should keep separate cache-read, 5-minute
  cache-write, and 1-hour cache-write token counts when the provider returns
  them.

## Usage Fields

Prompt caching responses can include:

- `cache_creation_input_tokens`.
- `cache_read_input_tokens`.
- `cache_creation` details split by TTL.
- Iteration-level usage for agent-like request flows.
- `service_tier` and `inference_geo` metadata.

cc-rust implementation implication:

- Existing `Usage` fields for cache read/write are only the minimum contract.
- Session export and request snapshots should tolerate extra usage fields
  without dropping the core totals.
- Cost calculation should use cache token fields instead of treating all input
  tokens as base input tokens.

## Pre-Warming

Anthropic documents cache pre-warming with `max_tokens: 0`. The request can
write the cache without producing output content. Some output-producing request
features are incompatible with zero-token pre-warm requests, including
streaming, extended thinking, structured output formats, and forced tool choice.

cc-rust implementation implication:

- Cache pre-warm should be a separate explicit operation, not an accidental
  side effect of normal chat calls.
- A future pre-warm command must disable streaming and thinking for the
  pre-warm request.
- The API client should not reuse a normal streaming request path when
  `max_tokens` is zero.

## cc-rust Contract Checklist

- Add top-level request `cache_control`.
- Add optional per-block `cache_control` for tools, system text, and message
  content blocks.
- Enforce provider-specific automatic-cache support.
- Keep the default 5-minute TTL and optional 1-hour TTL explicit in types.
- Snapshot request bodies with cache-control markers but without credentials.
- Add fixture tests for automatic cache request bodies, explicit breakpoint
  bodies, Bedrock/Vertex automatic-cache rejection, and usage accounting.
