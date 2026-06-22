# TUI 思考图标扫光实现指南

> 日期：2026-06-21
> 范围：Rust TUI，主要位于 `crates/allthecodes/src/ui/`
> 目标：为 TUI 的 thinking 行增加从左到右的浅深色循环效果，对齐 Web UI 的 thinking shimmer 气质，但使用 Ratatui 能稳定表达的实现方式。

---

## 1. 目标效果

Web UI 可以用 CSS `linear-gradient + background-position + background-clip` 做连续扫光；TUI 没有像素级渐变和 CSS 动画，所以实现目标应调整为：

- active thinking 行的图标/短标签呈现从左到右移动的高亮波。
- 默认颜色为 `theme.thinking` 或 dim thinking 色。
- 高亮颜色使用现有 accent/info 类颜色，不新增主题配置时优先复用已有字段。
- completed / redacted / transcript 静态历史内容不播放动画。
- 动画由现有 TUI tick 驱动，不新增计时线程。

视觉上不是像素渐变，而是多 `Span` 分段着色：

```text
∴ Thinking…
```

每一帧把其中 1-3 个字符设为高亮色，下一帧高亮窗口右移，循环后恢复浅色。

---

## 2. 当前入口

| 功能 | 当前文件 | 当前行为 |
|------|----------|----------|
| thinking 内容渲染 | `crates/allthecodes/src/ui/messages/assistant_thinking_message.rs` | verbose/transcript 时渲染静态 `∴ Thinking…` 和正文 |
| redacted thinking 渲染 | `crates/allthecodes/src/ui/messages/assistant_redacted_thinking_message.rs` | verbose/transcript 时渲染静态 `✻ Thinking…` |
| assistant block 分发 | `crates/allthecodes/src/ui/messages/render/render_assistant.rs` | 将 `ContentBlock::Thinking` 交给 thinking renderer |
| 消息区渲染 | `crates/allthecodes/src/ui/messages/render/mod.rs` | 接收 `streaming: bool`，仅给最后一条 assistant message 追加光标 `▌` |
| app tick | `crates/allthecodes/src/ui/app.rs` | `App::tick()` 每 16ms 调用；spinner 每 5 tick 约 80ms 推进一帧 |
| loading 组件参考 | `crates/allthecodes/src/ui/components/loading_state.rs` | 已有 caller-controlled frame 模式 |

关键约束：

- `render_assistant_thinking_lines()` 当前没有 frame 入参。
- `render_messages()` 当前只有 `streaming: bool`，没有动画帧上下文。
- `App::tick()` 只有 spinner active 时才稳定把 `dirty = true`，thinking shimmer 需要在 streaming 且存在 active thinking 时触发重绘。

---

## 3. 推荐设计

### 3.1 增加消息渲染动画上下文

在 `messages/render/context.rs` 或相邻模块中扩展现有 render options，而不是从全局状态读取：

```rust
pub struct MessageRenderOptions {
    pub verbose: bool,
    pub is_transcript_mode: bool,
    pub show_all_in_transcript: bool,
    pub thinking_animation_frame: Option<usize>,
}
```

语义：

- `Some(frame)`：当前 prompt-mode 正在 streaming，可播放 active thinking shimmer。
- `None`：静态渲染；transcript/focus/export/snapshot 不播放动画。

如果不想污染 `MessageRenderOptions`，可新增 `MessageAnimationState { thinking_frame: Option<usize> }` 并放入 `MessageRenderContext`，但不要让 thinking renderer 直接依赖 `App`。

### 3.2 App 驱动帧号

在 `App` 中复用 `tick_counter` 或新增只读派生方法：

```rust
fn thinking_animation_frame(&self) -> Option<usize> {
    self.is_streaming.then_some((self.tick_counter / 5) as usize)
}
```

规则：

- 和 spinner 一样每 5 tick 推进一帧，约 80ms。
- `App::tick()` 在 `self.is_streaming` 时也要定期 `self.dirty = true`，否则消息区不会重绘。
- 不需要单独存 frame；用 `tick_counter / 5` 派生即可。

### 3.3 Thinking renderer 分段着色

在 `assistant_thinking_message.rs` 中新增 helper：

