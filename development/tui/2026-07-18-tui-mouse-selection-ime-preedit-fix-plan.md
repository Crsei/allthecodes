# Rust TUI 鼠标拖选与 IME 预编辑修复计划

日期：2026-07-18
状态：待实施

## 1. 目标

- 默认允许用户在 allthecodes Rust TUI 中用终端原生鼠标拖选、复制会话文本。
- 保留鼠标滚轮和滚动条捕获能力，但改为用户显式选择，不再用默认行为牺牲文本选择。
- 将真实终端光标停放在当前文本输入 caret，使中文、日文、韩文等 IME 的预编辑文本和候选窗口出现在正在编辑的输入框位置。
- 统一按终端显示列计算输入窗口和光标位置，覆盖 CJK 宽字符、组合字符、行尾、水平滚动和窄终端。
- 为主 prompt 和所有实际可编辑 overlay 建立单一“当前光标所有者”契约，避免多个输入面同时声明真实光标。
- 保留已提交 Unicode 文本、粘贴、快捷键、ghost completion、命令面板、滚动和终端清理行为。

## 2. 已确认问题

### 2.1 默认 mouse capture 阻止终端原生拖选

`crates/allthecodes/src/ui/tui.rs::run_tui()` 当前在没有环境变量覆盖时执行
`EnableMouseCapture`。终端随后把鼠标按下、拖动、释放和滚轮作为应用事件发送，不再自行建立文本选择。

`App::handle_mouse_event()` 只对 session 滚动条拖动做处理；其它 `MouseEventKind::Drag`
直接返回 `AppAction::None`。所以当前行为既不是终端原生选择，也不是应用内选择，而是普通拖动被捕获后丢弃。

当前存在两个逃生变量：

- `ALLTHECODES_ENABLE_MOUSE_CAPTURE=0`
- `ALLTHECODES_DISABLE_MOUSE=1`

但 `TerminalEnvConfig::default()` 把 `disable_mouse` 设为 `false`，因此默认仍然捕获鼠标。历史文档曾把
“mouse capture 默认关闭”列为已修复，后续为直接滚轮滚动又改成默认开启；这应作为用户可见回归重新记录，
不能仅以“存在环境变量绕过”标记为已解决。

### 2.2 IME 预编辑态没有可用的物理光标锚点

TUI 启动时执行 `cursor::Hide`，正常运行期间不再显示或定位真实终端光标。`PromptInput` 当前通过给
caret 位置的字符绘制白色背景来模拟光标，但这个反色单元格对终端模拟器、IME 和辅助功能不可见。

crossterm 0.28 的公共 `Event` 只有 key、mouse、paste、resize 和 focus 等事件，没有浏览器式
`compositionstart` / `compositionupdate` / `compositionend` 或通用 preedit 事件。应用不能靠
`KeyCode::Char` 自己重建尚未提交的拼音、假名等内容；正确边界是让终端模拟器在真实物理光标处绘制
IME preedit，应用继续只接收提交后的 Unicode 输入。

上游 `claude-code-bun/src/components/BaseTextInput.tsx` 已通过 `useDeclaredCursor` 把物理光标停放到
输入 caret，并明确以 CJK IME preedit 和辅助功能作为原因。Rust TUI 当前缺少对应契约。

### 2.3 当前输入窗口算法不是显示列算法

`PromptInput::render_with_context()` 当前混用：

- UTF-8 byte offset：`cursor_position`；
- Unicode scalar count：`chars().count()`；
- `String::len()` byte count；
- 终端可视宽度：`area.width`。

该算法对 ASCII 基本成立，但 CJK 字符通常占两列，组合字符可能占零列，emoji 也可能由多个 scalar
组成。即使显示真实光标，如果继续按字符数计算，光标和 IME 候选窗口仍会在宽字符之后错位。

### 2.4 terminal focus 和输入所有权尚未进入主渲染状态

