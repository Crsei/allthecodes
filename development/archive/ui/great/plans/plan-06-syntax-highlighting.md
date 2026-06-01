# 执行计划：语法高亮

> **生成日期**: 2026-05-18
> **来源**: `docs/ui/great/06_missing_features_summary.md` §7
> **参考实现**:
> - TS 高亮: `F:\AIclassmanager\cc\src\utils\cliHighlight.ts` (58 行, WASM Shiki)
> - TS 集成: `F:\AIclassmanager\cc\src\components\Markdown.tsx` (Suspense + use(highlight))
> - TS Markdown: `F:\AIclassmanager\cc\src\utils\markdown.ts` (382 行, formatToken 高亮集成)
> - Rust Markdown: `crates/claude-code-rs/src/ui/rendering/markdown.rs` (259 行)
> - Rust Markdown 渲染: `crates/claude-code-rs/src/ui/rendering/markdown_render.rs` (63 行)
> - Rust 依赖: `crates/claude-code-rs/Cargo.toml` (syntect 和 tree-sitter 声明为 optional)
> - Rust 差异视图: `crates/claude-code-rs/src/ui/diff/` (261+ 行)

---

## 当前状态

语法高亮是 Rust UI 最大的单一渲染差距，标记为 **0/5** 完成。

### Cargo.toml 依赖声明
```toml
# syntect — Syntax highlighting via fancy-regex (no onig C dep).
# tree-sitter — AST parsing.
# Both are optionally gated and currently unused.
syntect = { workspace = true, optional = true }
tree-sitter = { workspace = true, optional = true }

[features]
syntect = ["dep:syntect"]
tree-sitter = ["dep:tree-sitter"]
```

两个 crate 作为 optional 依赖声明在 `Cargo.toml` 中，并有对应的 feature flags，但**零使用** — `crates/claude-code-rs/src/` 中没有 `use syntect` 或 `use tree_sitter` 语句。它们在 `Cargo.lock` 中解析但未被链接。

### Markdown 解析 (`markdown.rs`)
```rust
// 第 103 行 — CodeBlock 信息字符串被完全丢弃
Event::Start(Tag::CodeBlock(_)) => {
    flush_line(&mut current_spans, &mut lines);
    in_code_block = true;
    style_stack.push(theme.code);
}
```

`pulldown_cmark` 的 `Tag::CodeBlock(CodeBlockKind::Fenced(lang))` 提供语言标识符，但当前代码使用 `_` 通配符丢弃它。所有代码块使用 `theme.code` 单一样式渲染。

### 渲染流程
```
markdown_to_lines_inner(text, theme)
  → pulldown_cmark::Parser 产生事件流
  → Tag::CodeBlock(_) 仅推入 theme.code 样式
  → Event::Text(t) 添加为 Span::raw()
  → 输出 Vec<Line<'a>> (Style + &str)
```

整个过程中**没有令牌级着色**。

### 差异视图 (`diff/`)
差异代码行同样仅有 +/- 行级着色，没有语法高亮。TS 差异视图在代码块内应用高亮。

## 目标状态

1. **语法高亮引擎** — 启用 `syntect` feature 并使用其同步 API 进行代码块高亮
2. **语言检测** — 从 `Tag::CodeBlock(CodeBlockKind::Fenced(lang))` 提取语言标识符，映射至 syntect 语法包
3. **令牌级着色** — 代码块内的文本根据 tokens 应用不同 Style（关键字、字符串、注释、类型等不同颜色）
4. **纯文本回退** — 对 syntect 不支持的语言或无语言的 fence 块使用 `theme.code` 样式
5. **缓存** — 对已渲染的代码块进行缓存，避免重复高亮（复用现有 LRU 缓存机制）
6. **差异视图语法高亮** — 在结构化差异代码块中应用高亮（与行级 +/- 着色叠加）
7. **语言选择器 UI** — 当无法检测或用户希望覆盖语言时，可选择语言的下拉选择器
8. **`tree-sitter` 备用预留** — 代码中预留 tree-sitter 接口位置，但不作为第一阶段实现

## TS 参考

### `cliHighlight.ts` (58 行)
- 懒加载 `cli-highlight` (基于 highlight.js)
- `getCliHighlightPromise()`: 单例 Promise，跨模块共享
- `supportsLanguage()`: 检测语言支持
- `highlight(text, { language })`: 返回 ANSI 转义字符串
- `getLanguageName(file_path)`: 通过文件扩展名 → highlight.js 语言名

### Markdown 高亮集成 (`markdown.ts`)
- `formatToken()` 函数: 将 Markdown token 转为 Ink 组件，高亮作为 `cliHighlight.highlight()` 调用
- 代码块语言来自 fence info_string → `cliHighlight.highlight(text, { language })`
- 不支持的语言回退至 `plaintext`
- 懒加载期间显示无高亮回退 (~50ms)

### 差异视图高亮
TS 差异代码块同样应用语法高亮，与行级 +/- 叠加。

## Rust 当前代码

