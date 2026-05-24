# TUI 命令显示、模型切换与 DeepSeek 兼容修复记录

日期：2026-05-23

## 背景

本轮修复集中处理 Rust TUI 中若干命令显示、配置同步和 Anthropic-compatible 网关兼容问题：

- `/branch` 成功后展示内部 fork 细节，用户不容易知道如何回到父会话。
- `/effort` 无参数只打印帮助，无法像交互面板一样选择 thinking effort。
- `/cost` token 汇总不够紧凑，且没有展示 OpenAI/Codex/compatible provider 返回的 reasoning token。
- `/login codex` 或 Codex CLI import 后，live backend 已切到 Codex，但 footer、`/model` 和下一次请求模型仍可能沿用旧的 `deepseek-v4-pro`。
- DeepSeek Anthropic-compatible 网关会因历史 assistant thinking block 回放返回 `content[].thinking must be passed back`。
- `/context` 把缺失的 system prompt/tools schema 显示成 0，容易被理解成真实 API view。
- `/config` surface 中存在当前无真实运行价值或容易误导的项。
- `/brief` 仍作为可发现命令暴露，但本阶段暂时不希望用户入口使用它。

## 修复内容

### `/branch`

- 成功提示改为面向用户的文案：
  `Branched conversation. You are now in the new branch (session <new>).`
- 提示中包含 `/resume <parent>` 和当前二进制 `-r <parent>` 的恢复方式。
- 不再展示 `Forked session ->`、copied transcript entries、title 等内部 fork 信息。
- 保持原有 `SwitchSession` 行为，fork 后后续消息进入新 session。

### `/effort`

- TUI 中输入 `/effort` 无参数会打开 Thinking/Effort picker。
- `/effort <low|medium|high|auto|max|number>` 会同步更新：
  - live `AppState.effort_value`
  - live `settings.effortLevel`
  - user settings 文件中的 `effortLevel`
- `/config` Thinking picker 仍提交 `/config set effortLevel <id>`，并在描述中标明当前值和选择后立即生效。

### `/cost`

- 首行改为紧凑格式：
  `Token usage: total=<input+output> input=<input> (+ <cached> cached) output=<output>`
- 保留 API calls、cache read、cache creation、estimated cost 等明细行。
- `cc_types::message::Usage` 新增 `reasoning_output_tokens`，默认 0。
- OpenAI/Codex/compatible SSE usage 中的 `completion_tokens_details.reasoning_tokens` 或 `output_tokens_details.reasoning_tokens` 会被解析并在 `/cost` 中显示。

### Codex 登录与模型同步

- `/login codex`、Codex CLI import、OpenAI Codex OAuth 完成后同步设置：
  - `apiProvider=openai-codex`
  - `backend=codex`
  - live `main_loop_model`
  - persisted `model`
- Codex 默认模型优先级：
  1. 进程环境 `OPENAI_CODEX_MODEL`
  2. user settings `env.OPENAI_CODEX_MODEL`
  3. settings 中已有 `model`
  4. Codex provider 默认模型

#### 2026-05-23 追加：Codex CLI 切换后 `/model` 仍显示 DeepSeek

用户反馈：切换到 `codex-cli` 后，`/model` 的模型列表仍然显示 `deepseek-v4-pro`。

根因：

- model picker 只要检测到 `ANTHROPIC_DEFAULT_*` 环境变量，就按 Anthropic-compatible alias 映射显示，即使当前 `apiProvider` 已经明确是 `openai-codex`。
- `/login codex-cli` 切换 provider 时没有替换旧的 `availableModels`，用户 settings 中残留的 DeepSeek allow-list 会继续影响 `/model` 和下次启动的模型解析。

修复：