事件循环当前忽略 `Event::FocusGained` / `Event::FocusLost`。主 prompt、history search、picker、
permission/question free-text 等输入面也没有统一声明“哪个输入现在拥有物理光标”。如果直接在主 prompt
调用 `frame.set_cursor_position()`，打开 overlay 后仍可能把 IME 锚点留在被遮挡的 prompt 上。

## 3. 目标行为契约

### 3.1 鼠标模式

| 配置 | mouse capture | 拖选/复制 | TUI 滚轮与滚动条 |
| --- | --- | --- | --- |
| 无相关环境变量 | 关闭 | 终端原生可用 | 不接收鼠标事件；使用 PageUp/PageDown、方向键等 |
| `ALLTHECODES_ENABLE_MOUSE_CAPTURE=1` | 开启 | 依赖终端的 Shift+拖选能力，不作为保证 | 可用 |
| `ALLTHECODES_ENABLE_MOUSE_CAPTURE=0` | 关闭 | 终端原生可用 | 不可用 |
| `ALLTHECODES_DISABLE_MOUSE=1` | 关闭且优先级最高 | 终端原生可用 | 不可用 |
| 仅 `ALLTHECODES_DISABLE_MOUSE=0` | 保留旧兼容：开启 | 不保证 | 可用，并在诊断输出中标为 legacy |

实现必须满足：

1. 无变量默认值改为 native selection，而不是依赖某个终端恰好支持 Shift+拖选。
2. `ALLTHECODES_DISABLE_MOUSE=1` 继续覆盖冲突的 enable 配置。
3. 捕获开启时保留当前 wheel、mouse focus 和 session scrollbar 行为。
4. 捕获关闭时不伪装成应用内选择，不新增半成品 selection model；复制由终端负责。
5. `/terminal-setup` 显示 effective 模式、对应取舍和准确的 `ALLTHECODES_*` 变量名。
6. archived 文档中遗留的 `CLAUDE_CODE_DISABLE_MOUSE` 名称不得继续作为当前 allthecodes 指引。

应用内跨消息语义选择、点击链接、双击选词和同时兼得原生拖选/应用滚轮不属于本次第一阶段；如果未来
需要这些能力，应单独设计 screen-cell 到消息文本的映射，而不是继续吞掉普通 drag。

### 3.2 物理终端光标

建立一个可测试的 `TerminalCursorPlacement`（名称可在实现时调整），至少包含：

```text
TerminalCursorPlacement {
  position: Option<ratatui::layout::Position>,
  owner: Prompt | HistorySearch | Picker | PermissionInput | QuestionInput | ...,
}
```

契约如下：

1. 每帧最多一个 active owner。
2. owner 必须是当前真正接收字符输入的 surface。
3. terminal focus 存在、surface focused、caret 可见且坐标位于 frame 内时，调用
   `Frame::set_cursor_position()`；Ratatui 会显示并定位真实光标。
4. terminal focus 丢失、只读 surface、workspace trust/selection/transcript 模式或没有有效输入 owner 时，
   本帧不声明 cursor，让 Ratatui 隐藏它。
5. overlay 优先于底层 prompt；关闭 overlay 后下一帧把 owner 恢复到 prompt。
6. export/editor 切换、panic/正常退出继续由 `TerminalGuard` 恢复 cursor 和 alternate screen。
7. 不在事件循环外额外发送一套 `cursor::Show/Hide`，避免与 Ratatui frame cursor API 互相争用。

### 3.3 输入布局和 Unicode

- `cursor_position` 可以继续保存 UTF-8 byte offset，编辑操作仍保证 char boundary。
- 提取共享的输入 viewport/layout 计算结果，渲染和物理光标必须消费同一个结果，不能各算一遍。
- caret 的 x 坐标必须使用 `unicode-width` 的显示列宽，加上 `> ` 前缀和实际 viewport 起点。
- viewport 应按显示列裁剪，不能把两列字符的一半放到边界外。
- 对 combining sequence/emoji cluster，先审核是否需要把 workspace 的 `unicode-segmentation` 提升为直接依赖；
  如果只用 `unicode-width` 无法保持光标移动与渲染一致，则按 grapheme cluster 移动和裁剪。
