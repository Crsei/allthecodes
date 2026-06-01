# Cloud Claude Providers — AWS Bedrock / GCP Vertex AI

Bedrock and Vertex are **alternative transports for the same Claude
conversation loop**, not separate product modes. They replace the default
first-party `api.anthropic.com` endpoint with AWS- or GCP-managed endpoints
that serve the same Claude models.

## Provider selection

Set one of the following environment variables to activate a third-party
provider. It takes priority over `ANTHROPIC_API_KEY` and any other provider
detection:

| Variable                    | Effect                                    |
|-----------------------------|-------------------------------------------|
| `CLAUDE_CODE_USE_BEDROCK=1` | Route all Claude calls via AWS Bedrock   |
| `CLAUDE_CODE_USE_VERTEX=1`  | Route all Claude calls via GCP Vertex AI |

Truthy values: `1`, `true`, `yes`, `on` (case-insensitive). Matches the
`isEnvTruthy` behavior in `claude-code-bun`.

For the current REPL process, `/login bedrock` and `/login vertex` set the
same provider flags in-process. Persist the environment variables before
launching cc-rust if you want future sessions to use the same cloud provider.

## AWS Bedrock

### Setup

1. `export CLAUDE_CODE_USE_BEDROCK=1`
2. Set the region (defaults to `us-east-1`):
   ```bash
   export AWS_REGION=us-west-2
   # or AWS_DEFAULT_REGION=us-west-2
   ```
3. Choose an authentication mode:

   **Bedrock API key (recommended for simplicity)**:
   ```bash
   export AWS_BEARER_TOKEN_BEDROCK=<bedrock-api-key>
   ```

   **Standard AWS credentials (SigV4)**:
   ```bash
   export AWS_ACCESS_KEY_ID=...
   export AWS_SECRET_ACCESS_KEY=...
   export AWS_SESSION_TOKEN=...   # optional, for STS / assume-role
   ```

### Optional overrides

| Variable                       | Default                                                    | Purpose                           |
|--------------------------------|------------------------------------------------------------|-----------------------------------|
| `ANTHROPIC_BEDROCK_BASE_URL`   | `https://bedrock-runtime.{region}.amazonaws.com`          | Override endpoint (proxy / mocks) |
| `ANTHROPIC_MODEL`              | `claude-sonnet-4-5-20250929`                              | Initial default model             |

### Model mapping

cc-rust uses first-party model IDs internally and auto-translates to Bedrock
IDs on the wire. You can also pass Bedrock IDs directly.

| cc-rust input                   | Bedrock wire ID                                    |
|---------------------------------|----------------------------------------------------|
| `claude-sonnet-4-5-20250929`    | `us.anthropic.claude-sonnet-4-5-20250929-v1:0`    |
| `claude-haiku-4-5-20251001`     | `us.anthropic.claude-haiku-4-5-20251001-v1:0`     |
| `claude-opus-4-5-20251101`      | `us.anthropic.claude-opus-4-5-20251101-v1:0`      |
| `claude-opus-4-7`               | `us.anthropic.claude-opus-4-7`                    |
| `claude-3-7-sonnet-20250219`    | `us.anthropic.claude-3-7-sonnet-20250219-v1:0`    |
| `anthropic.claude-…-v2:0`       | *(passthrough)*                                    |
| `eu.anthropic.claude-…-v1:0`    | *(passthrough)*                                    |
| `arn:aws:bedrock:…`             | *(passthrough)* — supply your own inference profile ARN |

See `crates/claude-code-rs/src/api/model_mapping.rs` for the full list.

## GCP Vertex AI

### Setup

1. `export CLAUDE_CODE_USE_VERTEX=1`
2. Set the project ID (first non-empty wins):
   ```bash
   export ANTHROPIC_VERTEX_PROJECT_ID=my-project
   # or GOOGLE_CLOUD_PROJECT, or GCLOUD_PROJECT
   ```
3. Set the region (defaults to `us-east5`):
   ```bash
   export CLOUD_ML_REGION=europe-west4
   ```
   Per-model overrides are also supported:
   `VERTEX_REGION_CLAUDE_HAIKU_4_5`,
   `VERTEX_REGION_CLAUDE_3_5_HAIKU`,
   `VERTEX_REGION_CLAUDE_3_5_SONNET`,
   `VERTEX_REGION_CLAUDE_3_7_SONNET`,
   `VERTEX_REGION_CLAUDE_4_1_OPUS`,
   `VERTEX_REGION_CLAUDE_4_0_OPUS`,
   `VERTEX_REGION_CLAUDE_4_6_SONNET`,
   `VERTEX_REGION_CLAUDE_4_5_SONNET`,
   and `VERTEX_REGION_CLAUDE_4_0_SONNET`.
4. Provide an access token:

   **Option A — gcloud CLI (recommended)**:
   ```bash
   gcloud auth application-default login
   # cc-rust will invoke `gcloud auth application-default print-access-token`
   # automatically as a fallback.
   ```

   **Option B — pre-obtained token**:
   ```bash
   export CLAUDE_CODE_VERTEX_ACCESS_TOKEN=$(gcloud auth application-default print-access-token)
   # or GOOGLE_OAUTH_ACCESS_TOKEN=...
   ```

   **Option C — service-account key file**:
   ```bash
   export GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
   # cc-rust signs an RS256 JWT and exchanges it for a cloud-platform token.
   ```

### Model mapping

| cc-rust input                   | Vertex wire ID                |
|---------------------------------|-------------------------------|
| `claude-sonnet-4-5-20250929`    | `claude-sonnet-4-5@20250929`  |
| `claude-haiku-4-5-20251001`     | `claude-haiku-4-5@20251001`   |
| `claude-opus-4-5-20251101`      | `claude-opus-4-5@20251101`    |
| `claude-opus-4-6`               | `claude-opus-4-6`             |
| `claude-opus-4-7`               | `claude-opus-4-7`             |
| `claude-…@YYYYMMDD`             | *(passthrough)*               |

## Known limits (MVP)

Phase 1 ships basic Claude conversation support. The following are intentionally
out of scope and tracked for Phase 2:

- **Bedrock inference profile ARN discovery** — you can pass ARNs directly,
  but there is no automatic profile listing (`bedrock:ListInferenceProfiles`).
- **Bedrock CountTokens parity** — token counting uses the shared estimator,
  not Bedrock's dedicated endpoint.
- **First-party-only features** (voice, bridge, some analytics) continue to
  operate as if first-party; feature gating per-provider is also Phase 2.

## Quick verification

Once configured, running the CLI should produce a normal Claude session:

```bash
# Bedrock
CLAUDE_CODE_USE_BEDROCK=1 AWS_BEARER_TOKEN_BEDROCK=... AWS_REGION=us-east-1 \
  cargo run -- "say hi"

# Vertex
CLAUDE_CODE_USE_VERTEX=1 ANTHROPIC_VERTEX_PROJECT_ID=my-proj \
  CLOUD_ML_REGION=us-east5 \
  cargo run -- "say hi"
```

## Source files

- `crates/claude-code-rs/src/api/client/mod.rs` - `ApiProvider::{Bedrock, Vertex}` variants and `from_{bedrock,vertex}_env()`
- `crates/claude-code-rs/src/api/bedrock.rs` - Bedrock request flow + synthesized streaming
- `crates/claude-code-rs/src/api/vertex.rs` - Vertex request flow (real SSE streaming)
- `crates/claude-code-rs/src/api/sigv4.rs` - Minimal AWS SigV4 signer (sha2 + hmac, no SDK dep)
- `crates/claude-code-rs/src/api/model_mapping.rs` - First-party to provider model ID translation
