# Rust TUI 输入交互与 UTF-8 压缩稳定性修复计划

日期：2026-07-18
状态：代码与文档实施完成，自动化分层验证通过；真实桌面 IME 证据待人工验证

实施记录（2026-07-18）：已在 `worktree/tui-mouse-selection-ime-preedit` 完成计划中的代码、测试夹具、配置文案、归档文档和 HTML artifact 收口。实现覆盖默认 native mouse selection、真实 terminal cursor/owner/focus、多行 PromptInput 与显示列布局、用户消息 hanging indent、UTF-8 安全 compact/tool-result preview、query panic 可见失败与单次 `Done`，以及 editor/export 的 mouse/focus 状态恢复。自动化分层验证已全部以 exit 0 完成；真实桌面 IME preedit/候选窗口仍为 `Evidence pending`，PTY 不作为该项证据。

## 1. 目标

- 默认允许用户在 allthecodes Rust TUI 中用终端原生鼠标拖选、复制会话文本。
- 保留鼠标滚轮和滚动条捕获能力，但改为用户显式选择，不再用默认行为牺牲文本选择。
- 将真实终端光标停放在当前文本输入 caret，使中文、日文、韩文等 IME 的预编辑文本和候选窗口出现在正在编辑的输入框位置。
- 统一按终端显示列计算输入窗口和光标位置，覆盖 CJK 宽字符、组合字符、行尾、水平滚动和窄终端。
- 将当前单行 `PromptInput` 补齐为真正的多行输入：Enter 提交、Shift+Enter 插入换行、动态高度、可视行导航，
  并保证硬换行和自动折行的续行首字符与第一行内容处于同一显示列。
- 修复用户消息提交后的自动折行缩进，使续行不再比首行内容向左提前。
- 为主 prompt 和所有实际可编辑 overlay 建立单一“当前光标所有者”契约，避免多个输入面同时声明真实光标。
- 消除 compact/tool-result preview 对 UTF-8 字符串的任意字节切片，确保中文、emoji、组合字符和多 block
  工具结果不会触发 `byte index is not a char boundary` panic。
- 即使查询任务发生意外 panic，也要向 TUI 发送可见错误与完成信号，避免当前 turn 中止后界面永久停留在
  streaming/busy 状态。
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

### 2.5 `PromptInput` 仍是单行组件，多行与续行布局没有实现

`crates/allthecodes/src/ui/prompt_input.rs` 当前明确把 `PromptInput` 定义为 single-line widget。普通 Enter 和
Shift+Enter 都进入提交分支；`render_with_context()` 只在一个 `text_y` 上调用一次 `Buffer::set_line()`；
`app/render.rs` 虽然为输入区域固定保留 3 行高度，但没有按内容增加可见行或建立二维 caret 坐标。

粘贴包含换行的文本时，`input_preview()` 会把所有多行输入折叠成
`[N chars, M lines pasted] ...` 单行摘要。该策略可以用于真正的大粘贴保护，但不能代替两行或少量多行 prompt
的编辑、显示和光标移动。

提交后的用户消息还有一个独立但表现相同的缩进问题：`messages/render/render_user.rs` 给每个显式输入行添加
一个左侧空格，但通用 `messages/wrap.rs::wrap_line_to_width()` 在自动折行时从第 0 列创建续行，没有继承首行
内容区域的左边距，所以视觉结果是第二行比第一行向左提前一列。

上游完整版把 `PromptInput` 配置为 `multiline: true`，并把 mode/prompt indicator 与文本输入放在两个并列
布局节点中；自动折行只发生在文本节点内部，因此所有续行天然从同一文本列开始。Rust TUI 当前实现与该行为
不一致，不能继续按单行精简边界处理。

### 2.6 microcompact 按任意字节位置切片 UTF-8 字符串

`crates/allthecodes-compact/src/microcompact.rs` 的 `tool_result_content_len()` 使用 `String::len()` 返回字节数，
但注释、常量 `SIZE_THRESHOLD_CHARS` 和省略提示都把它称为 characters。`make_tool_result_summary()` 随后直接
执行 `&full_text[..200]` 和 `&full_text[full_text.len() - tail_len..]`。当 200 或尾部起点落在中文“端”等
多字节字符内部时，Rust 会以 `byte index ... is not a char boundary` panic。

