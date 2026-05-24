# Claude 默认模型落点清单

本文只记录会影响运行时行为的生产代码默认值和回退点。测试、快照、示例数据不展开，避免把验证样例误认为产品逻辑。

## 结论

项目里把模型默认到 Claude 系列的地方，主要集中在这几条链路：

1. 启动时的模型决策，默认落到 `claude-sonnet-4-20250514`
2. Anthropic / Bedrock / Vertex / Keychain 认证链路里的 provider default
3. `AppState` / `ToolAppState` / `QueryEngine` 的运行时初始值
4. 查询与自动压缩的兜底模型
5. `/fast` 命令触发的强制切换
6. 会话导出与系统提示打印中的兜底模型
7. 模型别名映射到 Claude 系列全量 ID

## 生产代码位置

### 1. 启动期模型决策

- `[crates/claude-code-rs/src/main.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/main.rs#L48)` 的 `resolve_startup_model()` 会按 `requested -> provider_default -> hardcoded_default -> first allowed` 依次回退。
- 同文件 `[main.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/main.rs#L669)` 里，非 codex 后端的 `hardcoded_default` 被写死为 `"claude-sonnet-4-20250514"`。
- 该结果随后写入 `AppState.main_loop_model` 和 `QueryEngineConfig.resolved_model`，因此会成为整个会话的初始主模型。

### 2. Provider 默认模型

- `[crates/cc-api/src/api/providers.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/providers.rs#L115)` 里，`anthropic` provider 的 `default_model` 是 `"claude-sonnet-4-20250514"`。
- 同文件 `[providers.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/providers.rs#L167)` 里，`openrouter` provider 默认值是 `"anthropic/claude-sonnet-4"`，仍然指向 Claude 系列。
- `[crates/cc-api/src/api/client/mod.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/client/mod.rs#L581)` 的 `from_provider_info()` 会直接把 `ProviderInfo.default_model` 写进 `ApiClientConfig.default_model`。
- `[client/mod.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/client/mod.rs#L619)` 的 `from_env_result()` 在检测到 provider 后，会把 provider 默认模型一路传到 `ApiClient`。

### 3. 认证路径里的 Claude 回退

- `[crates/cc-api/src/api/client/mod.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/client/mod.rs#L718)` 的 Bedrock 分支在 `ANTHROPIC_MODEL` 为空时，回退到 `"claude-sonnet-4-5-20250929"`。
- `[client/mod.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/client/mod.rs#L756)` 的 Vertex 分支使用同样的默认值。
- `[client/mod.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-api/src/api/client/mod.rs#L869)` 的 keychain / auth 兜底路径会把 Anthropic 客户端的 `default_model` 固定为 `"claude-sonnet-4-20250514"`。
- `[crates/cc-commands/src/login.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-commands/src/login.rs#L176)` 的 Bedrock / Vertex 状态文本也把空 `ANTHROPIC_MODEL` 显示为 Claude 默认值，分别用于 `/login` 的诊断输出。

### 4. 运行时状态默认值

- `[crates/cc-engine/src/types/app_state.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/types/app_state.rs#L76)` 的 `AppState::default()` 把 `main_loop_model` 初始化为 `"claude-sonnet-4-20250514"`。
- `[crates/cc-tools/src/tool.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-tools/src/tool.rs#L42)` 的 `ToolAppState::default()` 也使用同一个默认值。
- `[crates/cc-engine/src/lifecycle/mod.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/lifecycle/mod.rs#L145)` 会把 `QueryEngineConfig.resolved_model` 写入 `AppState.main_loop_model`。
- `[crates/cc-engine/src/types/config.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/types/config.rs#L228)` 明确标注 `resolved_model` 用于初始化 `AppState.main_loop_model`。

### 5. 请求构建与自动压缩兜底

- `[crates/cc-engine/src/lifecycle/deps.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/lifecycle/deps.rs#L208)` 的 `prepare_model_call_params_for_client()` 在请求没有 model 时，优先用 `app_model`，否则用 `client.config().default_model`。
- `[deps.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/lifecycle/deps.rs#L232)` 的 `model_for_autocompact()` 在没有 client 且没有 app_model 时，兜底到 `"claude-sonnet-4-20250514"`。
- `[deps.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/lifecycle/deps.rs#L606)` 和 `[deps.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/lifecycle/deps.rs#L634)` 的 reactive compact / collapse drain 都在 `main_loop_model` 为空时回退到 `client.config().default_model`，再无 client 时回退到 `"claude-sonnet-4-20250514"`。
- `[crates/cc-engine/src/lifecycle/helpers.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-engine/src/lifecycle/helpers.rs#L204)` 的 `build_messages_request()` 也把空 model 回退为 `"claude-sonnet-4-20250514"`。

### 6. `/fast` 命令

- `[crates/cc-commands/src/fast.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-commands/src/fast.rs#L17)` 定义 `FAST_MODE_MODEL = "claude-opus-4-6-20250414"`。
- 同文件 `[fast.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-commands/src/fast.rs#L75)` 在当前模型不支持 fast mode 时，会自动把 `main_loop_model` 和 `settings.model` 切到这个 Opus 模型。

### 7. 导出和调试路径

- `[crates/cc-session/src/session_export/builders.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-session/src/session_export/builders.rs#L90)` 在会话导出上下文中，`effective_model()` 为空时回退到 `"claude-sonnet-4-20250514"`。
- `[crates/start-up/src/fast_paths.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/start-up/src/fast_paths.rs#L89)` 的 `run_dump_system_prompt()` 也会在没有 CLI model 且没有 provider default 时回退到 `"claude-sonnet-4-20250514"`。

### 8. 模型别名

- `[crates/cc-models/src/aliases.rs](/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/cc-models/src/aliases.rs#L11)` 把 `SOTA/MOTA/FOTA` 分别映射到：
- `SOTA -> claude-opus-4-20250514`
- `MOTA -> claude-sonnet-4-20250514`
- `FOTA -> claude-haiku-3-5-20241022`

## 不计入本清单的内容

- 单元测试里的硬编码模型字符串
- snapshot / fixture / 示例 JSON
- 仅用于展示或文案的字符串

这些位置会出现大量 Claude 模型 ID，但不代表生产默认值。
