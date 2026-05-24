# Snapshot 测试构建指南

本文说明 `codex-rs/tui` 中 `src/snapshots` 的构建方法、用法、作用和限制。这里的“构建 snapshot”指运行 snapshot 测试，让测试代码渲染稳定输出，再由 `insta` 生成或比对 `.snap` 基线文件。

## 适用范围

主要适用于上游 Codex Rust TUI：

- 源码目录：`F:\AIclassmanager\cc\codex\codex-rs\tui`
- Snapshot 基线目录：`F:\AIclassmanager\cc\codex\codex-rs\tui\src\snapshots`
- 其他模块也可能有自己的 `snapshots` 子目录，例如 `src\chatwidget\snapshots`、`src\bottom_pane\snapshots`

如果 cc-rust 后续引入同类 TUI snapshot 测试，可以复用同一套流程。

## Snapshot 是什么

Snapshot 文件是测试输出的已审核基线。测试运行时会重新生成当前输出，并和已提交的 `.snap` 文件比较：

- 输出一致：测试通过。
- 输出不同：测试失败，并生成 `*.snap.new` 等待人工审核。
- 新增测试没有基线：生成新的 `*.snap.new`。

`.snap` 文件通常包含头部元数据和正文：

```text
---
source: tui/src/diff_render.rs
expression: terminal.backend()
---
...
```

头部说明 snapshot 来自哪个测试源文件、断言表达式是什么。正文是被捕获的字符串、TUI buffer、终端画面或其他可序列化显示结果。

## 构建方法

### 1. 准备工具

`codex-rs` 已经在 workspace 中声明了 `insta` 依赖。审核和接受 snapshot 需要 `cargo-insta`：

```powershell
cargo install cargo-insta
```

如果只是运行测试，通常不需要单独安装 `cargo-insta`；只有查看 pending snapshot 或接受基线时才需要。

### 2. 运行 TUI 测试生成 snapshot

从 `codex-rs` 目录运行：

```powershell
cd F:\AIclassmanager\cc\codex\codex-rs
cargo test -p codex-tui
```

这一步会执行 `codex-tui` 的单元测试和 snapshot 测试。测试中的 `insta::assert_snapshot!` 会把当前输出和已有 `.snap` 基线比对。

如果只想运行某个测试，可以加测试名过滤：

```powershell
cargo test -p codex-tui diff_gallery_80x24
```

### 3. 查看待审核 snapshot

测试失败且输出变化符合预期时，先查看 pending 列表：

```powershell
cargo insta pending-snapshots -p codex-tui
```

查看某个新文件内容：

```powershell
cargo insta show -p codex-tui path\to\file.snap.new
```

也可以直接打开仓库中的 `*.snap.new` 文件阅读。重点检查：

- UI 文本是否符合预期。
- 换行、缩进、截断、边框是否稳定。
- 路径、时间、平台差异是否被测试固定或规避。
- 变化是否只来自本次意图，而不是无关渲染漂移。

### 4. 接受 snapshot

确认所有 pending snapshot 都是预期变化后执行：

```powershell
cargo insta accept -p codex-tui
```

接受后，`*.snap.new` 会变成正式 `.snap` 基线。需要把更新后的 `.snap` 文件和代码改动一起提交。

## Snapshot 的来源

TUI snapshot 通常有两类来源。

### 终端画面 snapshot

测试创建固定尺寸的 `ratatui::Terminal`，渲染组件，然后断言 backend：

```rust
let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
terminal
    .draw(|f| {
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .render_ref(f.area(), f.buffer_mut())
    })
    .expect("draw");

insta::assert_snapshot!("diff_gallery_80x24", terminal.backend());
```

有些测试使用项目自定义的 `VT100Backend`，它基于 `vt100::Parser` 模拟真实终端，适合验证 crossterm/ANSI 行为、光标和真实终端输出。

### 文本 snapshot

测试把内部渲染结构转成字符串，再断言文本：

```rust
let rendered = render_lines(&cell.display_lines(/*width*/ 60)).join("\n");
insta::assert_snapshot!(rendered);
```

这种方式适合验证 history cell、状态行、命令输出摘要等纯文本结构。

## 文件命名规则

`insta` 会根据 crate、模块路径和 snapshot 名生成文件名。典型格式：

```text
codex_tui__diff_render__tests__diff_gallery_80x24.snap
```

含义：

- `codex_tui`：crate 名。
- `diff_render__tests`：模块路径。
- `diff_gallery_80x24`：测试中给出的 snapshot 名，或测试函数名。