- `apiProvider` 明确存在时，provider 判断以 `apiProvider` 为准；只有没有明确 provider 时才使用 `ANTHROPIC_*` 环境变量推断 Anthropic-compatible。
- Codex provider selection 会同步更新 live 和 persisted `availableModels`，保留 Codex 相关模型/alias，过滤掉旧 DeepSeek 条目。
- 旧 settings 中的 `model=deepseek-v4-pro` 不再作为 Codex 默认模型候选；除非显式设置 `OPENAI_CODEX_MODEL`，否则回退到 Codex provider 默认模型。

#### 2026-05-23 追加：SOTA/MOTA/FOTA 由 settings 覆盖

用户反馈：`SOTA_MODEL_ID`、`MOTA_MODEL_ID`、`FOTA_MODEL_ID` 不应写死在代码中；应允许在 settings 中配置，并让 settings 的模型设置具有最高优先级。

修复：

- 新增 settings 字段：`sotaModel`、`motaModel`、`fotaModel`。
- 有 settings 上下文的路径优先用这些字段解析 `SOTA`、`MOTA`、`FOTA`，再回退到 provider 环境映射或内置 fallback。
- 覆盖范围包括启动模型解析、`/model`、`/config set model`、`/fast` 默认模型、Codex login/import 后的模型 allow-list、TUI model picker。
- 更新 settings schema 和模型配置文档，保留 `cc_models` 中的常量作为无 settings 上下文时的 fallback。

#### 2026-05-23 追加：Codex model_reasoning_effort

用户反馈：Codex 模型不直接支持设置 reasoning token 数，应该支持 Codex 原生的 `model_reasoning_effort`。

修复：

- 新增 settings 字段 `model_reasoning_effort`，支持 `none`、`minimal`、`low`、`medium`、`high`、`xhigh`。
- `openai-codex` 请求会写入 Responses API body：`reasoning: { "effort": ... }`，并请求 `reasoning.encrypted_content`。
- `/config set model_reasoning_effort high` 会同步 live settings 并持久化到 cc-rust settings。
- 优先级：`model_reasoning_effort` 最高；未设置时，`effortLevel` 的 `low`/`medium`/`high` 会作为 Codex fallback，`max` 映射为 `xhigh`，数字 token budget 不映射到 Codex。

#### 2026-05-23 追加：`/model` 串联 `/effort`，并由 auth profile 模型能力驱动

用户反馈：`/model` 和 `/effort` 应按当前登录的 auth profile 可用模型与模型能力工作，不应继续从全局 settings、旧 DeepSeek allow-list 或硬编码别名中混合推断。

修复：

- settings 扩展 `authProfiles.<profile>.modelCapabilities`，能力字段覆盖 display name、description、默认 reasoning level、支持的 reasoning levels、context window、fast/search/parallel/image/verbosity 等开关。
- settings 扩展 profile 级 `modelReasoningEffort`；本轮之后 `/effort` 的主要持久化位置是当前 `authProfiles.<active>.modelReasoningEffort`，不再把 root-level `effortLevel` 作为主要写入口。
- `/login codex` 会初始化或刷新 Codex profile 的模型表与模型能力：
  - `gpt-5.5`
  - `gpt-5.4`
  - `gpt-5.4-mini`
  - `gpt-5.3-codex`
  - `gpt-5.3-codex-spark`
  - `gpt-5.2`
- 隐藏模型 `codex-auto-review` 不进入普通 `/model` 可选列表。
- `/model` 只展示当前 active auth profile 中同时存在于 `availableModels` 和 `modelCapabilities` 的模型；未配置能力的 profile 不再回退展示旧模型。
- TUI `/model` 选择模型后返回新的 `SubmitThenOpen` outcome：先提交 `/model <id>`，命令成功后自动打开当前 state 的 `/effort` 面板。
- `/effort` 面板按当前模型的 `supportedReasoningLevels` 渲染选项；`auto` 使用 `defaultReasoningLevel`；不支持的 reasoning level 会被拒绝。
- 如果当前 profile/模型没有配置 reasoning levels，`/effort` 显示只读提示，不允许选择。
- `/fast` 兼容性判断改为读取当前模型能力的 `supportsFastMode`，缺失配置时默认不支持。
- `/context` 和 context analysis 增加 settings/profile context window 注入路径；能力缺失时保留既有 200k 默认回退。
- `/config` 中 model/effort 改为只读展示入口；`/config set model`、`/config set effortLevel`、`/config set modelReasoningEffort` 返回只读提示，引导使用 `/login`、`/model`、`/effort`。

