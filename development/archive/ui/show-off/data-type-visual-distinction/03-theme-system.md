# 第三章：主题系统索引

本文索引 `data-type-visual-distinction.md` 第三章「主题系统」对应的 Rust TUI 实现，重点说明内置主题、主题类型、语义色到 `ratatui::style::Style` 的落点，以及 ANSI、Daltonized、语法高亮、diff 和 Agent 颜色的边界。

## 1. 源码入口

| 路径 | 角色 | 主要类型/函数 |
|---|---|---|
| `crates/allthecodes/src/ui/theme/mod.rs` | 设计系统主题主入口；定义 6 套内置主题、运行时 provider、旧渲染主题桥接 | `ThemeName`, `ThemeSetting`, `ThemeColors`, `ThemeProvider`, `get_theme`, `load_theme_setting`, `Theme::from_design_colors` |
| `crates/allthecodes/src/ui/theme/color.rs` | 颜色解析器；把主题 key、hex、RGB、ANSI 字符串解析为 `ratatui::style::Color` | `resolve_color`, `resolve_theme_key`, `parse_hex`, `parse_rgb`, `parse_ansi256`, `parse_ansi_name` |
| `crates/allthecodes/src/ui/rendering/theme.rs` | legacy 渲染主题结构；多数消息、Markdown、工具活动和 diff 仍消费这个 `Theme` | `Theme`, `ThemeKind`（测试用）, `Theme::default` |
| `crates/allthecodes/src/ui/rendering/syntax_highlight.rs` | fenced code block 的语法高亮入口 | `highlight_code_block`, `supports_language`, `supported_languages`（后两者测试用） |
| `crates/allthecodes/src/ui/rendering/markdown.rs` | Markdown 渲染；把 `Theme` 用到标题、链接、代码块等内容 | `markdown_to_lines`, `markdown_to_lines_inner` |
| `crates/allthecodes/src/ui/diff.rs` | 普通 diff、word-level diff 的样式消费方 | `render_word_diff_line`, `render_word_diff_spans` |
| `crates/allthecodes/src/ui/diff/structured_diff.rs` | 结构化 diff 预览；上下文行可叠加语法高亮 | `render_structured_diff_preview`, `render_context_line_spans` |
| `crates/allthecodes/src/ui/agents/color_picker.rs` | Agent 颜色选择器的文本模型 | `COLOR_OPTIONS`, `ColorPickerState` |
| `crates/allthecodes-teams/src/constants.rs` | Team/teammate 的颜色池 | `AGENT_COLORS`, `TEAMMATE_COLOR_ENV_VAR` |

## 2. 六套内置主题

`ThemeName` 是所有内置主题的枚举。当前有 6 个变体，`get_theme(&ThemeName)` 通过 `OnceLock<[ThemeColors; 6]>` 懒加载并返回静态颜色表。

| `ThemeName` | 设置值 | 说明 | 是否深色 |
|---|---|---|---|
| `Dark` | `dark` | 默认深色主题；`ThemeProvider::new()` 和无配置时使用 | 是 |
| `Light` | `light` | 浅色主题 | 否 |
| `LightDaltonized` | `light-daltonized` | 浅色色盲友好变体；以蓝/橙替代部分红/绿语义 | 否 |
| `DarkDaltonized` | `dark-daltonized` | 深色色盲友好变体；以蓝/橙替代部分红/绿语义 | 是 |
| `LightAnsi` | `light-ansi` | 浅色 ANSI 兼容变体；主要语义色使用终端 0-15 索引 | 否 |
| `DarkAnsi` | `dark-ansi` | 深色 ANSI 兼容变体；主要语义色使用终端 0-15 索引 | 是 |

`ThemeSetting` 在 `ThemeName` 外再包一层 `Auto`。`auto` 会读取 `COLORFGBG` 最后一段背景色编号：`0..=6` 或 `8` 认为偏深，`7` 或 `9..=15` 认为偏浅；无法判断时默认深色。配置读取入口是 `load_theme_setting()`，读取 allthecodes 用户设置文件中的 `"theme"` 字段；未知值回退到 `Dark`。