Bash 不是直接崩溃源。包含中文的 Bash/tool result 进入消息历史后，在后续模型请求前执行 microcompact；当结果
超过阈值、已不属于最近 10 个工具结果且不在最新 assistant turn 时才进入该分支。任务 JSON 格式本身没有问题。

相同缺陷还存在于 `crates/allthecodes-compact/src/tool_result_budget.rs`：`make_preview()` 和
`truncate_in_place()` 也按任意字节索引截取首尾。本任务必须修复共享缺陷，不能只把 panic 日志指向的第 231 行
改成一个特例。

### 2.7 panic 会绕过 compact fallback 并使当前查询缺少完成信号

`prepare_model_request()` 只在 `deps.microcompact(...).await` 返回 `Err` 时回退原始消息；panic 不会转换成
普通错误。TUI 查询又运行在未被 join 的 `tokio::spawn` 中，panic 会跳过末尾的 `EngineEvent::Done`。结果通常
不是整个进程退出，而是当前查询任务消失、TUI 仍保持 streaming/busy，用户无法判断该 turn 已失败。

根因修复仍应优先消除 panic；查询任务边界的兜底只负责恢复状态并显示错误，不能用 `catch_unwind` 掩盖正常错误。

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

### 3.5 多行输入与续行对齐

- prompt indicator/mode indicator 与可编辑文本使用独立布局区域；第一行和所有续行都从文本区域的 `x` 开始，
  不允许把 `> ` 只拼到第一条 `Line` 后再让后续行回到输入框第 0 列。
- Enter 提交当前 prompt；Shift+Enter 插入 `\n`。粘贴保留规范化后的换行，不能因为包含第二行就强制折叠为
  单行摘要。
- 输入框高度随硬换行和软折行增长，并设置明确的最大可视行数；超过上限时使用垂直 viewport 保持 caret 可见，
  不能无限挤压消息区域。
- Left/Right 继续按合法 UTF-8/grapheme 边界移动；Up/Down 优先在可视行之间保持目标显示列，只有无法继续移动时
  才进入 prompt history；Home/End 的行内/全文语义应写入测试并与上游行为对齐。
- 小型多行输入直接渲染和编辑；真正的大粘贴可以保留紧凑占位/notice，但必须有清晰阈值、可恢复完整内容，且
  caret、提交值和历史记录仍基于原始文本。
- ghost suffix、argument hint、mode indicator 和真实 terminal cursor 必须消费同一多行 layout，不能分别计算
  可见宽度或 caret 坐标。
- 提交后的用户消息使用“外层背景/前缀 + 内层文本矩形”或等价 hanging-indent 契约；显式换行和自动折行的
  首字符列必须一致，CJK 宽字符不能造成额外漂移。

### 3.6 UTF-8 安全的长度与首尾预览

- `*_CHARS`、省略提示和 `original_size` 如果继续声明 characters，就统一使用 Unicode scalar count；仅用于文件
  或内存上限的字节预算必须改名为 `*_BYTES` 并在提示中明确标成 bytes，禁止同一个值跨两种语义复用。
- 提取共享的 UTF-8 安全首尾截断 helper。实现可以基于 `char_indices()` 或经过验证的 boundary floor/ceil，
  但不得直接把任意整数用于 `&text[..n]` 或 `&text[len-n..]`。
- head/tail 预算、实际保留长度、省略数量和 token-freed 估算必须来自同一计数口径，并使用 checked/saturating
  算术；多个 text block 连接时产生的换行也必须计入最终全文长度。
- `microcompact.rs` 和 `tool_result_budget.rs` 复用同一 helper，覆盖保存到磁盘成功和失败后的 in-place
  truncate 两条路径。
- 空字符串、短字符串、恰好命中边界、首部跨 CJK、尾部跨 CJK、emoji/combining、混合 ASCII/CJK、多个
  `ContentBlock::Text` 都必须稳定且输出仍是合法 UTF-8。

