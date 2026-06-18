# Anthropic API Overview Reference

Source: https://docs.anthropic.com/en/api/overview
Resolved source: https://platform.claude.com/docs/en/api/overview
Captured: 2026-05-17

This is a local integration summary for cc-rust. It is not a full copy of the
Anthropic documentation. Use the source URL as the authority when API behavior
changes.

## Scope

The Claude API is the direct REST API hosted at `https://api.anthropic.com`.
For cc-rust, the relevant direct-model surface is the Messages API and the
Token Counting API. Managed Agents, Sessions, Environments, Skills, and Files
are separate API families and should not be treated as already covered by the
current `MessagesRequest` adapter.

## Available API Families

General availability surfaces called out by the overview:

- Messages API: `POST /v1/messages`.
- Message Batches API: `POST /v1/messages/batches`.
- Token Counting API: `POST /v1/messages/count_tokens`.
- Models API: `GET /v1/models`.

Beta surfaces called out by the overview:

- Files API.
- Skills API.
- Agents API.
- Sessions API.
- Environments API.

cc-rust currently focuses on synchronous and streaming Messages calls plus
exact token counting. Other API families require separate request types,
capability flags, tests, and user-facing commands before they should be marked
supported.

## Required Request Headers

Direct Claude API requests require:

- `anthropic-version`, for example `2023-06-01`.
- `content-type: application/json`.
- Exactly one supported auth header:
  - `x-api-key` for Console API keys.
  - `Authorization: Bearer <token>` for short-lived access tokens from
    Workload Identity Federation or equivalent bearer-token auth flows.

cc-rust implementation implication:

- The API client must preserve auth kind after `cc-auth` resolves credentials.
- `ANTHROPIC_API_KEY` and keychain API keys should send `x-api-key`.
- `ANTHROPIC_AUTH_TOKEN` and OAuth access tokens should send
  `Authorization: Bearer ...`.
- Request snapshots must never persist either auth header.

## Cloud Platform Boundary

The overview separates direct Claude API access from cloud platform access.
Claude can be reached through AWS, Amazon Bedrock, Vertex AI, and Microsoft
Foundry, but authentication, feature availability, request limits, and endpoint
shape vary by platform.

cc-rust implementation implication:

- Direct Anthropic, Bedrock, Vertex, Foundry, and third-party
  Anthropic-compatible endpoints need distinct provider capability records.
- A custom base URL that speaks the Anthropic Messages format should be modeled
  as Anthropic-compatible transport, not as OpenAI-compatible transport.
- Provider-specific beta headers must be generated from capability metadata
  instead of copied blindly across providers.

## Request Size Limits

The overview lists these current limits:

- Messages and Token Counting: 32 MB.
- Message Batches: 256 MB.
- Files: 500 MB.
- Sessions, Agents, Environments: 32 MB.
- Vertex AI partner-operated requests: 30 MB.
- Bedrock partner-operated requests: 20 MB.

cc-rust implementation implication:

- Request-size validation should be provider-aware.
- Bedrock and Vertex should not inherit the 32 MB direct Anthropic limit.
- Request snapshots should preserve enough sanitized structure to debug 413
  request-too-large failures without storing credentials.

## Response Metadata

Direct Claude API responses include request metadata such as `request-id` and
organization ID. Claude Platform on AWS may add an AWS request ID as well.

cc-rust implementation implication:

- API error types and stream-start failures should preserve provider request
  IDs when present.
- Streaming errors should be surfaced as structured API errors instead of being
  silently ignored.

## cc-rust Contract Checklist

- Preserve auth kind from `cc-auth` to all Anthropic request paths.
- Use `x-api-key` only for API keys, not bearer tokens.
- Keep `anthropic-version` explicit and test-covered.
- Build beta headers from provider capabilities.
- Treat cloud and third-party Anthropic-compatible providers as separate
  capability profiles.
- Keep exact token counting available only where the provider exposes a
  compatible endpoint.
- Add mocked fixtures for headers, request bodies, stream events, error events,
  and count-token calls.