```rust
pub fn render_thinking_label(frame: Option<usize>, theme: &Theme) -> Line<'static>
```

推荐行为：

- `frame == None`：返回现有静态 `Line::from(Span::styled("∴ Thinking…", theme.thinking))`。
- `Some(frame)`：把 label 拆成 chars，按移动窗口生成多个 `Span`。
- 高亮窗口宽度为 2 个字符；相邻字符可用中间色，终端里保持柔和。

伪代码：

```rust
const LABEL: &str = "∴ Thinking…";

fn shimmer_style_for(pos: usize, frame: usize, len: usize, theme: &Theme) -> Style {
    let head = frame % (len + 3);
    let distance = pos.abs_diff(head);
    match distance {
        0 => theme.info,       // 或 theme.accent / theme.thinking_highlight
        1 => theme.thinking,   // 可加 bold，避免过亮
        _ => theme.dim,
    }
}
```

注意：

- Unicode 字符按 `chars()` 拆，不能按 byte index。
- 合并连续同 style 字符，避免每个字符都生成一个 span 造成过多分配。
- 正文 thinking 内容保持 `theme.thinking`，只给第一行 label 做动画。

### 3.4 Redacted thinking

`assistant_redacted_thinking_message.rs` 默认保持静态。

原因：

- redacted thinking 通常代表历史/策略隐藏状态，不是正在生成的 active thinking。
- 如果后续确实有 active redacted thinking，可另加 frame 入参，但 v1 不做。

---

## 4. 实施步骤

### Phase 1：接入 animation frame

修改：

- `crates/allthecodes/src/ui/app.rs`
- `crates/allthecodes/src/ui/app/render.rs`
- `crates/allthecodes/src/ui/messages/render/context.rs`

要求：

- prompt-mode 渲染 `build_message_render_context_with_options()` 时传入 `thinking_animation_frame`。
- transcript/focus/export 路径传 `None`。
- `App::tick()` 在 `is_streaming` 时每 5 tick 标记 dirty，即使 spinner 被 immediate notification 暂时隐藏。

### Phase 2：实现 thinking label shimmer

修改：

- `crates/allthecodes/src/ui/messages/assistant_thinking_message.rs`
- `crates/allthecodes/src/ui/messages/render/render_assistant.rs`

要求：

- `AssistantThinkingView` 增加 `animation_frame: Option<usize>`。
- `render_assistant_thinking_lines()` 第一行调用 `render_thinking_label()`。
- 只有 `verbose || is_transcript_mode` 且 body 非空时仍按现有规则展示 thinking 行，避免改变可见性策略。
- `is_transcript_mode` 强制不播放动画，即使误传 frame 也应静态渲染。

### Phase 3：测试和快照

新增或更新测试：

- `assistant_thinking_message` 单元测试：
  - `None` frame 返回静态 label。
  - `Some(0)` 与 `Some(1)` 的 spans 样式不同。
  - 输出纯文本仍为 `∴ Thinking…`。
- `render_assistant` 或 `messages/render` 测试：
  - streaming prompt-mode 传递 frame。
  - transcript-mode 不播放动画。
- `app::tests`：
  - `App::tick()` 在 streaming 时推进 frame 并置 dirty。

建议命令：

```bash
cargo test -p allthecodes assistant_thinking_message
cargo test -p allthecodes messages::render
cargo test -p allthecodes app::tests
```

---

## 5. 验收标准

- TUI streaming 期间，thinking label 的高亮区随 tick 从左到右移动。
- 完成后的历史 thinking、redacted thinking、transcript/focus/export 输出保持静态。
- thinking 正文内容、复制文本、会话存储和协议内容不变。
- 小终端下不增加行数，不影响 wrapping 和 virtual scroll 高度计算。
- 没有新增线程、sleep、async timer；动画完全由现有 `AppEvent::Tick` 驱动。

---

## 6. 不做事项

- 不在终端里模拟 CSS 像素渐变。
- 不为这个局部效果新增主题 schema 字段。
- 不改变 thinking 是否展示的产品策略。
- 不改变 `ContentBlock::Thinking` / `RedactedThinking` 协议结构。
- 不让 redacted thinking 默认播放动画。