### 3.7 查询任务 panic 恢复

- compact pipeline 的可预期失败继续走 `Result`，不得用 panic 表达普通输入问题。
- TUI 启动的查询 future 必须在任务边界捕获意外 unwind 或通过可观察的 `JoinHandle` 处理 panic；记录错误后发送
  用户可见失败事件，并且无论正常、错误还是 panic 都只发送一次 `EngineEvent::Done`。
- 状态恢复必须清除 streaming/busy，保留已输入草稿，并允许下一次提交；不能把 panic 文本伪装成模型回复。
- panic 兜底测试使用受控的 fake engine/dependency 注入，不在产品代码里故意制造 panic，也不能把兜底通过当作
  microcompact 根因已修复的证据。

## 4. 实施阶段

### 阶段 A：在本计划内建立四类失败证据

1. 本计划作为四类问题的统一活跃记录，不另建分散的问题文档；实施 artifact 按问题逐项记录修复证据。
2. 为 UI 与 commands 两份 terminal env 解析增加失败测试：无变量时应关闭 capture，显式 opt-in 应开启，
   disable 应覆盖 enable。
3. 增加 app mouse 测试，固定 capture 开启时当前滚轮/滚动条行为不回退。
4. 增加 PromptInput layout 测试，先暴露 Shift+Enter 无法换行、第二行左移、动态高度缺失、CJK、combining、
   行尾、软折行和 viewport 后 caret 列坐标错误。
5. 增加用户消息 wrap 测试，证明自动折行续行当前丢失首行内容缩进。
6. 用中文字符恰好跨过 head=200 和 tail=100 边界的 fixture 复现 microcompact panic，并为
   `tool_result_budget` 的 preview/in-place 两条路径增加同类失败测试。
7. 增加 app render/TestBackend 测试，证明当前没有声明真实 cursor，并固定目标 owner 优先级。
8. 增加受控查询 panic 测试，证明当前缺少可见失败事件/`Done`，并固定修复后的单次完成契约。

### 阶段 B：收口 mouse capture 配置

1. 以 `allthecodes-commands::terminal_env::TerminalEnvConfig` 作为 canonical 解析入口，评估删除
   `ui/platform/terminal_env.rs` 的重复实现；若依赖边界不允许删除，至少通过共享测试向量保证两者一致。
2. 把默认值改为 native selection，保留表格中的显式兼容行为。
3. `run_tui()` 只在 effective capture=true 时发送 `EnableMouseCapture`，并把同一 bool 交给
   `TerminalGuard` 和正常退出清理。
4. 更新 `/terminal-setup` 文案和测试，使默认输出明确显示
   `disabled (native selection/copy)`，并给出开启滚轮捕获的 opt-in 命令。
5. 审核 export/editor 暂停再进入 alternate screen 的路径，保证只恢复原先启用的 mouse 模式。

### 阶段 C：建立共享多行输入 viewport 与 cursor placement

1. 从 `PromptInput::render_with_context()` 提取无副作用多行 layout helper，返回 hard/soft visual lines、
   viewport、caret visual row/display column、caret cell 和可见性。
2. 把 prompt/mode indicator 与文本内容拆成独立区域；按可见行数动态计算输入高度，并设置最大可视行数。
3. 实现 Enter 提交、Shift+Enter 换行、Up/Down 可视行移动和多行 viewport；保留完整 paste 内容，仅对真正的
   大粘贴使用可恢复的紧凑占位。
4. 使用同一 layout 结果绘制输入并生成物理 cursor 坐标，保证第一行、硬换行和软折行首字符列一致。
5. 修复 CJK/combining/emoji 下的显示列计算；需要时引入直接 `unicode-segmentation` 依赖。
6. 修复用户消息通用 wrap 的 hanging indent，使提交后的自动折行也与首行内容列对齐。
7. 决定假光标样式：真实 cursor active 时不再额外绘制冲突的反色块；terminal focus 丢失时可保留静态
   unfocused caret，但不能让用户看到两个光标。