## 3. ThemeColors 配色表

`ThemeColors` 是完整颜色表，字段名刻意沿用上游 TypeScript `Theme` 接口命名。它不是 `Style`，只保存 `ratatui::style::Color`。

| 分组 | 字段 | 用途 |
|---|---|---|
| 核心调色板 | `accent`, `accentDim`, `accentText`, `inverted`, `invertedText` | 品牌主色、反相文本 |
| 语义状态 | `success`, `error`, `warning`, `suggestion`, `info` | 成功、错误、警告、建议、信息提示 |
| 表面/边框 | `surface`, `surfaceText`, `muted`, `mutedText`, `inactive`, `inactiveText`, `border`, `borderFocus`, `borderError`, `permission`, `permissionText` | 面板、边框、权限提示、不可用态 |
| 排版 | `dim`, `bold`, `link`, `code`, `codeBg`, `heading`, `blockquote`, `blockquoteBorder`, `hr` | Markdown 和正文排版 |
| 输入/选择 | `selection`, `selectionText`, `cursor`, `cursorText`, `searchHighlight`, `searchHighlightText` | 选中态、光标、搜索命中 |
| Diff | `diffAdd`, `diffAddBg`, `diffRemove`, `diffRemoveBg`, `diffHeader`, `diffContext` | 增删行、背景、hunk/header、上下文 |
| 语法高亮 | `syntaxKeyword`, `syntaxString`, `syntaxNumber`, `syntaxType`, `syntaxFunction`, `syntaxComment`, `syntaxOperator`, `syntaxPunctuation`, `syntaxBuiltin`, `syntaxConstant`, `syntaxVariable`, `syntaxParameter`, `syntaxLabel` | 代码 token 的设计系统颜色 |
| 状态图标 | `iconSuccess`, `iconError`, `iconWarning`, `iconInfo`, `iconPending`, `iconLoading` | 组件级状态图标 |
| Agent | `agentRed`, `agentOrange`, `agentYellow`, `agentGreen`, `agentCyan`, `agentBlue`, `agentPurple`, `agentPink` | Agent 标签/点位预留色 |

`theme/color.rs` 的 `resolve_color()` 支持以下输入：

| 输入形式 | 示例 | 结果 |
|---|---|---|
| 主题 key | `success`, `border-focus`, `agentRed` | 查 `ThemeColors` 字段；大小写不敏感，忽略 `-` 和 `_` |
| Hex | `#ffcc00` | `Color::Rgb(255, 204, 0)` |
| RGB | `rgb(255,204,0)` | `Color::Rgb(255, 204, 0)` |
| ANSI 256 | `ansi256(208)` | `Color::Indexed(208)` |
| ANSI 名称 | `ansi:red`, `ansi:bright_blue` | `Color::Indexed(1)` / `Color::Indexed(12)` |

限制：hex 只接受 6 位；`rgb()` 必须是 3 个 `u8`；无法解析时返回 `None`，调用方负责 fallback。

## 4. ThemeProvider 运行时模型

`ThemeProvider` 是 TUI 运行时主题状态容器，字段上只保存当前 `ThemeName`，测试模式下额外保留 `ThemeSetting`。核心职责：

| 方法 | 行为 |
|---|---|
| `new()` | 默认 `ThemeName::Dark` |
| `from_user_settings()` | 从 allthecodes 用户设置读取 `"theme"`；失败时记录 warn 并回退 `Dark` |
| `from_setting_str()` | 从字符串解析 `ThemeSetting` |
| `colors()` | 返回当前 `ThemeColors` |
| `legacy_theme()` | 调用 `Theme::from_design_colors(self.colors())`，生成旧渲染系统使用的 `Theme` |
| `set_theme`, `set_setting`, `refresh_auto`, `all_themes` | 测试配置/枚举辅助，目前带 `#[cfg(test)]` |