某些测试会自定义 snapshot 目录或名称。例如 `chatwidget` 测试通过 helper 把 snapshot 固定写到 `src/chatwidget/snapshots`，并显式控制文件名前缀。平台差异可能使用 suffix：

```rust
insta::with_settings!({ snapshot_suffix => "windows" }, {
    assert_chatwidget_snapshot!("approvals_selection_popup", popup);
});
```

这会生成类似 `name@windows.snap` 的平台专用基线。

## 用法

Snapshot 测试适合用于：

- 验证 TUI 组件在固定宽高下的完整渲染结果。
- 锁定用户可见文本、边框、缩进、换行、截断和颜色退化后的显示。
- 捕获大块文本输出的回归，例如 diff 摘要、状态面板、权限弹窗、审批弹窗。
- 让 PR 审核者直接看到 UI 输出变化，而不是只读渲染代码。

新增或修改用户可见 UI 时，一般流程是：

1. 写普通逻辑测试覆盖关键行为。
2. 为用户可见输出添加或更新 snapshot。
3. 运行 `cargo test -p codex-tui` 生成变化。
4. 审核 `*.snap.new`。
5. 用 `cargo insta accept -p codex-tui` 接受预期变化。

## 作用

Snapshot 的主要价值是降低 UI 回归成本：

- 把复杂终端输出固化成可 review 的文本基线。
- 对布局变化非常敏感，能捕获不易用字段断言发现的问题。
- 适合覆盖宽度、高度、平台、降级样式等组合。
- 让重构有安全网，只要输出不变，snapshot 测试就能证明行为保持稳定。

## 限制

Snapshot 不是完整测试策略的替代品。

- 它只证明输出和基线一致，不证明业务逻辑正确。
- 它容易受到环境影响，例如操作系统、路径分隔符、终端宽度、Unicode 宽度、颜色支持、时间和随机值。
- 它对大范围 UI 重排很敏感，可能产生大量 diff，需要人工判断哪些变化是预期的。
- 它不能自动判断新输出是否“更好”，只能提示“变了”。
- 它不适合直接包含真实密钥、用户路径、机器名、当前时间、网络结果等不稳定内容。
- 在 Windows 上，多宽字符、ANSI、路径和默认字体相关行为可能和 Unix 不同，必要时应使用平台专用 snapshot 或在测试中规范化输出。
- `*.snap.new` 不是最终基线，必须审核并 accept 后才应提交。

## 稳定性建议

- 固定终端尺寸，例如 80x24、120x40。
- 固定输入数据，不使用当前时间、真实 HOME、随机 UUID 或网络响应。
- 对路径、临时目录、时间戳做脱敏或规范化。
- 对平台差异明确分支，必要时使用 `snapshot_suffix`。
- 对纯逻辑使用 `assert_eq!`，只把用户可见渲染交给 snapshot。
- 不要无脑接受所有 snapshot。先看 diff，再 accept。
- Snapshot 变更应和对应 UI 或文本改动在同一个 PR 中提交。

## Bazel 注意事项

`codex-rs/tui/BUILD.bazel` 会把 `src/**/snapshots/**` 加入测试数据。新增 snapshot 目录或文件时，要确保它仍然落在 Bazel 已声明的数据路径中。否则 Cargo 测试可能通过，但 Bazel 测试可能找不到基线文件。

## 常见问题

### 测试生成了 `.snap.new`，是否直接提交？

不要直接提交 `.snap.new`。先审核内容，再运行：

```powershell
cargo insta accept -p codex-tui
```

提交正式 `.snap` 文件。

### 修改 UI 后 snapshot 测试失败怎么办？

先判断失败是否符合预期：

- 如果是预期变化，审核并 accept。
- 如果是不相关变化，回到代码中修正渲染或测试输入。
- 如果只在某个平台失败，检查路径、Unicode 宽度、ANSI 支持和平台条件分支。

### 什么时候不该用 snapshot？

以下场景优先用普通断言：

- 只验证一个布尔值、枚举值、计数或状态迁移。
- 输出包含大量不稳定字段。
- 测试目标是协议结构、序列化字段或错误类型。
- 失败时需要精确指出某个字段，而不是审查整块输出。

## 最小检查清单

提交 snapshot 相关改动前确认：

- `cargo test -p codex-tui` 已运行。
- `cargo insta pending-snapshots -p codex-tui` 没有未处理项。
- 所有 `.snap` diff 都已人工审核。
- 没有提交 `*.snap.new`。
- 新 snapshot 的测试输入稳定、终端尺寸固定。
- 用户可见 UI 变化有对应 snapshot 覆盖。