8. 通过 Ratatui `Frame::set_cursor_position()` 声明 cursor，删除启动阶段永久 `cursor::Hide` 与 frame
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

### 阶段 E：修复 UTF-8 截断与查询 panic 恢复

1. 明确 compact/tool-result budget 的 character 与 byte 计数口径，重命名所有语义不符的常量、字段局部变量和
   用户可见提示。
2. 提取共享的 UTF-8 安全 head/tail helper，并替换 `microcompact.rs`、`tool_result_budget.rs` 的所有任意
   字节切片；同时修正 block join 长度和 omitted/saved 的 checked/saturating 计算。
3. 覆盖 Text/Blocks、CJK/emoji/combining、保存成功/失败、阈值边界和短文本不变的单元测试。
4. 在 TUI 查询任务边界增加意外 panic 可见化和单次 `Done` 兜底，保证 streaming/busy 清理和下一轮可继续。
5. 运行包含中文任务 JSON 的 microcompact 集成 fixture，证明 Bash/tool result 进入历史后不会再终止查询。

### 阶段 F：回归、人工验证与文档收口

1. 在普通终端启动默认配置，验证鼠标拖选和复制；再用
   `ALLTHECODES_ENABLE_MOUSE_CAPTURE=1` 验证滚轮和滚动条。
2. 在报告问题的实际终端/输入法验证：拼音或假名 preedit 出现在 prompt caret，候选窗口跟随 caret，
   选词后最终字符只写入一次。
3. 验证两行/多行输入、硬换行、自动折行、CJK 行、上下移动、最大高度 viewport、提交后消息续行对齐，
   以及空输入、文本中间插入、窄终端、overlay 输入和失焦/回焦。
4. 验证 microcompact 与 tool-result budget 对中文/emoji 数据不 panic，查询异常兜底会恢复 TUI 状态。
5. 在本计划中勾稽最终状态；自动化或平台证据不足的项目明确标注 Evidence pending，不另写模糊“已修复”。
6. 修订当前 terminal 配置说明，清理 allthecodes 文档中的旧 `CLAUDE_CODE_*` 鼠标变量。
7. 创建 `development/worktree-workflow-artifacts/2026-07-18-tui-mouse-selection-ime-preedit.html`，记录目标、
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
| `crates/allthecodes/src/ui/prompt_input.rs` | 真正的多行编辑、hard/soft visual lines、动态高度/viewport、显示列坐标、真实 cursor placement 和 Unicode 测试。 |
| `crates/allthecodes/src/ui/app.rs`、`app/domain.rs` | terminal focus 与 active cursor owner 状态。 |
| `crates/allthecodes/src/ui/app/render.rs` | 动态 prompt 高度、消息区让位、overlay 优先级和 `Frame::set_cursor_position()` 接线。 |
| `crates/allthecodes/src/ui/app/input.rs` | Enter/Shift+Enter、多行 Up/Down/history 边界、mouse 与 focus/input ownership。 |
| `crates/allthecodes/src/ui/messages/render/render_user.rs`、`messages/wrap.rs` | 修复提交后用户消息的自动折行 hanging indent。 |
| `crates/allthecodes/src/ui/components/{search_box,fuzzy_picker,history_search_dialog}.rs` | editable overlay 的 caret 坐标/owner 接口。 |
| `crates/allthecodes/src/ui/permissions/**` | 审核并接入实际自由文本输入面；只读确认面不声明 caret。 |
| `crates/allthecodes/src/ui/tui/engine_events.rs` | 查询 future 意外 panic 的可见错误、单次 `Done` 与状态恢复边界。 |
| `crates/allthecodes-compact/src/microcompact.rs` | 统一 character 语义、UTF-8 安全首尾摘要、Blocks 长度与跨边界回归测试。 |
| `crates/allthecodes-compact/src/tool_result_budget.rs` | preview 和 in-place truncate 复用 UTF-8 安全 helper，修正长度/省略计数。 |
| `crates/allthecodes-compact/src/` 下共享 helper（位置实施时确定） | 集中实现 head/tail UTF-8 安全截断，避免两套边界算法继续漂移。 |
| `crates/allthecodes-engine/src/query/turn_context.rs` 及相关测试依赖 | 审核 microcompact `Result` 回退与 panic 测试注入边界。 |
| `crates/allthecodes/src/ui/app/tests.rs` 及相关组件测试 | 多行/续行、cursor owner、坐标、focus、overlay、panic recovery 和 mouse 回归。 |
| `crates/allthecodes/tests/pty_tui_e2e/**` | 默认/opt-in mouse escape、多行对齐、最终 Unicode 输入、cursor escape 和查询恢复的针对性 PTY 证据。 |
| `development/worktree-workflow-artifacts/2026-07-18-tui-mouse-selection-ime-preedit.html` | 四类问题的统一实施记录和验证证据。 |

