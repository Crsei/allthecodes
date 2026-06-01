> ## Documentation Index
> Fetch the complete documentation index at: https://code.claude.com/docs/llms.txt
> Use this file to discover all available pages before exploring further.

# Model configuration

> cc-rust model configuration. cc-rust intentionally uses neutral public aliases
> `SOTA`, `MOTA`, and `FOTA`; removed Claude Code family aliases such as
> `opus`, `sonnet`, and `haiku` are rejected instead of being passed to providers.

## Available models

For the `model` setting in Claude Code, you can configure either:

* A **cc-rust model alias** (`SOTA`, `MOTA`, or `FOTA`)
* A **model name**
  * Anthropic API: A full **[model name](https://platform.claude.com/docs/en/about-claude/models/overview)**
  * Bedrock: an inference profile ARN
  * Foundry: a deployment name
  * Vertex: a version name

### Model aliases

Model aliases provide a convenient way to select model settings without
remembering exact version numbers:

| Model alias | Current target | Behavior |
| ------------ | -------------- | -------- |
| **`SOTA`** | `gpt-5.5` | Highest-capability work |
| **`MOTA`** | `gpt-5.5` | Balanced capability and speed |
| **`FOTA`** | `gpt-5.5` | Fast lightweight work |

The old family aliases `opus`, `sonnet`, and `haiku` are no longer accepted as
public aliases. Use `SOTA`, `MOTA`, `FOTA`, or a full provider/model ID.

### Setting your model

You can configure your model in several ways, listed in order of priority:

1. **During session** - Use `/model <SOTA|MOTA|FOTA|name>` to switch models mid-session
2. **At startup** - Launch with `claude --model <SOTA|MOTA|FOTA|name>`
3. **Environment variable** - Set `ANTHROPIC_MODEL=<SOTA|MOTA|FOTA|name>`
4. **Settings** - Configure permanently in your settings file using the `model`
   field.

Example usage:

```bash theme={null}
# Start with the highest-capability cc-rust alias
claude --model SOTA

# Switch to the balanced cc-rust alias during session
/model MOTA
```

Example settings file:

```json theme={null}
{
    "permissions": {
        ...
    },
    "model": "SOTA"
}
```

## Restrict model selection

Enterprise administrators can use `availableModels` in [managed or policy settings](/en/settings#settings-files) to restrict which models users can select.

When `availableModels` is set, users cannot switch to models not in the list via `/model`, `--model` flag, Config tool, or `ANTHROPIC_MODEL` environment variable.

```json theme={null}
{
  "availableModels": ["MOTA", "FOTA"]
}
```

### Configure cc-rust model aliases

cc-rust resolves the neutral aliases `SOTA`, `MOTA`, and `FOTA` from settings before falling back to provider-specific environment mapping or built-in constants.

```json theme={null}
{
  "sotaModel": "gpt-5.5",
  "motaModel": "gpt-5.4-mini",
  "fotaModel": "gpt-5.3-codex-spark",
  "model": "SOTA",
  "availableModels": ["SOTA", "MOTA", "FOTA"]
}
```

This keeps aliases stable in commands and policy while letting user, project, local, or managed settings choose the concrete model IDs.

### Default model behavior

The Default option in the model picker is not affected by `availableModels`. It always remains available and represents the system's runtime default [based on the user's subscription tier](#default-model-setting).

Even with `availableModels: []`, users can still use Claude Code with the Default model for their tier.

### Control the model users run on

The `model` setting is an initial selection, not enforcement. It sets which model is active when a session starts, but users can still open `/model` and pick Default, which resolves to the system default for their tier regardless of what `model` is set to.

To fully control the model experience, combine three settings:

* **`availableModels`**: restricts which named models users can switch to
* **`model`**: sets the initial model selection when a session starts
* **Full model IDs or cc-rust aliases**: use `SOTA`, `MOTA`, `FOTA`, or full model IDs. The removed `sonnet`, `opus`, and `haiku` aliases are not accepted.

This example starts users on Sonnet 4.5 and limits the picker to Sonnet 4.5 and the fast cc-rust alias:

```json theme={null}
{
  "model": "claude-sonnet-4-5",
  "availableModels": ["claude-sonnet-4-5", "FOTA"]
}
```

Use full model IDs when you need exact version pins.

### Merge behavior

When `availableModels` is set at multiple levels, such as user settings and project settings, arrays are merged and deduplicated. To enforce a strict allowlist, set `availableModels` in managed or policy settings which take highest priority.

### Mantle model IDs

When the [Bedrock Mantle endpoint](/en/amazon-bedrock#use-the-mantle-endpoint) is enabled, entries in `availableModels` that start with `anthropic.` are added to the `/model` picker as custom options and routed to the Mantle endpoint. The setting still restricts the picker to listed entries, so include the cc-rust aliases (`SOTA`, `MOTA`, `FOTA`) alongside any Mantle IDs you want available.

## Special model behavior

### `default` model setting

The behavior of `default` depends on your account type:

* **Max and Team Premium**: defaults to Opus 4.6
* **Pro and Team Standard**: defaults to Sonnet 4.6
* **Enterprise**: Opus 4.6 is available but not the default

Claude Code may automatically fall back to Sonnet if you hit a usage threshold with Opus.

### Removed family aliases

cc-rust does not implement the upstream `opusplan`, `opus`, `sonnet`, or
`haiku` compatibility aliases. Use `SOTA`, `MOTA`, `FOTA`, or explicit model IDs
for both runtime model selection and configuration allowlists.

### Adjust effort level

[Effort levels](https://platform.claude.com/docs/en/build-with-claude/effort) control adaptive reasoning, which dynamically allocates thinking based on task complexity. Lower effort is faster and cheaper for straightforward tasks, while higher effort provides deeper reasoning for complex problems.

Three levels persist across sessions: **low**, **medium**, and **high**. A fourth level, **max**, provides the deepest reasoning with no constraint on token spending, so responses are slower and cost more than at `high`. `max` is available on Opus 4.6 only and does not persist across sessions except through the `CLAUDE_CODE_EFFORT_LEVEL` environment variable.

The default effort level depends on your plan. Pro and Max subscribers default to medium effort. All other users default to high effort: API key, Team, Enterprise, and third-party provider (Bedrock, Vertex AI, Foundry) users.

Your plan's default suits most coding tasks. Raise effort for work that benefits from deeper reasoning, such as hard debugging problems or complex architectural decisions. Higher levels can cause the model to overthink routine work.

For one-off deep reasoning without changing your session setting, include "ultrathink" in your prompt to trigger high effort for that turn. This has no effect if your session is already at high or max.

**Setting effort:**

* **`/effort`**: run `/effort low`, `/effort medium`, `/effort high`, or `/effort max` to change the level, or `/effort auto` to reset to the model default
* **In `/model`**: use left/right arrow keys to adjust the effort slider when selecting a model
* **`--effort` flag**: pass `low`, `medium`, `high`, or `max` to set the level for a single session when launching Claude Code
* **Environment variable**: set `CLAUDE_CODE_EFFORT_LEVEL` to `low`, `medium`, `high`, `max`, or `auto`
* **Settings**: set `effortLevel` in your settings file to `"low"`, `"medium"`, or `"high"`
* **Skill and subagent frontmatter**: set `effort` in a [skill](/en/skills#frontmatter-reference) or [subagent](/en/sub-agents#supported-frontmatter-fields) markdown file to override the effort level when that skill or subagent runs

For Codex/OpenAI Responses models, use the Codex-native setting instead of a token budget:

```json
{
  "apiProvider": "openai-codex",
  "backend": "codex",
  "model": "SOTA",
  "model_reasoning_effort": "high"
}
```

`model_reasoning_effort` accepts `"none"`, `"minimal"`, `"low"`, `"medium"`, `"high"`, and `"xhigh"`. It maps to the Responses API `reasoning.effort` field.

The environment variable takes precedence over all other methods, then your configured level, then the model default. Frontmatter effort applies when that skill or subagent is active, overriding the session level but not the environment variable.

Effort is supported on Opus 4.6 and Sonnet 4.6. The effort slider appears in `/model` when a supported model is selected. The current effort level is also displayed next to the logo and spinner, for example "with low effort", so you can confirm which setting is active without opening `/model`.

To disable adaptive reasoning on Opus 4.6 and Sonnet 4.6 and revert to the previous fixed thinking budget, set `CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING=1`. When disabled, these models use the fixed budget controlled by `MAX_THINKING_TOKENS`. See [environment variables](/en/env-vars).

### Extended context

Opus 4.6 and Sonnet 4.6 support a [1 million token context window](https://platform.claude.com/docs/en/build-with-claude/context-windows#1m-token-context-window) for long sessions with large codebases.

Availability varies by model and plan. On Max, Team, and Enterprise plans, Opus is automatically upgraded to 1M context with no additional configuration. This applies to both Team Standard and Team Premium seats.

| Plan                      | Opus 4.6 with 1M context                                                                                  | Sonnet 4.6 with 1M context                                                                                |
| ------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Max, Team, and Enterprise | Included with subscription                                                                                | Requires [extra usage](https://support.claude.com/en/articles/12429409-extra-usage-for-paid-claude-plans) |
| Pro                       | Requires [extra usage](https://support.claude.com/en/articles/12429409-extra-usage-for-paid-claude-plans) | Requires [extra usage](https://support.claude.com/en/articles/12429409-extra-usage-for-paid-claude-plans) |
| API and pay-as-you-go     | Full access                                                                                               | Full access                                                                                               |

To disable 1M context entirely, set `CLAUDE_CODE_DISABLE_1M_CONTEXT=1`. This removes 1M model variants from the model picker. See [environment variables](/en/env-vars).

The 1M context window uses standard model pricing with no premium for tokens beyond 200K. For plans where extended context is included with your subscription, usage remains covered by your subscription. For plans that access extended context through extra usage, tokens are billed to extra usage.

If your account supports 1M context, the option appears in the model picker (`/model`) in the latest versions of Claude Code. If you don't see it, try restarting your session.

Use the provider-supported full model name when selecting extended context:

```bash theme={null}
/model claude-opus-4-6[1m]
```

## Checking your current model

You can see which model you're currently using in several ways:

1. In [status line](/en/statusline) (if configured)
2. In `/status`, which also displays your account information.

## Add a custom model option

Use `ANTHROPIC_CUSTOM_MODEL_OPTION` to add a single custom entry to the `/model` picker without replacing the built-in aliases. This is useful for LLM gateway deployments or testing model IDs that Claude Code does not list by default.

This example sets all three variables to make a gateway-routed Opus deployment selectable:

```bash theme={null}
export ANTHROPIC_CUSTOM_MODEL_OPTION="my-gateway/claude-opus-4-6"
export ANTHROPIC_CUSTOM_MODEL_OPTION_NAME="Opus via Gateway"
export ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION="Custom deployment routed through the internal LLM gateway"
```

The custom entry appears at the bottom of the `/model` picker. `ANTHROPIC_CUSTOM_MODEL_OPTION_NAME` and `ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION` are optional. If omitted, the model ID is used as the name and the description defaults to `Custom model (<model-id>)`.

Claude Code skips validation for the model ID set in `ANTHROPIC_CUSTOM_MODEL_OPTION`, so you can use any string your API endpoint accepts.

## Environment variables

cc-rust does not remap the removed family aliases. Use full **model names** (or
equivalent provider IDs) in provider environment variables.

| Environment variable             | Description                                                                                   |
| -------------------------------- | --------------------------------------------------------------------------------------------- |
| `ANTHROPIC_DEFAULT_OPUS_MODEL`   | Upstream family default variable; cc-rust model selection should use full IDs or `SOTA` instead of `opus`. |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | Upstream family default variable; cc-rust model selection should use full IDs or `MOTA` instead of `sonnet`. |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL`  | Upstream family default variable; cc-rust model selection should use full IDs or `FOTA` instead of `haiku`. |
| `CLAUDE_CODE_SUBAGENT_MODEL`     | The model to use for [subagents](/en/sub-agents)                                              |

Note: `ANTHROPIC_SMALL_FAST_MODEL` is deprecated in favor of
`ANTHROPIC_DEFAULT_HAIKU_MODEL`.

### Pin models for third-party deployments

When deploying Claude Code through [Bedrock](/en/amazon-bedrock), [Vertex AI](/en/google-vertex-ai), or [Foundry](/en/microsoft-foundry), pin model versions before rolling out to users.

Without pinning, upstream Claude Code may use family aliases. cc-rust avoids
that compatibility path: configure `SOTA`, `MOTA`, `FOTA`, or explicit model IDs
so provider requests never receive removed aliases like `sonnet`, `opus`, or
`haiku`.

<Warning>
  Set all three model environment variables to specific version IDs as part of your initial setup. Pinning lets you control when your users move to a new model.
</Warning>

Use the following environment variables with version-specific model IDs for your provider:

| Provider  | Example                                                                 |
| :-------- | :---------------------------------------------------------------------- |
| Bedrock   | `export ANTHROPIC_DEFAULT_OPUS_MODEL='us.anthropic.claude-opus-4-6-v1'` |
| Vertex AI | `export ANTHROPIC_DEFAULT_OPUS_MODEL='claude-opus-4-6'`                 |
| Foundry   | `export ANTHROPIC_DEFAULT_OPUS_MODEL='claude-opus-4-6'`                 |

Apply the same pattern for `ANTHROPIC_DEFAULT_SONNET_MODEL` and `ANTHROPIC_DEFAULT_HAIKU_MODEL`. For current and legacy model IDs across all providers, see [Models overview](https://platform.claude.com/docs/en/about-claude/models/overview). To upgrade users to a new model version, update these environment variables and redeploy.

To enable [extended context](#extended-context) for a pinned model, append `[1m]` to the model ID in `ANTHROPIC_DEFAULT_OPUS_MODEL` or `ANTHROPIC_DEFAULT_SONNET_MODEL`:

```bash theme={null}
export ANTHROPIC_DEFAULT_OPUS_MODEL='claude-opus-4-6[1m]'
```

The `[1m]` suffix applies the 1M context window to the configured model ID. Only append `[1m]` when the underlying model supports 1M context, such as Opus 4.6 or Sonnet 4.6.

<Note>
  The `settings.availableModels` allowlist still applies when using third-party providers. In cc-rust, filtering matches `SOTA`, `MOTA`, `FOTA`, and full model IDs; removed legacy aliases (`opus`, `sonnet`, `haiku`) do not match.
</Note>

### Customize pinned model display and capabilities

When you pin a model on a third-party provider, the provider-specific ID appears as-is in the `/model` picker and Claude Code may not recognize which features the model supports. You can override the display name and declare capabilities with companion environment variables for each pinned model.

These variables only take effect on third-party providers such as Bedrock, Vertex AI, and Foundry. They have no effect when using the Anthropic API directly.

| Environment variable                                  | Description                                                                                                        |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `ANTHROPIC_DEFAULT_OPUS_MODEL_NAME`                   | Display name for the pinned Opus model in the `/model` picker. Defaults to the model ID when not set               |
| `ANTHROPIC_DEFAULT_OPUS_MODEL_DESCRIPTION`            | Display description for the pinned Opus model in the `/model` picker. Defaults to `Custom Opus model` when not set |
| `ANTHROPIC_DEFAULT_OPUS_MODEL_SUPPORTED_CAPABILITIES` | Comma-separated list of capabilities the pinned Opus model supports                                                |

The same `_NAME`, `_DESCRIPTION`, and `_SUPPORTED_CAPABILITIES` suffixes are available for `ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_HAIKU_MODEL`, and `ANTHROPIC_CUSTOM_MODEL_OPTION`.

Claude Code enables features like [effort levels](#adjust-effort-level) and [extended thinking](/en/common-workflows#use-extended-thinking-thinking-mode) by matching the model ID against known patterns. Provider-specific IDs such as Bedrock ARNs or custom deployment names often don't match these patterns, leaving supported features disabled. Set `_SUPPORTED_CAPABILITIES` to tell Claude Code which features the model actually supports:

| Capability value       | Enables                                                                         |
| ---------------------- | ------------------------------------------------------------------------------- |
| `effort`               | [Effort levels](#adjust-effort-level) and the `/effort` command                 |
| `max_effort`           | The `max` effort level                                                          |
| `thinking`             | [Extended thinking](/en/common-workflows#use-extended-thinking-thinking-mode)   |
| `adaptive_thinking`    | Adaptive reasoning that dynamically allocates thinking based on task complexity |
| `interleaved_thinking` | Thinking between tool calls                                                     |

When `_SUPPORTED_CAPABILITIES` is set, listed capabilities are enabled and unlisted capabilities are disabled for the matching pinned model. When the variable is unset, Claude Code falls back to built-in detection based on the model ID.

This example pins Opus to a Bedrock custom model ARN, sets a friendly name, and declares its capabilities:

```bash theme={null}
export ANTHROPIC_DEFAULT_OPUS_MODEL='arn:aws:bedrock:us-east-1:123456789012:custom-model/abc'
export ANTHROPIC_DEFAULT_OPUS_MODEL_NAME='Opus via Bedrock'
export ANTHROPIC_DEFAULT_OPUS_MODEL_DESCRIPTION='Opus 4.6 routed through a Bedrock custom endpoint'
export ANTHROPIC_DEFAULT_OPUS_MODEL_SUPPORTED_CAPABILITIES='effort,max_effort,thinking,adaptive_thinking,interleaved_thinking'
```

### Override model IDs per version

The family-level environment variables above configure one model ID per family alias. If you need to map several versions within the same family to distinct provider IDs, use the `modelOverrides` setting instead.

`modelOverrides` maps individual Anthropic model IDs to the provider-specific strings that Claude Code sends to your provider's API. When a user selects a mapped model in the `/model` picker, Claude Code uses your configured value instead of the built-in default.

This lets enterprise administrators route each model version to a specific Bedrock inference profile ARN, Vertex AI version name, or Foundry deployment name for governance, cost allocation, or regional routing.

Set `modelOverrides` in your [settings file](/en/settings#settings-files):

```json theme={null}
{
  "modelOverrides": {
    "claude-opus-4-6": "arn:aws:bedrock:us-east-2:123456789012:application-inference-profile/opus-prod",
    "claude-opus-4-5-20251101": "arn:aws:bedrock:us-east-2:123456789012:application-inference-profile/opus-45-prod",
    "claude-sonnet-4-6": "arn:aws:bedrock:us-east-2:123456789012:application-inference-profile/sonnet-prod"
  }
}
```

Keys must be Anthropic model IDs as listed in the [Models overview](https://platform.claude.com/docs/en/about-claude/models/overview). For dated model IDs, include the date suffix exactly as it appears there. Unknown keys are ignored.

Overrides replace the built-in model IDs that back each entry in the `/model` picker. On Bedrock, overrides take precedence over any inference profiles that Claude Code discovers automatically at startup. Values you supply directly through `ANTHROPIC_MODEL`, `--model`, or the `ANTHROPIC_DEFAULT_*_MODEL` environment variables are passed to the provider as-is and are not transformed by `modelOverrides`.

`modelOverrides` works alongside `availableModels`. In cc-rust, use `SOTA`,
`MOTA`, `FOTA`, or explicit model IDs in `availableModels`; an entry like
`"opus"` is a removed legacy alias and will not match.

### Prompt caching configuration

Claude Code automatically uses [prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) to optimize performance and reduce costs. You can disable prompt caching globally or for specific model tiers:

| Environment variable            | Description                                                                                    |
| ------------------------------- | ---------------------------------------------------------------------------------------------- |
| `DISABLE_PROMPT_CACHING`        | Set to `1` to disable prompt caching for all models (takes precedence over per-model settings) |
| `DISABLE_PROMPT_CACHING_HAIKU`  | Set to `1` to disable prompt caching for Haiku models only                                     |
| `DISABLE_PROMPT_CACHING_SONNET` | Set to `1` to disable prompt caching for Sonnet models only                                    |
| `DISABLE_PROMPT_CACHING_OPUS`   | Set to `1` to disable prompt caching for Opus models only                                      |

These environment variables give you fine-grained control over prompt caching behavior. The global `DISABLE_PROMPT_CACHING` setting takes precedence over the model-specific settings, allowing you to quickly disable all caching when needed. The per-model settings are useful for selective control, such as when debugging specific models or working with cloud providers that may have different caching implementations.