### `markdown.rs` 代码块处理（第 103-113 行）
```rust
Event::Start(Tag::CodeBlock(_)) => {
    flush_line(&mut current_spans, &mut lines);
    in_code_block = true;
    style_stack.push(theme.code);
}
Event::End(TagEnd::CodeBlock) => {
    flush_line(&mut current_spans, &mut lines);
    in_code_block = false;
    style_stack.pop();
    lines.push(Line::from(""));
}
```

### `markdown_render.rs` (63 行)
薄外观层，`wrap_lines()` 在换行时丢弃所有区间级样式。

### `Cargo.toml` syntect 配置
```toml
syntect = { workspace = true, optional = true }
[features]
syntect = ["dep:syntect"]
```

在 `workspace` 级别的 `Cargo.toml` 中，syntect 声明为无 `default-features`，使用 `fancy-regex` 特性避免 oniguruma C 依赖。

## 分步实施

### 阶段 1: syntect 集成基础设施（~120 行）

- [ ] 1.1 新建 `crates/claude-code-rs/src/ui/rendering/syntax_highlight.rs`
  - 定义 `SyntaxHighlighter` 结构体: 持有 `syntect::parsing::SyntaxSet` + `syntect::highlighting::ThemeSet`
  - 实现 `new()`: lazy-init（`OnceLock` 或 `LazyLock`）加载默认语法包
  - 实现 `highlight(code: &str, lang: &str) -> Result<Vec<(Style, &str)>, ()>`:
    - 从 lang 查找语法 → `syntax_set.find_syntax_by_token(lang)`
    - 调用 `syntect::highlight::HighlightLines::highlight()` → 产生带样式的 token 区间
    - 将 syntect 的 `Style` 转换为 ratatui `Style`
    - 回退: lang 为空或不支持时返回 `Err(())`，由调用方使用 `theme.code`
  - 实现 `supports_language(lang: &str) -> bool`
  - 编译门控: `#[cfg(feature = "syntect")]` 整个模块

- [ ] 1.2 颜色转换工具
  - `syntect::highlighting::Style` → `ratatui::style::Style` 转换函数
  - syntect 使用 RGBA，ratatui 使用 `Color::Rgb(u8, u8, u8)`
  - 支持前景色和字体样式（粗体/斜体/下划线）

- [ ] 1.3 增加 `theme_syntax` 字段到 `Theme` 结构体
  - 为不同语法 token 类型提供 `syntect::highlighting::Theme`
  - 或使用内置主题之一（如 `base16-ocean.dark`）

### 阶段 2: Markdown 代码块语言检测（~30 行）

- [ ] 2.1 修改 `markdown.rs` 中的 `Tag::CodeBlock` 匹配
  ```rust
  // 从: Event::Start(Tag::CodeBlock(_)) => {
  // 改为:
  Event::Start(Tag::CodeBlock(kind)) => {
      let lang = match kind {
          CodeBlockKind::Fenced(info) => {
              // 提取语言标识符 (info_string 中第一个空格前的部分)
              info.split_whitespace().next().unwrap_or("").to_string()
          }
          CodeBlockKind::Indented => String::new(),
      };
  ```
- [ ] 2.2 在代码块事件处理器中存储 `lang` 用于高亮调用
- [ ] 2.3 `flush_code_block()`: 在代码块结束时，如有 lang 则调用语法高亮

### 阶段 3: 令牌级着色（~150 行）

- [ ] 3.1 实现代码块内的令牌级渲染:
  - 收集代码块内所有 `Event::Text(t)` 的文本到一个缓冲区
  - 在 `TagEnd::CodeBlock` 处调用 `SyntaxHighlighter::highlight(code, lang)`
  - 将返回的带样式区间转换为 `Vec<Span>` 追加到输出行
  - 若高亮失败（lang 不支持或未启用 feature），回退到 `theme.code` + 原始文本

- [ ] 3.2 考虑 syntect `#[cfg(feature = "syntect")]` 编译门控:
  - 启用 syntect feature 时: 使用 `SyntaxHighlighter`
  - 未启用时: 保持现有 `theme.code` 回退行为
  - 使 `cargo build --release` (不含 syntect) 依然可用

- [ ] 3.3 缓存:
  - 对代码块文本+lang 计算哈希，缓存高亮结果
  - 使用现有 `LruCache` 或 `lru` crate
  - 流式场景中，缓存失效策略: 当代码块正在流式传输时，每次追加新文本都重新高亮

### 阶段 4: 差异视图语法高亮（~80 行）

- [ ] 4.1 在 `structured_diff.rs` 的代码块渲染中集成高亮
  - 在差异行级 +/- 着色之上叠加语法令牌着色
  - 默认行类型颜色（绿色/红色）作为背景或降权，语法高亮覆盖前景
- [ ] 4.2 保持差异语义: 添加行绿色、删除行红色背景，语法高亮覆盖前景文本颜色
- [ ] 4.3 提取 diff 上下文中的 `lang` 来自差异头部（如 `diff --git a/foo.rs b/foo.rs` 中的 `.rs` 扩展名）