- 坐标使用 `saturating_add` 和 frame bounds 检查；零宽、零高、极窄输入区域不声明 cursor。
- 空输入、行尾、文本中间、CJK 后、水平滚动后和 ghost suffix 前都必须得到确定坐标。

### 3.4 IME 输入边界

- 不在 crossterm 事件层发明无法可靠获得的 composition 状态。
- 尚未提交的 preedit 由终端/操作系统绘制，不能写入 `PromptInput.input`、history、completion 或请求。
- 输入法提交后的字符继续通过 `KeyCode::Char`/paste 路径写入；一次提交产生多个事件时不得丢字符或只留最后一个。
- composition 期间的 Enter/Space 由终端输入法优先处理；只有终端最终发送给应用的 Enter 才触发提交。
- PTY 自动化只能证明 cursor escape、最终 Unicode 文本和回归安全，不能声称证明真实 OS IME preedit；
  preedit 必须有人工终端证据。

## 4. 实施阶段

### 阶段 A：建立失败证据和用户可见问题记录

1. 在 `development/archive/KNOWN_ISSUES.md` 新增独立条目，分别记录默认 mouse capture 回归和 IME
   preedit/caret 锚点缺失；实现完成前保持 Open。
2. 为 UI 与 commands 两份 terminal env 解析增加失败测试：无变量时应关闭 capture，显式 opt-in 应开启，
   disable 应覆盖 enable。
3. 增加 app mouse 测试，固定 capture 开启时当前滚轮/滚动条行为不回退。
4. 增加 PromptInput layout 测试，先暴露 CJK、combining、行尾和水平滚动后的列坐标错误。
5. 增加 app render/TestBackend 测试，证明当前没有声明真实 cursor，并固定目标 owner 优先级。

### 阶段 B：收口 mouse capture 配置

1. 以 `allthecodes-commands::terminal_env::TerminalEnvConfig` 作为 canonical 解析入口，评估删除
   `ui/platform/terminal_env.rs` 的重复实现；若依赖边界不允许删除，至少通过共享测试向量保证两者一致。
2. 把默认值改为 native selection，保留表格中的显式兼容行为。
3. `run_tui()` 只在 effective capture=true 时发送 `EnableMouseCapture`，并把同一 bool 交给
   `TerminalGuard` 和正常退出清理。
4. 更新 `/terminal-setup` 文案和测试，使默认输出明确显示
   `disabled (native selection/copy)`，并给出开启滚轮捕获的 opt-in 命令。
5. 审核 export/editor 暂停再进入 alternate screen 的路径，保证只恢复原先启用的 mouse 模式。

### 阶段 C：建立共享输入 viewport 与 cursor placement

1. 从 `PromptInput::render_with_context()` 提取无副作用 layout helper，返回 visible text、viewport、
   caret display column、caret cell 和可见性。
2. 使用同一 layout 结果绘制输入并生成物理 cursor 坐标。
3. 修复 CJK/combining/emoji 下的显示列计算；需要时引入直接 `unicode-segmentation` 依赖。
4. 决定假光标样式：真实 cursor active 时不再额外绘制冲突的反色块；terminal focus 丢失时可保留静态
   unfocused caret，但不能让用户看到两个光标。
5. 通过 Ratatui `Frame::set_cursor_position()` 声明 cursor，删除启动阶段永久 `cursor::Hide` 与 frame
   API 冲突的假设；退出恢复仍保留。

### 阶段 D：接入 focus 与全部可编辑 surface

1. 在 App/runtime 状态记录 terminal focus，处理 `FocusGained` / `FocusLost` 并触发 redraw。
2. 主 prompt 在没有更高优先级 editable overlay 时拥有 cursor。
3. 审核并接入 `HistorySearchDialog`、`SearchBox`/`FuzzyPicker`、permission feedback、question free-text、
   config/agent/plugin 等实际接收自由文本的 surface。
