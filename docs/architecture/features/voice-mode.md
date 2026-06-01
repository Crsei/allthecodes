---
title: "语音模式"
description: "Push-to-Talk 语音输入功能，支持双 STT 后端：Anthropic 和豆包 ASR，实现按键说话并实时转录。"
keywords: ["voice", "语音", "STT", "push-to-talk", "转录", "ASR"]
---

## 概述

语音模式（Voice Mode）实现"按键说话"（Push-to-Talk）语音输入。用户按住快捷键录音，音频流式传输到 STT 后端，实时转录显示在终端中。当前 build 中，录音和转录后端为存根（NullAudioBackend + NullTranscriptionClient），明确声明不支持真实录音，但完整的兼容层已就绪。

## 架构

### 模块结构

`allthecodes-voice` crate 分为 5 个模块：

| 模块 | 文件 | 职责 |
|------|------|------|
| `controller` | `controller.rs` | 状态机：Idle → Recording → Transcribing → Idle |
| `audio` | `audio.rs` | 音频捕获后端抽象 + NullAudioBackend |
| `stt` | `stt.rs` | 语音转文本客户端抽象 + NullTranscriptionClient |
| `language` | `language.rs` | 语言代码规范化（BCP-47 → ISO-639-1） |
| `feasibility` | `feasibility.rs` | 可行性检查（构建支持、认证、远程环境） |

### 状态机

`VoiceController` 实现 Push-to-Talk 状态机：

```
Idle → 用户按下按键 → Recording
Recording → 用户释放按键 → Transcribing
Transcribing → STT 返回结果 → Idle
任意状态 → cancel() → Idle
Recording/Transcribing → 出错 → Error
```

状态转换通过 `VoiceEvent` 队列通知 TUI：

- `StateChanged(VoiceState)` — 状态变更
- `Transcription(String)` — 最终转录文本
- `Error(String)` — 错误信息

### 音频捕获抽象

`AudioCaptureBackend` trait 定义音频后端接口：

```rust
pub trait AudioCaptureBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> Result<(), AudioUnavailable>;
    fn start(&self) -> Result<RecordingHandle, AudioUnavailable>;
}
```

`RecordingHandle` 包含：
- `audio` — PCM 帧的 `UnboundedReceiver<Vec<u8>>`（16kHz mono s16le）
- `stopped` — 停止标志，通过 `stop()` 或 Drop 触发

### STT 抽象

`TranscriptionClient` trait 定义 STT 后端接口：

```rust
pub trait TranscriptionClient: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> Result<(), SttUnavailable>;
    async fn transcribe(&self, handle: RecordingHandle, language: &str)
        -> Result<TranscriptionResult, SttError>;
}
```

### 语言规范化

`normalize_language_for_stt()` 函数将用户配置的语言设置映射到 STT 端点支持的 ISO-639-1 代码：

| 用户设置 | 规范化代码 |
|----------|-----------|
| `en`, `en-US`, `english` | `en` |
| `zh`, `zh-CN`, `chinese`, `中文` | `zh` |
| `ja`, `ja-JP`, `japanese`, `日本語` | `ja` |
| `ko`, `ko-KR`, `korean`, `한국어` | `ko` |
| `es`, `es-ES`, `spanish`, `español` | `es` |
| `fr`, `fr-FR`, `french`, `français` | `fr` |
| 未知语言 | `en`（带回落提示） |

### 可行性检查

`check_feasibility()` 按优先级检查语音可用性：

1. **构建级别**：当前 build 是否支持录音/转录
2. **认证级别**：`AuthMethod::None` → 要求登录；`ApiKey` → 不支持语音；仅 `OAuthToken(claude_ai)` 可通过
3. **远程环境**：`CC_RUST_REMOTE` 或 `CLAUDE_CODE_REMOTE` 环境变量设置时阻止
4. **运行级别**：音频后端和 STT 后端的实时可用性

## 设计决策

1. **当前 build 明确不支持录音**：`NullAudioBackend` 和 `NullTranscriptionClient` 作为默认实现，`/voice` 命令直接报告未支持状态
2. **Trait 边界保留**：即使当前不可用，接口已对齐 TypeScript 原版，未来接入真实后端无需修改控制器
3. **语言规范化**：严格对齐 `claude-code-bun` 的 `normalizeLanguageForSTT` 映射表
4. **可行性检查分层**：构建级别不支持优先于认证检查，避免提示用户登录一个没有实现的功能
5. **Tokio 任务驱动**：转录在后台任务中异步执行，控制器通过 mpsc 通道接收事件

## 使用方式

```bash
# 当前 build 中，语音模式声明为未支持
# 运行 /voice 查看状态
# 预期输出："Voice capture is unsupported in this build of cc-rust"

# 启用 feature（未来真实后端接入后）
FEATURE_VOICE_MODE=1 cargo run

# 在终端中使用
# 1. /voice 启用语音模式
# 2. 按住快捷键录音
# 3. 释放按键完成转录

# 设置语言
# settings.json: { "language": "zh-CN" }
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-voice/src/controller.rs` | 状态机实现（VoiceController） |
| `crates/allthecodes-voice/src/audio.rs` | 音频捕获抽象（AudioCaptureBackend + NullAudioBackend） |
| `crates/allthecodes-voice/src/stt.rs` | STT 抽象（TranscriptionClient + NullTranscriptionClient） |
| `crates/allthecodes-voice/src/language.rs` | 语言代码规范化 |
| `crates/allthecodes-voice/src/feasibility.rs` | 可行性检查（构建、认证、远程环境） |
| `crates/allthecodes-voice/src/mod.rs` | 模块入口 |