`App` 同时保存 `design_theme_provider: ThemeProvider` 和 `theme: Theme`。启动时 `App::new()` 使用 `ThemeProvider::from_user_settings()`，随后生成 legacy `Theme`；`App::set_theme_setting()` 会同时替换 provider 和 legacy theme。`tui.rs` / `tui/commands.rs` 会把后端 `AppState.settings.theme` 同步到 `App`。

## 5. 语义颜色到 ratatui Style 的映射

多数渲染器仍消费 `crates/allthecodes/src/ui/rendering/theme.rs` 的 legacy `Theme`。设计系统颜色通过 `Theme::from_design_colors(colors: &ThemeColors)` 转换为 `ratatui::style::Style`：

| legacy `Theme` 字段 | 来源 `ThemeColors` | `Style` 修饰 | 主要用途 |
|---|---|---|---|
| `assistant_name` | `accent` | `BOLD` | 助理名称 |
| `user_name` | `suggestion` | `BOLD` | 用户名称 |
| `system_name` | `dim` | `ITALIC` | 系统名称/系统行 |
| `tool_name` | `code` | `BOLD` | 工具名 |
| `tool_result` | `diffContext` | 无 | 工具输出 |
| `error` | `error` | `BOLD` | 错误文本 |
| `warning` | `warning` | 无 | 警告文本 |
| `info` | `info` | 无 | 信息文本 |
| `prompt` | `suggestion` | `BOLD` | 输入提示符 |
| `border` | `border` | 无 | 边框/分隔线 |
| `code` | `code` + `codeBg` | fg + bg | 行内代码和代码块 fallback |
| `code_bg` | `codeBg` | 颜色值 | 代码块背景 |
| `thinking` | `dim` | `ITALIC` | thinking 块 |
| `dim` | `dim` | 无 | 次要文本 |
| `heading` | `heading` | `BOLD | UNDERLINED` | Markdown 标题 |
| `bold` | `bold` | `BOLD` | Markdown strong |
| `italic` | 无前景色 | `ITALIC` | Markdown emphasis |
| `link` | `link` | `UNDERLINED` | Markdown link |
| `syntax_keyword` 等 | 对应 `syntax*` | comment 额外 `ITALIC` | 设计系统语法 token 样式 |
| `diff_add` | `diffAdd` | 无 | diff 新增 |
| `diff_remove` | `diffRemove` | 无 | diff 删除 |
| `diff_context` | `diffContext` | 无 | diff 上下文 |
| `diff_header` | `diffHeader` | `BOLD` | diff header/hunk |
| `selected` | `selectionText` + `selection` | fg + bg + `BOLD` | 选中项 |
| `unselected` | `inactiveText` | 无 | 未选中项 |
| `progress_fill` | `success` | 无 | 进度条填充 |
| `progress_empty` | `inactive` | 无 | 进度条空槽 |

这层映射是“设计系统颜色表 -> legacy 渲染样式”的关键桥。新增主题只要补齐 `ThemeColors`，绝大多数旧渲染代码可以通过 `legacy_theme()` 自动吃到新颜色。

## 6. ANSI 与 Daltonized 的差异

### 6.1 Daltonized

`LightDaltonized` 从 `light_theme()` 派生，`DarkDaltonized` 从 `dark_theme()` 派生。当前只替换少量依赖红/绿区分的字段：

| 字段组 | LightDaltonized | DarkDaltonized |
|---|---|---|
| 成功/错误 | `success = blue`, `error = orange` | `success = blue`, `error = orange` |
| diff 前景 | `diffAdd = blue`, `diffRemove = orange` | `diffAdd = blue`, `diffRemove = orange` |
| diff 背景 | 浅蓝/浅橙背景 | 深蓝/深橙背景 |
| icon | `iconSuccess = success`, `iconError = error` | 同左 |
| 语法 | `syntaxKeyword`, `syntaxString`, `syntaxBuiltin` 调整为橙/蓝/黄倾向 | 同类调整 |

限制：源码注释明确写有 `TODO: load from upstream daltonized palette`。也就是说 Daltonized 当前是局部替换，不是完整上游色盲友好调色板；表面、边框、Agent 色等大多继承普通 Light/Dark。

### 6.2 ANSI