具体实施前必须用 `rg` 再审计所有 `cursor::Hide/Show`、`set_cursor_position`、`SearchBox`、自由文本
overlay、mouse capture 消费点，以及 compact/tool preview 中的字符串切片和 `*_CHARS` 长度口径；预计文件列表
不是绕过遗漏审计的固定白名单。

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
- Enter 提交，Shift+Enter 插入换行；粘贴的 CRLF/CR 规范化为 LF 且完整内容仍可提交。
- 两行和多行输入的首字符列一致；软折行与硬换行都从 prompt 文本区域的同一 `x` 开始。
- 输入高度随 visual lines 增长，到达上限后 viewport 滚动且 caret 始终可见。
- Up/Down 保持期望显示列，在首/末可视行才回退到 history；Home/End 行为与上游契约一致。
- 小型多行输入不折叠成 paste 摘要；大型 paste 占位不改变底层完整输入、caret、提交值和 history。
- 极窄/零高区域返回 `None`，不产生越界位置。
- ghost suffix 和 argument hint 不改变真实 caret。
- terminal focus lost、prompt inactive、read-only overlay 时 cursor hidden。
- editable overlay 的 owner 覆盖 prompt，关闭后恢复 prompt。
- 提交后的用户消息在显式换行和自动折行时保持相同内容起始列，包含 CJK 时也不漂移。

### 6.3 compact 与查询恢复单元测试

- ASCII 恰好等于/低于/超过阈值时输出稳定。
- 中文字符分别跨越 microcompact head=200、tail=100，以及 budget head/tail 边界时不 panic。
- 混合 ASCII/CJK、emoji、combining sequence、空文本、短文本和多个 Text blocks 始终生成合法 UTF-8。
- character/byte 口径与常量、字段、omitted 提示一致；Blocks join 换行被计数，所有差值使用安全算术。
- tool-result save 成功走 preview，失败走 in-place truncate，两条路径使用相同边界 helper。
- 包含所报告中文任务 JSON 的旧 Bash/tool result 被 microcompact 后仍可完成下一次模型请求。
- 受控 query panic 产生一次可见错误、一次 `EngineEvent::Done`，并清除 streaming/busy；正常和普通 Err 路径
  不会重复 Done。

### 6.4 PTY 自动化

- 默认启动输出不包含 mouse capture enable 序列或等价模式。
- opt-in 启动仍启用 mouse capture，滚轮测试继续通过。
- focused prompt frame 声明可见 cursor 并把它移动到预期 cell。
- 发送最终 Unicode 字符串后完整保留顺序和内容；Backspace/Left/Right 不破坏 UTF-8 边界。
- 输入两行内容后两行首字符处于同一列；窄终端软折行后续行也不向左提前。
- 打开/关闭 history search 或一个自由文本 overlay 后 cursor owner 正确切换。
- 注入可控查询失败后 prompt 恢复可编辑且下一条输入可以提交。

PTY 不具备真实桌面 IME，不能把这些测试写成“IME 已验证”。

### 6.5 人工终端验证

至少记录：