### DeepSeek Anthropic-compatible thinking 回放

- 非官方 Anthropic-compatible 请求继续清理：
  - top-level `thinking`
  - `context_management`
  - prompt-cache 扩展字段
  - assistant `thinking` / `redacted_thinking` content blocks
- 这覆盖 TUI streaming 产生的空 thinking block，避免 DeepSeek 网关在后续 `/anthropic` 请求中报 `content[].thinking must be passed back`。
- 官方 Anthropic 请求路径不改变，仍保留 signed thinking 回传能力。

### `/context`

- 文案改成 `Estimated Conversation Context`。
- 默认输出不再把缺失的 system prompt/tools schema 显示为真实 0。
- JSON 输出新增：
  - `unavailable_categories`
  - `estimation_notes`
- 分类中使用 `cached input`、`hook results`、`free` 等估算口径。

### `/config`

- 隐藏以下 surface 项：
  - voice 开关
  - terminal progress bar
  - raw/schema 重复入口
- 保留：
  - Status
  - Model
  - Theme
  - Thinking
  - Usage
  - Output style
  - Language
  - Safety sources

### `/brief`

- 从默认 command registry 移除 `/brief`，因此 slash completion、command palette、help/CLI reference 不再暴露它。
- 保留 `brief.rs` 和 runtime adapter，避免破坏 Kairos/历史内部能力。

## 主要文件

- `crates/cc-commands/src/branch.rs`
- `crates/cc-commands/src/effort.rs`
- `crates/cc-commands/src/cost.rs`
- `crates/cc-commands/src/context.rs`
- `crates/cc-commands/src/login.rs`
- `crates/cc-commands/src/login_code.rs`
- `crates/cc-commands/src/lib.rs`
- `crates/cc-commands/src/model.rs`
- `crates/cc-commands/src/fast.rs`
- `crates/cc-commands/src/config_cmd.rs`
- `crates/cc-types/src/message.rs`
- `crates/cc-api/src/api/openai_compat.rs`
- `crates/cc-api/src/api/streaming.rs`
- `crates/cc-compact/src/context_analysis.rs`
- `crates/cc-config/src/settings/providers.rs`
- `crates/cc-config/src/settings/effective.rs`
- `crates/cc-config/src/settings/schema.rs`
- `crates/cc-config/src/runtime_settings.rs`
- `crates/cc-utils/src/tokens.rs`
- `crates/claude-code-rs/src/ui/app.rs`
- `crates/claude-code-rs/src/ui/command_surface/`
- `crates/claude-code-rs/src/ui/command_palette/`
- `docs/USAGE_GUIDE.md`

## 验证

已通过：

```bash
cargo fmt --all --check
git diff --check
cargo test -p cc-commands branch
cargo test -p cc-commands effort
cargo test -p cc-commands cost
cargo test -p cc-commands context
cargo test -p cc-commands login
cargo test -p cc-commands
cargo test -p cc-config settings::
cargo test -p cc-utils tokens
cargo test -p cc-api compatible_anthropic
cargo test -p cc-api test_parse_codex_responses_text_stream
cargo test -p claude-code-rs command_surface
cargo test -p claude-code-rs ui::command_surface
cargo test -p claude-code-rs command_palette
cargo test -p cc-compact context_analysis
cargo test -p cc-compact
cargo test -p cc-engine --no-run
cargo build --workspace --release
```

未执行真实 DeepSeek 手动 smoke；该项需要可用的 DeepSeek Anthropic-compatible 凭据和运行时配置。