`LightAnsi` 和 `DarkAnsi` 分别从普通 Light/Dark 派生，只把主要语义色改成 ANSI 索引：

| 字段 | ANSI 值 |
|---|---|
| `accent` | `Color::Indexed(5)` magenta |
| `success` | `Color::Indexed(2)` green |
| `error` | `Color::Indexed(1)` red |
| `warning` | `Color::Indexed(3)` yellow |
| `suggestion` | `Color::Indexed(4)` blue |
| `info` | `Color::Indexed(6)` cyan |

限制：ANSI 变体没有把整张 `ThemeColors` 全量改成 ANSI；`diff*`、`syntax*`、`icon*`、`agent*` 等字段仍继承普通 Light/Dark 的 RGB。实际视觉效果因此取决于消费方使用的是语义字段还是专用字段。例如 `prompt` 使用 `suggestion`，会进入 ANSI；`diff_add` 使用 `diffAdd`，仍是 RGB。

## 7. 语法高亮与 diff 颜色

### 7.1 Markdown 代码块

`rendering/markdown.rs` 用 `pulldown-cmark` 解析 Markdown。代码块结束时调用 `highlight_code_block(&code_block_buf, &code_block_lang, theme)`：

| 情况 | 行为 |
|---|---|
| 空代码 | 返回一个 `theme.code` 样式的空 span |
| 无语言标识 | 走 `fallback_highlight()`，整块使用 `theme.code` |
| 未启用 `syntect` feature | 走 `fallback_highlight()`，整块使用 `theme.code` |
| 启用 `syntect` 且语言可识别 | 用 syntect token 颜色生成 ratatui spans |
| 启用 `syntect` 但语言未知/行解析失败 | 回退 `theme.code` |

语言别名覆盖：`javascript/js/node`, `typescript/ts`, `tsx`, `jsx`, `python/py/python3`, `ruby/rb`, `rust/rs`, `go/golang`, `bash/shell/sh/zsh`, `yaml/yml`, `json`, `toml`, `markdown/md`, `html`, `css`, `sql`, `c/h`, `cpp/c++/cc`, `java`, `kotlin/kt`, `swift`, `dart`, `lua`, `php`, `r`, `scala`, `haskell/hs`, `ocaml/ml`, `nim`, `powershell/ps1`, `dockerfile`, `makefile/make`, `graphql/gql`, `protobuf/proto`, `latex/tex`, `xml`。

限制：虽然 `ThemeColors` 和 legacy `Theme` 都有 `syntaxKeyword` / `syntaxString` 等字段，但当前 `syntect` 路径使用固定 syntect 主题 `base16-ocean.dark` 的 token 前景色，再叠加 `theme.code` 作为基础样式；它没有把 syntect scope 映射回 `ThemeColors.syntax*`。这些 `syntax*` 字段当前更像设计系统预留和缓存 fingerprint 的一部分，而不是 syntect token 的直接来源。

### 7.2 Diff

diff 相关样式通过 legacy `Theme` 消费：

| 场景 | 使用样式 | 源码 |
|---|---|---|
| diff header / hunk header | `theme.diff_header` | `ui/diff.rs`, `ui/diff/structured_diff.rs`, `rendering/history_cell.rs` |
| 新增行 | `theme.diff_add` | `ui/diff.rs`, `ui/diff/structured_diff.rs` |
| 删除行 | `theme.diff_remove` | 同上 |
| 上下文行 | `theme.diff_context` | 同上 |
| word-level 新增/删除片段 | `theme.diff_add` / `theme.diff_remove` | `render_word_diff_line`, `render_word_diff_spans` |
| 结构化 diff 的上下文代码 | 先 `theme.diff_context` 标前缀；内容可调用 `highlight_code_block()` | `render_context_line_spans` |

限制：`ThemeColors` 有 `diffAddBg` / `diffRemoveBg`，但当前 inspected diff 渲染主要使用前景色 `diff_add` / `diff_remove`，没有普遍使用增删背景色。

## 8. Agent 颜色

主题系统为 Agent 预留 8 个颜色槽：

