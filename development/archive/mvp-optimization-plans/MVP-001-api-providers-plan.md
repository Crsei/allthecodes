# MVP-001 API Providers Optimization Plan

## Goal

Bring cc-rust provider support to a production-grade contract for Bedrock, Vertex, Anthropic, OpenAI-compatible, Google, Azure, and Codex-style backends, or explicitly mark unsupported providers as intentional crops.

## Current Rust Surface

- `crates/claude-code-rs/src/api/`
- `crates/claude-code-rs/src/api/model_mapping.rs`
- `crates/cc-config/src/`
- `crates/claude-code-rs/src/auth/`

Current risk: provider variants exist in configuration/status documents, and real provider/e2e coverage still lags behind the adapter matrix. This makes provider selection validation stronger than before, but end-to-end provider drift can still escape unit tests.

## Bun Reference Surface

- `F:\AIclassmanager\cc\claude-code-bun\src\commands\provider.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\claude.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\bedrockClient.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\gemini\client.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\grok\client.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\openai\client.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\__tests__\bedrockClient.test.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\__tests__\betaHeaders.test.ts`

The Bun implementation separates provider command selection, provider-specific clients, request body translation, beta/header handling, retry/error utilities, and provider tests.

## Architecture Delta

| Concern | Rust target | Bun reference | Gap |
| --- | --- | --- | --- |
| Provider selection | config/auth/API factory | `commands/provider.ts` | Rust needs validation before runtime query execution. |
| Bedrock | API client + auth resolver | `bedrockClient.ts`, `aws.ts` | Native AWS EventStream and auth are implemented; real provider smoke coverage still missing. |
| Vertex | API client + auth resolver | provider/env branches in API layer | Project/location auth and service-account JWT exchange are implemented; real provider smoke coverage and error mapping still need expansion. |
| Headers/betas | provider request builder | `claude.ts`, `constants/betas.ts` | Needs provider-specific beta compatibility. |
| Tests | integration fixtures | provider `__tests__` | Need mocked provider coverage and real smoke hooks. |

## Plan

1. Define a provider capability matrix in Rust: auth source, streaming support, tool-use support, thinking support, prompt-cache support, and unsupported reason.
2. Move provider selection into a fallible validation step before `ApiClient` construction.
3. Add provider-specific request builders instead of branching inside one generic request path.
4. Keep Bedrock request signing, region/profile resolution, Anthropic-body conversion, EventStream parsing, and Bedrock error normalization covered by focused tests.
5. Keep Vertex project/location auth, per-model region selection, service-account JWT exchange, and streamRawPredict conversion covered by focused tests.
6. Add test fixtures for config validation, auth failure, throttling, malformed provider response, streaming chunks, and beta/header behavior.

## Verification

- Unit tests for every provider config variant.
- Mocked integration tests for Bedrock and Vertex request construction.
- Snapshot tests for provider-specific headers and JSON request bodies.
- Manual smoke checklist for real endpoints, kept out of normal CI unless credentials are present.

## Implementation Notes

### 2026-04-26 Slice: Provider contract and startup validation

Implemented:

- Added a Rust provider capability matrix for Anthropic, Azure, OpenAI-compatible, Google, OpenAI Codex, Bedrock, Vertex, and Foundry.
- Added fallible API client construction paths so explicit Bedrock/Vertex selection fails early with actionable configuration errors instead of silently falling back.
- Marked Bedrock and Vertex as partially supported, and Foundry as unsupported until a request/auth adapter exists.

Initially deferred by request:

- Bedrock native AWS EventStream support. Implemented on 2026-05-05 in Stage 8.1.
- Vertex service-account JWT exchange. Implemented on 2026-05-05 in Stage 8.3.
- Provider-specific mock integration fixtures.
- Beta/header snapshot coverage.

Reminder: do not treat MVP-001 as fully closed until the deferred items above are either implemented or explicitly moved to an intentional-crop decision.

## Close Criteria

MVP-001 can close when unsupported providers fail early with actionable diagnostics, supported providers have provider-specific tests, and docs no longer list Bedrock/Vertex as ambiguous TODOs.
