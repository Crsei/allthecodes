use serde_json::{json, Value};

use super::VALID_API_PROVIDERS;

// ---------------------------------------------------------------------------
// JSON Schema
// ---------------------------------------------------------------------------

/// Return a JSON Schema (Draft 2020-12) describing the on-disk
/// `settings.json` shape.
///
/// The schema is hand-maintained so the repo can commit it without pulling
/// in `schemars`. Keep in sync with [`crate::settings::RawSettings`].
pub fn settings_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://allthecodes/settings.schema.json",
        "title": "allthecodes settings",
        "description": "On-disk shape of settings.json (managed/user/project/local).",
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "model": { "type": "string" },
            "backend": { "type": "string", "enum": ["native", "codex"] },
            "apiProvider": { "type": "string", "enum": VALID_API_PROVIDERS },
            "activeAuthProfile": { "type": "string" },
            "authProfiles": {
                "type": "object",
                "additionalProperties": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "backend": { "type": "string", "enum": ["native", "codex"] },
                        "apiProvider": { "type": "string", "enum": VALID_API_PROVIDERS },
                        "model": { "type": "string" },
                        "availableModels": { "type": "array", "items": { "type": "string" } },
                        "modelReasoningEffort": {
                            "type": "string",
                            "enum": ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
                        },
                        "modelCapabilities": {
                            "type": "object",
                            "additionalProperties": {
                                "type": "object",
                                "additionalProperties": true,
                                "properties": {
                                    "displayName": { "type": "string" },
                                    "description": { "type": "string" },
                                    "defaultReasoningLevel": { "type": "string" },
                                    "supportedReasoningLevels": { "type": "array", "items": { "type": "string" } },
                                    "contextWindow": { "type": "integer", "minimum": 1 },
                                    "maxContextWindow": { "type": "integer", "minimum": 1 },
                                    "effectiveContextWindowPercent": { "type": "integer", "minimum": 1, "maximum": 100 },
                                    "supportsFastMode": { "type": "boolean" },
                                    "supportsReasoningSummaries": { "type": "boolean" },
                                    "supportVerbosity": { "type": "boolean" },
                                    "supportsParallelToolCalls": { "type": "boolean" },
                                    "supportsImageDetailOriginal": { "type": "boolean" },
                                    "supportsSearchTool": { "type": "boolean" },
                                    "supportedInApi": { "type": "boolean" },
                                    "inputModalities": { "type": "array", "items": { "type": "string" } },
                                    "serviceTiers": { "type": "array", "items": { "type": "string" } }
                                }
                            }
                        },
                        "baseUrl": { "type": "string" },
                        "apiKey": { "type": "string" },
                        "env": {
                            "type": "object",
                            "additionalProperties": { "type": "string" }
                        },
                        "authSource": true
                    }
                }
            },
            "theme": { "type": "string" },
            "verbose": { "type": "boolean" },
            "permissionMode": {
                "type": "string",
                "enum": ["default", "ask", "auto", "bypass", "plan", "acceptEdits", "dontAsk"]
            },
            "allowedTools": { "type": "array", "items": { "type": "string" } },
            "permissions": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "defaultMode": {
                        "type": "string",
                        "enum": ["default", "ask", "auto", "bypass", "plan", "acceptEdits", "dontAsk"]
                    },
                    "allow": { "type": "array", "items": { "type": "string" } },
                    "ask": { "type": "array", "items": { "type": "string" } },
                    "deny": { "type": "array", "items": { "type": "string" } },
                    "additionalDirectories": {
                        "type": "array", "items": { "type": "string" }
                    },
                    "enableBypassMode": { "type": "boolean" },
                    "skipDangerousModePermissionPrompt": { "type": "boolean" },
                    "enableAutoMode": { "type": "boolean" },
                    "autoMode": {
                        "type": "object",
                        "additionalProperties": true,
                        "properties": {
                            "environment": { "type": "array", "items": { "type": "string" } },
                            "allow": { "type": "array", "items": { "type": "string" } },
                            "softDeny": { "type": "array", "items": { "type": "string" } }
                        }
                    }
                }
            },
            "sandbox": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "enabled": { "type": "boolean" },
                    "mode": {
                        "type": "string",
                        "enum": ["read-only", "workspace", "full"]
                    },
                    "failIfUnavailable": { "type": "boolean" },
                    "allowUnsandboxedCommands": { "type": "boolean" },
                    "allowManagedReadPathsOnly": { "type": "boolean" },
                    "allowManagedDomainsOnly": { "type": "boolean" },
                    "excludedCommands": {
                        "type": "array", "items": { "type": "string" }
                    },
                    "allowedCommands": {
                        "type": "array", "items": { "type": "string" }
                    },
                    "filesystem": {
                        "type": "object",
                        "additionalProperties": true,
                        "properties": {
                            "allowRead": { "type": "array", "items": { "type": "string" } },
                            "denyRead": { "type": "array", "items": { "type": "string" } },
                            "allowWrite": { "type": "array", "items": { "type": "string" } },
                            "denyWrite": { "type": "array", "items": { "type": "string" } }
                        }
                    },
                    "network": {
                        "type": "object",
                        "additionalProperties": true,
                        "properties": {
                            "disabled": { "type": "boolean" },
                            "allowedDomains": {
                                "type": "array", "items": { "type": "string" }
                            },
                            "httpProxyPort": { "type": "integer", "minimum": 0, "maximum": 65535 },
                            "socksProxyPort": { "type": "integer", "minimum": 0, "maximum": 65535 }
                        }
                    }
                }
            },
            "kairos": {
                "type": "object",
                "additionalProperties": true,
                "description": "Persistent KAIROS desired feature profile. Environment variables remain higher-priority compatibility overrides.",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "brief": { "type": "boolean" },
                    "channels": { "type": "boolean" },
                    "pushNotifications": { "type": "boolean" },
                    "githubWebhooks": { "type": "boolean" },
                    "proactive": { "type": "boolean" }
                }
            },
            "hooks": { "type": "object", "additionalProperties": true },
            "statusLine": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["none", "minimal", "command", "script"]
                    },
                    "command": { "type": "string" },
                    "script": { "type": "string" },
                    "format": { "type": "string" },
                    "enabled": { "type": "boolean" },
                    "padding": { "type": "integer", "minimum": 0, "maximum": 64 },
                    "refreshIntervalMs": { "type": "integer", "minimum": 100, "maximum": 5000 },
                    "timeoutMs": { "type": "integer", "minimum": 100, "maximum": 30000 }
                }
            },
            "outputStyle": { "type": "string" },
            "language": { "type": "string" },
            "voiceEnabled": { "type": "boolean" },
            "editorMode": { "type": "string", "enum": ["normal", "vim"] },
            "viewMode": {
                "type": "string",
                "enum": ["prompt", "transcript", "focus"],
                "description": "Default Rust TUI view mode at startup. Invalid persisted values fall back to prompt mode."
            },
            "hermesEnabled": {
                "type": "boolean",
                "description": "Enable Hermes autonomous runtime surfaces. Merge rule is On wins across user/project layers."
            },
            "spinnerTips": {
                "type": "object",
                "additionalProperties": true,
                "description": "Rust TUI spinner tip settings. Custom tips rotate while preserving the main spinner status text.",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "intervalMs": { "type": "integer", "minimum": 1 },
                    "customTips": {
                        "type": "array", "items": { "type": "string" }
                    }
                }
            },
            "terminalProgressBarEnabled": { "type": "boolean" },
            "appIcon": {
                "type": "string",
                "deprecated": true,
                "description": "Legacy Electron app icon setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "autoStart": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron auto-start setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "startMinimized": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron start-minimized setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "minimizeToTray": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron tray setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "closeToTray": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron tray setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "quickChatHideOnBlur": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron quick-chat setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "quickChatInjectScreen": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron quick-chat setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "quickChatAmbient": {
                "type": "boolean",
                "deprecated": true,
                "description": "Legacy Electron quick-chat setting. Accepted for compatibility, but unsupported by the Rust TUI and has no runtime effect."
            },
            "autoApproveTools": {
                "type": "boolean",
                "description": "Compatibility field only. It is not wired to Rust permissions and has no runtime effect; use permissionMode or permissions.defaultMode instead."
            },
            "analyticsEnabled": {
                "type": "boolean",
                "description": "Compatibility field only. Analytics collection is not implemented and this setting has no runtime effect."
            },
            "availableModels": {
                "type": "array", "items": { "type": "string" }
            },
            "defaultModel": { "type": "string" },
            "contextWindow": { "type": "integer", "minimum": 1 },
            "maxMessages": { "type": "integer", "minimum": 1 },
            "autoTitle": { "type": "boolean" },
            "temperature": { "type": "number", "minimum": 0, "maximum": 2 },
            "maxTokens": { "type": "integer", "minimum": 1 },
            "streaming": { "type": "boolean" },
            "showTokenUsage": { "type": "boolean" },
            "showReasoningDetails": { "type": "boolean" },
            "markdownRendering": { "type": "boolean" },
            "singleDollarMath": { "type": "boolean" },
            "infographic": { "type": "boolean" },
            "autoCollapseReasoning": { "type": "boolean" },
            "quickReplySuggestions": { "type": "boolean" },
            "defaultToolSelection": { "type": "string" },
            "defaultSkillSelection": { "type": "string" },
            "soundEffects": { "type": "boolean" },
            "autoCompact": {
                "type": "boolean",
                "description": "Controls only threshold-triggered full auto-compact summarization. Local budget/snip/microcompact/context-collapse and prompt-too-long recovery still run when false."
            },
            "compactThreshold": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "Context-window percentage that triggers full auto-compact summarization. Defaults to 80."
            },
            "keepRecentMessages": {
                "type": "integer",
                "minimum": 1,
                "maximum": 255,
                "description": "Recent conversation turns retained by the snip stage. Defaults to 200."
            },
            "hashlineMode": { "type": "boolean" },
            "fallbackModel": { "type": "string" },
            "fastModel": { "type": "string" },
            "sotaModel": { "type": "string" },
            "motaModel": { "type": "string" },
            "fotaModel": { "type": "string" },
            "effortLevel": {
                "type": "string",
                "description": "Anthropic fixed thinking-budget label (low|medium|high|xhigh|auto|max) or a positive integer token count. Provider boundary: Anthropic only. Codex requests ignore this field; use authProfiles.<profile>.modelReasoningEffort for the Codex reasoning.effort channel."
            },
            "thinking": {
                "type": ["object", "boolean", "string"],
                "description": "Anthropic thinking toggle. Object form accepts type=enabled/adaptive/disabled."
            },
            "output_config": {
                "type": "object",
                "properties": {
                    "effort": {
                        "type": "string",
                        "description": "Claude output effort (Anthropic wire). low/medium map to high; xhigh and any other non-empty value map to max. Provider boundary: Anthropic only. Do not set this on Codex profiles — Use authProfiles.<profile>.modelReasoningEffort instead."
                    }
                },
                "additionalProperties": true,
                "description": "Anthropic output_config passthrough. effort controls Claude-side reasoning strength. Provider boundary: Anthropic only. The /effort command stops writing here on Codex profiles."
            },
            "model_reasoning_effort": {
                "type": "string",
                "enum": ["none", "minimal", "low", "medium", "high", "xhigh", "max"],
                "description": "Codex/OpenAI Responses reasoning effort. Directly maps to reasoning.effort for openai-codex requests. Provider boundary: Codex only. Per-profile version: authProfiles.<profile>.modelReasoningEffort. Root-level value remains readable for backward compatibility but active profile and per-turn override take precedence."
            },
            "fastMode": { "type": "boolean" },
            "fastModePerSessionOptIn": { "type": "boolean" },
            "teammateMode": {
                "type": "boolean",
                "description": "Compatibility field only. Rust TUI teammate display modes are not implemented and this setting has no runtime effect."
            },
            "claudeInChromeDefaultEnabled": {
                "type": "boolean",
                "description": "Browser integration default consumed by allthecodes-browser, not by the Rust TUI."
            },
            "autoMemoryEnabled": { "type": "boolean" },
            "memoryAutoRetrieve": { "type": "boolean", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryQueryRewriting": { "type": "boolean", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryMaxRetrieved": { "type": "integer", "minimum": 1, "maximum": 255, "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memorySimilarityThreshold": { "type": "integer", "minimum": 1, "maximum": 255, "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryAutoSummarize": { "type": "boolean", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryNightly": { "type": "boolean", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memorySleepTime": { "type": "string", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryTempTtl": { "type": "integer", "minimum": 1, "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryArchiveRetention": { "type": "integer", "minimum": 1, "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryToolModel": { "type": "string", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "memoryEmbeddingModel": { "type": "string", "description": "Reserved for future memory runtime tuning; currently has no runtime effect." },
            "proxyEnabled": { "type": "boolean" },
            "proxyUrl": { "type": "string" },
            "preferIpv4": { "type": "boolean" },
            "requestTimeout": { "type": "integer", "minimum": 1 },
            "retryAttempts": { "type": "integer", "minimum": 1, "maximum": 255 },
            "customUserAgent": { "type": "string" },
            "speechEnabled": { "type": "boolean" },
            "speechActiveModel": { "type": "string" },
            "speechLanguage": { "type": "string" },
            "ttsProvider": { "type": "string" },
            "ttsApiKey": { "type": "string" },
            "ttsVoice": { "type": "string" },
            "ttsVoiceCustomId": { "type": "string" },
            "ttsModel": { "type": "string" },
            "searchEngine": { "type": "string" },
            "cloudSyncEnabled": { "type": "boolean" },
            "cloudSyncPath": { "type": "string" },
            "tokenSavingsTracking": { "type": "boolean" },
            "advisorModel": {
                "type": "string",
                "description": "Advisor model consumed by supported native providers."
            },
            "systemPrompt": { "type": "string" },
            "apiKey": { "type": "string" },
            "env": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "mcpBindings": {
                "type": "array",
                "description": "Explicit MCP server bindings by runtime scope. Legacy mcpServers still create compatibility bindings.",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "required": ["serverId", "scope"],
                    "properties": {
                        "serverId": { "type": "string" },
                        "scope": {
                            "type": "string",
                            "enum": ["global", "project", "session", "thread"]
                        },
                        "projectPath": { "type": "string" },
                        "sessionId": { "type": "string" },
                        "threadId": { "type": "string" },
                        "permissions": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["connect", "list_tools", "call_tools", "read_resources"]
                            }
                        },
                        "readOnly": { "type": "boolean" },
                        "sourceScope": { "type": "string" }
                    }
                }
            }
        }
    })
}