| 主题字段 | Agent 颜色名 |
|---|---|
| `agentRed` | `red` |
| `agentOrange` | `orange` |
| `agentYellow` | `yellow` |
| `agentGreen` | `green` |
| `agentCyan` | `cyan` |
| `agentBlue` | `blue` |
| `agentPurple` | `purple` |
| `agentPink` | `pink` |

Agent 编辑器的颜色选择器定义在 `ui/agents/color_picker.rs`，支持：

```text
automatic, red, orange, yellow, green, cyan, blue, purple, pink
```

`automatic` 会返回 `None`，其他值作为 agent 定义的 `color` 字段保存；`agent_file_utils.rs` 会把它写进 agent markdown front matter 的 `color: ...`。Team 子系统还有一套自动分配池，定义在 `allthecodes-teams/src/constants.rs`：

```text
red, blue, green, yellow, purple, orange, pink, cyan
```

Team 颜色通过 `layout_manager::available_colors()` 暴露，并可通过 `ALLTHECODES_AGENT_COLOR` 环境变量传给 teammate。

限制：主题模块可以解析 `agentRed`、`agent-blue` 这类主题 key，但 inspected UI 中 Agent 颜色选择器本身是确定性的文本模型，尚未看到统一的“颜色名 -> `ThemeColors.agent*` -> 终端彩色 swatch/label”生产渲染链路。也就是说，颜色名已经能被保存和分配，主题表也有对应颜色槽，但具体 Agent 列表/树是否使用这些槽要看后续渲染面是否接入。

## 9. 同类可视化工具聚合支持矩阵

| 可视化类型 | 当前支持 | 不支持/限制 |
|---|---|---|
| 消息角色颜色 | 通过 legacy `Theme` 支持 assistant/user/system/tool/error/warning/info/thinking | 角色样式仍集中在 legacy `Theme`，不是所有渲染器直接消费 `ThemeColors` |
| Markdown 排版 | 标题、粗体、斜体、链接、行内代码、代码块、列表、表格均使用 `Theme` | `blockquote`, `hr` 等 `ThemeColors` 字段未在 inspected Markdown 路径中形成完整消费链 |
| 语法高亮 | 有 syntect feature 时支持常见语言别名；无 feature 时优雅退化 | syntect token 色不来自 `ThemeColors.syntax*`；固定 `base16-ocean.dark` 对浅色主题不完全自适应 |
| Diff | 新增、删除、上下文、header、word-level 变化均有样式 | `diffAddBg` / `diffRemoveBg` 使用不足 |
| 进度/状态 | `progress_fill` 用 `success`，`progress_empty` 用 `inactive`；工具活动成功态复用 `diff_add` | icon 专用色和 ANSI 语义色之间不是全量联动 |
| ANSI 兼容 | 主语义色走 `Color::Indexed` | 非主语义字段仍多为 RGB |
| Daltonized | 成功/错误、diff、部分语法色转蓝/橙 | 不是完整调色板替换；源码仍标 TODO |
| Agent 颜色 | 支持 8 色字段、agent front matter、team round-robin 分配 | 缺少统一生产渲染映射；`automatic` 只代表未指定颜色 |

## 10. 维护建议

1. 新增主题时先补 `ThemeName`、`get_theme()` 的数组和 match，再补 `ThemeColors` 完整字段，最后验证 `Theme::from_design_colors()` 是否覆盖需要的 legacy 样式。
2. 若要让 ANSI 真正全局生效，应决定 `diff*`、`syntax*`、`icon*`、`agent*` 是否也切成 `Color::Indexed`。
3. 若要让 Daltonized 达到完整无障碍目标，应替换整张 palette，而不是只覆盖 success/error/diff。
4. 若要让语法高亮完全主题化，需要把 syntect scope 分类映射到 `theme.syntax_keyword`、`theme.syntax_string` 等字段，或生成每套主题对应的 syntect theme。
5. 若要让 Agent 颜色可视化闭环，需要新增颜色名到 `ThemeColors.agent*` 的公共解析函数，并让 Agent 列表、树、footer、team surface 使用同一套映射。