4. 只读 command surface、消息 selection、transcript 浏览和 confirmation button 不得声明文本 caret。
5. 为 overlay 打开、嵌套优先级、关闭恢复、terminal focus 切换增加 app render 测试。
6. 如果 proactive terminal-focus payload 继续使用硬编码 `true`，在不扩大行为风险的前提下改用同一状态并补测试；
   若无法在本任务内安全收口，记录为相邻问题而不静默改变。

### 阶段 E：回归、人工验证与文档收口

1. 在普通终端启动默认配置，验证鼠标拖选和复制；再用
   `ALLTHECODES_ENABLE_MOUSE_CAPTURE=1` 验证滚轮和滚动条。
2. 在报告问题的实际终端/输入法验证：拼音或假名 preedit 出现在 prompt caret，候选窗口跟随 caret，
   选词后最终字符只写入一次。
3. 验证空输入、已有 ASCII、已有中文、文本中间插入、长输入横向滚动、窄终端、overlay 输入和失焦/回焦。
4. 将 known issue 条目改为 Fixed，只记录实际获得的自动化和人工证据；未覆盖平台标注 Evidence pending。
5. 修订当前 terminal 配置说明，清理 allthecodes 文档中的旧 `CLAUDE_CODE_*` 鼠标变量。
6. 创建 `development/worktree-workflow-artifacts/2026-07-18-tui-mouse-selection-ime-preedit.html`，记录目标、
   改动、commit、自动化结果、测试终端/IME 和未验证平台。

## 5. 预计修改文件

| 路径 | 计划改动 |
| --- | --- |
| `crates/allthecodes/src/ui/tui.rs` | mouse capture 默认/清理接线，focus 事件分发，移除与 frame cursor 冲突的永久隐藏策略。 |
| `crates/allthecodes/src/ui/tui/terminal_guard.rs` | 按实际启用状态恢复 mouse、cursor 和 alternate screen。 |
| `crates/allthecodes/src/ui/tui/export.rs` | 暂停/恢复 TUI 时保持 mouse/cursor 模式一致。 |
| `crates/allthecodes/src/ui/platform/terminal_env.rs` | 删除重复解析或同步新的默认值与兼容矩阵。 |
| `crates/allthecodes-commands/src/terminal_env.rs` | canonical mouse 配置默认、优先级与测试。 |
| `crates/allthecodes-commands/src/terminal_setup.rs` | 展示 effective 模式、取舍和准确环境变量。 |
| `crates/allthecodes/src/ui/prompt_input.rs` | 共享 viewport/layout、显示列坐标、真实 cursor placement 和 Unicode 测试。 |
| `crates/allthecodes/src/ui/app.rs`、`app/domain.rs` | terminal focus 与 active cursor owner 状态。 |
| `crates/allthecodes/src/ui/app/render.rs` | overlay 优先级和 `Frame::set_cursor_position()` 接线。 |
| `crates/allthecodes/src/ui/app/input.rs` | 保持 mouse 行为并处理需要的 focus/input ownership 变化。 |
| `crates/allthecodes/src/ui/components/{search_box,fuzzy_picker,history_search_dialog}.rs` | editable overlay 的 caret 坐标/owner 接口。 |
| `crates/allthecodes/src/ui/permissions/**` | 审核并接入实际自由文本输入面；只读确认面不声明 caret。 |
| `crates/allthecodes/src/ui/app/tests.rs` 及相关组件测试 | cursor owner、坐标、focus、overlay 和 mouse 回归。 |
| `crates/allthecodes/tests/pty_tui_e2e/**` | 默认/opt-in mouse escape、最终 Unicode 输入和 cursor escape 的针对性 PTY 证据。 |
| `development/archive/KNOWN_ISSUES.md` | 分别记录并收口两个用户可见问题。 |
| `development/worktree-workflow-artifacts/2026-07-18-tui-mouse-selection-ime-preedit.html` | 实施记录和验证证据。 |

具体实施前必须用 `rg` 再审计所有 `cursor::Hide/Show`、`set_cursor_position`、`SearchBox`、自由文本
overlay 和 mouse capture 消费点；预计文件列表不是绕过遗漏审计的固定白名单。