### 阶段 5: 语言选择器 UI（~80 行）

- [ ] 5.1 当语言检测失败或用户按特定键时，显示语言选择器
  - 可选语言列表来自 `SyntaxSet::find_syntax_plain_text()` 可用的语法
  - 使用模糊匹配过滤
- [ ] 5.2 在代码块上方或覆盖层渲染下拉选择框
  - 显示语言名称，可用 ↑↓ 选择，Enter 确认
- [ ] 5.3 将选择结果回传至 Markdown 渲染器，使用选定语言重新高亮

### 阶段 6: 流式代码块高亮优化（~60 行）

- [ ] 6.1 `MarkdownStreamCollector` 在流式代码块过程中:
  - 当前: 整个缓冲区在每一帧重新渲染
  - 高亮后: 在代码块流式传输时，仅重新高亮新增文本，保留已有令牌的样式
  - syntect 的 `HighlightLines` 支持增量风格（`highlight()` 方法逐行调用）
- [ ] 6.2 使用逐行高亮实现增量更新:
  - 每行代码块文本单独调用 `HighlightLines::highlight()`
  - 新行追加时仅高亮新行，无需重做整个块

### 阶段 7: 测试（~80 行）

- [ ] 7.1 单元测试: 语言映射表、颜色转换、回退逻辑
- [ ] 7.2 快照测试: 多种语言的语法高亮输出（Rust、Python、TypeScript、JSON、HTML）
- [ ] 7.3 边缘情况: 空代码块、极长行、嵌套在列表中的代码块、不支持的语言
- [ ] 7.4 映射表测试: `pulldown_cmark` 语言标识 → syntect 语法名称

## 工作量估算

| 阶段 | 内容 | 估算行数 | 复杂度 |
|:----:|------|:--------:|:------:|
| 1 | syntect 集成基础设施 | ~120 | 中 |
| 2 | 语言检测 | ~30 | 低 |
| 3 | 令牌级着色 | ~150 | 高 |
| 4 | 差异视图高亮 | ~80 | 中 |
| 5 | 语言选择器 UI | ~80 | 中 |
| 6 | 流式优化 | ~60 | 中 |
| 7 | 测试 | ~80 | 低 |
| **合计** | | **~600** | |

## 依赖/前提

1. **syntect crate** — 已在 `Cargo.toml` 中声明为 optional，需 feat gate 启用
2. **无 oniguruma C 依赖** — syntect 使用 `fancy-regex` feature（已在 workspace Cargo.toml 中配置 `default-features = false`）
3. **pulldown_cmark 已升级** — `CodeBlockKind::Fenced(lang)` 在较新版本中可用（确认当前版本兼容）
4. **编译门控** — `syntect` feature 不应是默认 feature，保持 `cargo build --release` 无高亮回退
5. **性能考虑** — 长代码块的语法高亮是本机原生（非 WASM），预期在亚毫秒级完成。但仍需 LRU 缓存避免重复高亮相同文本
6. **tree-sitter 暂不启用** — 第一阶段仅使用 syntect，tree-sitter 作为 future work 预留

### 预期的面向用户体验
- 启用 syntect feature 后: 代码块自动获得语法高亮，语言从 fence info_string 检测
- 未启用 syntect feature 时: 维持现状（`theme.code` 单色样式）
- 高亮语言库: syntect 内置约 200+ 种语法包，覆盖常见编程语言
- 性能: 每个代码块高亮通常在 <1ms（本地原生），无需 WASM 加载延迟

### 风险
- syntect 的 `fancy-regex` 依赖可能在极端正则表达式上出现性能退化（罕见）
- 语言标识符映射: `pulldown_cmark` 提取的语言字符串需映射至 syntect 语法名（如 `js` → `JavaScript`、`py` → `Python`）
- 长代码块（>500 行）首次高亮可能产生可观延迟，缓存可缓解

## 实施后遗留问题（2026-05-20）

本计划的 Markdown fenced code block 高亮已接入：代码块语言会从 fence info string 提取，常见别名会映射到 syntect 语法，失败时回退到 `theme.code`，并保留缓存/快照测试覆盖。`cargo check -p claude-code-rs` 和 `cargo build --workspace --release` 未产生 Rust 生产警告。

仍需在后续计划中跟踪：

1. 差异视图的令牌级语法高亮尚未作为生产路径完成；当前差异 UI 仍以行级 add/remove/context 样式为主，需要单独把文件扩展名语言推断与 diff 前景色叠加策略接入。
2. 语言选择器 UI 与用户覆盖语言的交互尚未实现；当前语言仅来自 Markdown fence。
3. 流式代码块仍按现有 Markdown 渲染路径刷新，没有实现 syntect `HighlightLines` 的逐行增量复用；长代码块性能需要后续基准验证。
4. 测试目标中可能继续出现与语言枚举/未来 UI 入口相关的 `dead_code` 警告；生产构建路径保持干净。