- 操作系统、终端模拟器及版本；
- 是否经过 tmux/zellij/SSH；
- 输入法名称；
- 默认拖选、复制、opt-in wheel/scrollbar；
- preedit 位置、候选窗口位置、提交结果；
- 两行/多行输入、自动折行、CJK 行和最大高度 viewport 的首字符/光标列；
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
2. `cargo test -p allthecodes-compact`，先验证 UTF-8 边界、Blocks、budget 和 microcompact fixture
3. PromptInput、message wrap、App render/input、query panic recovery 和相关 component focused tests
4. `cargo clippy --workspace --all-targets -- -D warnings`
5. `cargo test --workspace --exclude allthecodes --lib`
6. 针对 mouse/cursor/multiline/Unicode/query recovery 的 PTY test filter，`--test-threads=1`
7. 完成人工终端/IME/多行输入验证
8. `cargo build -p allthecodes --release`
9. `git diff --check`

完整 `cargo test --workspace` 不作为第一轮；只有 targeted 证据无法覆盖集成边界时才进入最终层，并遵守单任务最多
两次的仓库限制。

### 7.1 实际验证结果（2026-07-18）

以下命令均在独立 target
`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-tui-mouse-selection-ime-preedit`
下执行并以 exit 0 结束：

- `cargo fmt --all --check`：通过；最终 `git diff --check`：通过。
- `cargo test -p allthecodes-compact`：58 passed，0 failed；覆盖 UTF-8 head/tail、Blocks join、budget 和 microcompact。
- TUI 定向测试：PromptInput 20 passed、App 98 passed、message wrap 4 passed、query engine events 2 passed、
  terminal env 16 passed、terminal setup 14 passed。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过，无新增 warning。
- `cargo test --workspace --exclude allthecodes --lib`：exit 0；非-PTY workspace library tests 全部通过，既有 ignored
  测试保持 ignored。
- PTY：`tests::commands_surface` 39 passed（`--test-threads=1`），覆盖 overlay/picker/permission surface 与 prompt
  恢复；`tests::commands_core_info::terminal_setup_command` 和 `welcome::shows_prompt_on_startup` 各 1 passed。
- `cargo build -p allthecodes --release`：通过，exit 0（5 分 08 秒）。

定向单元测试证明了真实 cursor 的 frame 坐标、CJK/combining/grapheme 显示列、多行 hard/soft wrap、focus/owner
优先级和 UTF-8 截断边界。现有 PTY 套件没有真实桌面 IME composition 事件，也没有把 OS preedit 当作可自动化输入；
默认/opt-in mouse 的配置矩阵由 commands 单测和 terminal-setup PTY 覆盖，但真实终端拖选、滚轮和 IME 候选窗仍需人工记录。

## 8. 提交与完成标准

计划文件已在主分支前置提交。实现已在 worktree 内完成并形成以下提交边界：

1. `aeb9b984 fix(tui): complete input and UTF-8 stability fixes`：代码、测试、依赖和归档说明。
2. 当前文档收口提交：本计划与 HTML artifact 的实际验证结果、边界和 commit 记录。

完成必须同时满足：

- 默认配置可原生拖选，显式 opt-in 的滚轮/滚动条仍工作。
- 真实 cursor 跟随当前 editable caret，overlay/focus 切换无双光标或错误锚点。
- CJK 等宽字符之后的 cursor cell 正确。
- PromptInput 支持 Enter 提交、Shift+Enter 换行、Up/Down 可视行移动和有上限的动态高度；第一行、硬换行、
  软折行及提交后的用户消息续行内容首字符列一致。
- microcompact 和 tool-result budget 不再对 UTF-8 使用任意字节切片；所报告中文任务 JSON 及边界 fixture
  均不 panic，长度与 omitted 提示口径一致。
- 查询 future 的普通错误和意外 panic 都能结束当前 streaming/busy 状态；意外 panic 有可见错误且 `Done`
  恰好发送一次，下一次提交可继续。
- 报告环境中的真实 IME preedit 和候选窗口有人工证据。
- 自动化只声明它实际证明的 cursor/mouse/multiline/final-text/UTF-8/query recovery 行为。
- 无新增 warning，分层检查通过；任何平台证据缺口被明确记录。
- HTML artifact 完整，worktree commit fast-forward 合并回 `allthecodes` 后删除 worktree 和分支。