## 6. 测试矩阵

### 6.1 配置与 mouse 单元测试

- 无变量：capture=false。
- enable=1：capture=true；enable=0：capture=false。
- disable=1 + enable=1：capture=false。
- 单独 disable=0：按 legacy 兼容为 true，并在 terminal setup 输出中标注。
- capture=true 时 wheel、scrollbar click/drag 和 overlay scroll isolation 保持现状。
- capture=false 时启动和退出均不发送 enable/disable mouse 的不对称序列。

### 6.2 输入布局与 cursor 单元测试

- 空输入、ASCII 行尾、ASCII 中间插入。
- `你`、`中文abc`、`abc中文` 前后移动的显示列坐标。
- combining mark、emoji/ZWJ（按最终选定的 grapheme 支持边界断言）。
- 长输入水平滚动后 caret 始终位于输入区域内。
- 极窄/零高区域返回 `None`，不产生越界位置。
- ghost suffix 和 argument hint 不改变真实 caret。
- terminal focus lost、prompt inactive、read-only overlay 时 cursor hidden。
- editable overlay 的 owner 覆盖 prompt，关闭后恢复 prompt。

### 6.3 PTY 自动化

- 默认启动输出不包含 mouse capture enable 序列或等价模式。
- opt-in 启动仍启用 mouse capture，滚轮测试继续通过。
- focused prompt frame 声明可见 cursor 并把它移动到预期 cell。
- 发送最终 Unicode 字符串后完整保留顺序和内容；Backspace/Left/Right 不破坏 UTF-8 边界。
- 打开/关闭 history search 或一个自由文本 overlay 后 cursor owner 正确切换。

PTY 不具备真实桌面 IME，不能把这些测试写成“IME 已验证”。

### 6.4 人工终端验证

至少记录：

- 操作系统、终端模拟器及版本；
- 是否经过 tmux/zellij/SSH；
- 输入法名称；
- 默认拖选、复制、opt-in wheel/scrollbar；
- preedit 位置、候选窗口位置、提交结果；
- focus lost/gained 和 overlay 输入结果。

优先验证报告问题的环境；若 Linux、macOS、Windows 无法全部获得，未验证平台必须明确标注，不做全平台声明。

## 7. 分层验证顺序

实现使用独立 worktree：

```bash
git worktree add -b worktree/tui-mouse-selection-ime-preedit \
  .worktrees/tui-mouse-selection-ime-preedit allthecodes
```

并设置独立 target：

```bash
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-tui-mouse-selection-ime-preedit
```

按仓库 SOP 顺序运行：

1. `cargo fmt --all --check`
2. terminal env、PromptInput、App render/input 和相关 component focused tests
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace --exclude allthecodes --lib`
5. 针对 mouse/cursor/Unicode 的 PTY test filter，`--test-threads=1`
6. 完成人工终端/IME 验证
7. `cargo build -p allthecodes --release`
8. `git diff --check`

完整 `cargo test --workspace` 不作为第一轮；只有 targeted 证据无法覆盖集成边界时才进入最终层，并遵守单任务最多
两次的仓库限制。

## 8. 提交与完成标准

计划文件按主分支前置流程单独提交。后续实现全部在 worktree 中完成，建议提交边界：

1. `fix(tui): restore native selection and IME cursor placement`
2. `docs(tui): record mouse selection and IME verification`
3. `docs(workflow): record TUI input compatibility fix`

完成必须同时满足：

- 默认配置可原生拖选，显式 opt-in 的滚轮/滚动条仍工作。
- 真实 cursor 跟随当前 editable caret，overlay/focus 切换无双光标或错误锚点。
- CJK 等宽字符之后的 cursor cell 正确。
- 报告环境中的真实 IME preedit 和候选窗口有人工证据。
- 自动化只声明它实际证明的 cursor/mouse/final-text 行为。
- 无新增 warning，分层检查通过；任何平台证据缺口被明确记录。
- HTML artifact 完整，worktree commit fast-forward 合并回 `allthecodes` 后删除 worktree 和分支。
