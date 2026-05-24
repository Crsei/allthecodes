# ink-terminal vs OpenTUI: cc-rust 项目迁移差异文档

> 记录 cc-rust 前端从 ink-terminal 迁移至 OpenTUI 过程中的所有 API 差异、兼容层实现、降级项和回退方案。

## 1. 架构差异

### 1.1 依赖与初始化

| | ink-terminal | OpenTUI |
|--|---|---|
| **npm 包** | `ink-terminal` (workspace submodule) | `@opentui/core` + `@opentui/react` |
| **底层引擎** | 纯 JS/TS, React reconciler | Zig 原生核心 + C ABI + JS 绑定 |
| **运行时** | Bun | Bun (Node/Deno 在路线图中) |
| **渲染入口** | `render(<App />)` → 返回 `{ unmount }` | `createCliRenderer(opts)` → `createRoot(renderer).render(<App />)` |
| **销毁** | `instance.unmount()` | `renderer.destroy()` |
| **Alternate Screen** | 显式 `<AlternateScreen>` 包裹 | 默认启用，无需包裹 |
| **鼠标支持** | `<AlternateScreen mouseTracking>` | `createCliRenderer({ useMouse: true })` |
| **Ctrl+C** | 手动处理 (`useInput`) | 可通过 `exitOnCtrlC: true` 自动处理 |

```tsx
// ─── ink-terminal ───
import { render } from 'ink-terminal'
const instance = await render(<App />)
instance.unmount()

// ─── OpenTUI ───
import { createCliRenderer } from '@opentui/core'
import { createRoot } from '@opentui/react'
const renderer = await createCliRenderer({ exitOnCtrlC: false, useMouse: true })
createRoot(renderer).render(<App />)
renderer.destroy()
```

### 1.2 JSX 模型

| | ink-terminal | OpenTUI React |
|--|---|---|
| **元素命名** | 大写 React 组件: `<Box>`, `<Text>` | 小写 JSX intrinsics: `<box>`, `<text>` |
| **JSX 工厂** | 标准 `react-jsx` | 自定义 `jsxImportSource: "@opentui/react"` |
| **tsconfig** | `"jsx": "react-jsx"` | `"jsx": "react-jsx"` + `"jsxImportSource": "@opentui/react"` |

本项目通过兼容层 (`compat/ink-compat.tsx`) 保持大写组件名，内部映射到小写 intrinsics。

---

## 2. 组件差异

### 2.1 Text

这是差异最大的组件。ink-terminal 通过 **props** 设置文本样式，OpenTUI 通过 **嵌套修饰符元素** 设置。

| 特性 | ink-terminal | OpenTUI React |
|------|---|---|
| **加粗** | `<Text bold>` | `<text><strong>...</strong></text>` |
| **斜体** | `<Text italic>` | `<text><em>...</em></text>` |
| **暗淡** | `<Text dim>` | 无直接等价 (见下方兼容方案) |
| **反色** | `<Text inverse>` | 无直接等价 (见下方兼容方案) |
| **下划线** | `<Text underline>` | `<text><u>...</u></text>` |
| **删除线** | `<Text strikethrough>` | 无 JSX 修饰符 |
| **颜色** | `<Text color="ansi:magenta">` | `<text fg="#CC00CC">` 或 `<span fg="...">` |
| **背景色** | `<Text backgroundColor="red">` | `<text bg="#CC0000">` |
| **换行** | `<Text wrap="wrap">` | 无直接等价 (OpenTUI 自动处理) |
| **嵌套** | `<Text><Text bold>inner</Text></Text>` | `<text><strong>inner</strong></text>` |

**可用修饰符元素** (必须在 `<text>` 内部):
- `<span fg="..." bg="...">` — 行内着色
- `<strong>` / `<b>` — 加粗
- `<em>` / `<i>` — 斜体
- `<u>` — 下划线
- `<br />` — 换行
- `<a href="...">` — 链接

```tsx
// ─── ink-terminal ───
<Text bold color="ansi:magenta">cc-rust</Text>
<Text dim>some hint</Text>
<Text inverse> selected </Text>
<Text>
  <Text dim>Label: </Text>
  <Text bold>{value}</Text>
</Text>

// ─── OpenTUI (原生) ───
<text><strong><span fg="#CC00CC">cc-rust</span></strong></text>
<text fg="#888888">some hint</text>
<text fg="#000000" bg="#CCCCCC"> selected </text>
<text>
  <span fg="#888888">Label: </span>
  <strong>{value}</strong>
</text>
```

**兼容层方案:**
- `dim` → 当无显式 color 时，映射 `fg="#888888"` (灰色近似)
- `inverse` → 交换 `fg`/`bg` (`fg="#000000"`, `bg` 取原 fg 值或 `#CCCCCC`)
- `bold` → 包裹 `<strong>`
- `italic` → 包裹 `<em>`
- 嵌套 `<Text>` → 顶层渲染 `<text>`，子级渲染 `<span>` (通过 React Context 检测嵌套层级)

### 2.2 Box

大部分 flexbox 布局属性相同，差异集中在边框和事件。

| 特性 | ink-terminal | OpenTUI React |
|------|---|---|
| **圆角边框** | `borderStyle="round"` | `borderStyle="rounded"` |
| **单线边框** | `borderStyle="single"` | `borderStyle="single"` (相同) |
| **双线边框** | `borderStyle="double"` | `borderStyle="double"` (相同) |
| **粗线边框** | `borderStyle="bold"` | `borderStyle="bold"` (相同) |
| **暗淡边框色** | `borderDimColor` (boolean) | 无直接等价 |
| **边框标题** | 无 | `title="..." titleAlignment="left"` |
| **点击事件** | `onClick={(event) => ...}` | `onMouseDown={(event) => ...}` |
| **鼠标移动** | 无 | `onMouseMove`, `onMouseUp` |
| **焦点控制** | 无 | `focusable`, `focused` |

```tsx
// ─── ink-terminal ───
<Box borderStyle="round" borderDimColor onClick={handleClick}>

// ─── OpenTUI (原生) ───
<box borderStyle="rounded" borderColor="#666666" onMouseDown={handleClick}>
```

**共同属性** (API 一致):
`flexDirection`, `flexGrow`, `alignItems`, `justifyContent`, `gap`,
`width`, `height`, `minWidth`, `maxWidth`,
`padding`, `paddingX`, `paddingY`, `paddingLeft`, `paddingRight`, `paddingTop`, `paddingBottom`,
`margin`, `marginTop`, `marginBottom`, `marginLeft`, `marginRight`,
`borderColor`, `borderTop`, `borderBottom`, `borderLeft`, `borderRight`,
`position`, `top`, `bottom`, `left`, `right`

### 2.3 Spacer

| ink-terminal | OpenTUI React |
|---|---|
| `<Spacer />` (专用组件) | 无专用组件，使用 `<box flexGrow={1} />` |

### 2.4 AlternateScreen

| ink-terminal | OpenTUI React |
|---|---|
| `<AlternateScreen mouseTracking>` 显式包裹 | 默认使用 alternate screen，无需包裹 |

兼容层中 `AlternateScreen` 为空 passthrough，忽略 `mouseTracking` prop。

### 2.5 ScrollBox

| 特性 | ink-terminal | OpenTUI React |
|------|---|---|
| **组件名** | `<ScrollBox>` | `<scrollbox>` |
| **粘性滚动** | `stickyScroll` prop | 无直接等价 (兼容层通过 `useEffect` 模拟) |
| **滚动条自定义** | 有限 | `scrollbarOptions` 支持颜色、箭头 |
| **ref handle 方法** | `getScrollTop()`, `getPendingDelta()`, `getViewportHeight()`, `getFreshScrollHeight()`, `scrollBy()`, `scrollTo()`, `scrollToBottom()` | `scrollTo()`, `scrollBy()`, `scrollToBottom()`, `scrollChildIntoView()` |

**兼容层:** 通过 `useImperativeHandle` 适配 handle 接口，`getViewportHeight()` 和 `getFreshScrollHeight()` 退化为 `process.stdout.rows` 近似值。

### 2.6 VirtualList

| ink-terminal | OpenTUI React |
|---|---|
| `<VirtualList items={...} renderItem={...} />` — 虚拟化渲染 | **无等价组件** |

**兼容层:** 降级为全量渲染 (遍历所有 items，无虚拟化)。对中等消息量可接受，长对话可能有性能影响。

### 2.7 Markdown

| ink-terminal | OpenTUI React |
|---|---|
| `import { Markdown } from 'ink-terminal/markdown'` — 完整 Markdown 渲染 | Core 有 `MarkdownRenderable`，但 React API 未暴露 `<markdown>` intrinsic |

**兼容层:** 降级为纯 `<text>{children}</text>` (无格式化)。

**改进方向:** OpenTUI core 导出了 `MarkdownRenderable`，后续可通过 `extend()` 注册为 React 自定义组件。

---

## 3. Hooks 差异

### 3.1 useInput → useKeyboard

这是行为差异最大的 hook。

| | ink-terminal `useInput` | OpenTUI `useKeyboard` |
|--|---|---|
| **回调签名** | `(input: string, key: KeyInfo, event: InputEvent) => void` | `(event: KeyEvent) => void` |
| **字符输入** | 第一参数 `input` 是可打印字符 | `event.sequence` 或 `event.name` |
| **特殊键** | `key.upArrow`, `key.return`, `key.backspace` 等 boolean | `event.name === 'up'`, `'return'`, `'backspace'` |
| **修饰符** | `key.ctrl`, `key.meta`, `key.shift` | `event.ctrl`, `event.meta`, `event.shift`, `event.option` |
| **鼠标滚轮** | `key.wheelUp`, `key.wheelDown` | `event.name === 'wheel_up'` / `'wheel_down'` (待确认) |
| **事件冒泡控制** | `event.stopImmediatePropagation()` | 无直接等价 |
| **按键释放** | 不触发 | `event.eventType === 'release'` 时触发 |
| **重复按键** | 不区分 | `event.eventType === 'repeat'`, `event.repeated` |
| **活跃控制** | `useInput(handler, { isActive })` | 无内置选项 |

```tsx
// ─── ink-terminal ───
useInput((input, key, event) => {
  if (key.ctrl && input === 'c') abort()
  if (key.return) submit()
  if (key.upArrow) navigateUp()
  if (input === 'y') confirm()
}, { isActive: focused })

// ─── OpenTUI (原生) ───
useKeyboard((e: KeyEvent) => {
  if (e.eventType === 'release') return
  if (e.ctrl && e.name === 'c') abort()
  if (e.name === 'return' || e.name === 'enter') submit()
  if (e.name === 'up') navigateUp()
  if (e.sequence === 'y') confirm()
})
```

**兼容层:** `useInput` 包裹 `useKeyboard`，内部将 `KeyEvent` 转换为 `(input, key, event)` 三参数格式。`stopImmediatePropagation()` 为 no-op。

**KeyEvent.name 映射表:**

| ink-terminal `key.*` | OpenTUI `event.name` |
|---|---|
| `upArrow` | `'up'` |
| `downArrow` | `'down'` |
| `leftArrow` | `'left'` |
| `rightArrow` | `'right'` |
| `return` | `'return'` 或 `'enter'` |
| `escape` | `'escape'` |
| `tab` | `'tab'` |
| `backspace` | `'backspace'` |
| `delete` | `'delete'` |
| `pageUp` | `'pageup'` 或 `'page_up'` |
| `pageDown` | `'pagedown'` 或 `'page_down'` |
| `home` | `'home'` |
| `end` | `'end'` |

### 3.2 useApp → useRenderer

| | ink-terminal `useApp` | OpenTUI `useRenderer` |
|--|---|---|
| **退出** | `useApp().exit()` | `useRenderer().destroy()` |
| **终端尺寸** | 无 | `renderer.width`, `renderer.height` |
| **主题模式** | 无 | `renderer.themeMode` (`"dark"` / `"light"` / `null`) |
| **调试控制台** | 无 | `renderer.displayDebugConsole()` |

### 3.3 useAnimationFrame → useTimeline

| | ink-terminal `useAnimationFrame` | OpenTUI `useTimeline` |
|--|---|---|
| **返回值** | `[ref, elapsedMs]` (ref 绑定到 Box 启用 tick) | `{ add, play, pause, restart }` |
| **用途** | 定时 tick (Spinner 动画、计时器) | CSS-like 关键帧动画 |
| **暂停** | 传 `null` 暂停 | `timeline.pause()` |

**兼容层:** 使用 `setInterval` + `useState` 模拟��返回 `[ref, elapsedMs]` 形式。ref 为空 (OpenTUI 不需要)。

### 3.4 OpenTUI 独有 Hooks

| Hook | 用途 | ink-terminal 等价 |
|------|------|---|
| `useOnResize(callback)` | 终端尺寸变化回调 | 无 (需手动 `process.stdout.on('resize')`) |
| `useTerminalDimensions()` | 响应式 `{ width, height }` | 无 |
| `useTimeline(options)` | 关键帧动画 | 无 |

---

## 4. 颜色系统差异

### 4.1 颜色格式

| ink-terminal | OpenTUI React |
|---|---|
| `"ansi:red"`, `"ansi:magenta"` 等命名 ANSI 色 | `"red"`, `"#FF0000"` 命名色或 hex |
| `"ansi:cyanBright"` 亮色变�� | `"#55FFFF"` hex |
| `color` prop on `<Text>` | `fg` prop on `<text>` 或 `<span>` |
| `backgroundColor` prop | `bg` prop |

### 4.2 兼容层颜色映射表

| ink-terminal 名 | hex 值 | 说明 |
|---|---|---|
| `ansi:black` | `#000000` | |
| `ansi:red` | `#CC0000` | |
| `ansi:green` | `#4EC940` | |
| `ansi:yellow` | `#C4A500` | |
| `ansi:blue` | `#3D6DCC` | |
| `ansi:magenta` | `#CC00CC` | |
| `ansi:cyan` | `#00AAAA` | |
| `ansi:white` | `#CCCCCC` | |
| `ansi:blackBright` | `#666666` | 用作 dim 近似色 |
| `ansi:redBright` | `#FF5555` | |
| `ansi:greenBright` | `#55FF55` | |
| `ansi:yellowBright` | `#FFFF55` | |
| `ansi:blueBright` | `#5555FF` | |
| `ansi:magentaBright` | `#FF55FF` | |
| `ansi:cyanBright` | `#55FFFF` | |
| `ansi:whiteBright` | `#FFFFFF` | |

---

## 5. 降级项与已知差异

以下功能在迁移后存在行为降级:

| 功能 | 原行为 | 当前行为 | 影响 | 改进方向 |
|------|--------|---------|------|---------|
| **Markdown 渲染** | 完整 Markdown → 终端���染 (标题、代码块、列表等) | 纯文本输出 | 助手回复失去格式 | 通过 `extend()` 注册 `MarkdownRenderable` |
| **VirtualList** | 仅渲染可视区域内的消息 | 渲染全部消息 | 长对话性能下降 | OpenTUI 未提供等价组件，需自实现 |
| **dim 文本** | ANSI SGR dim attribute (真正的亮度降低) | `fg="#888888"` 灰色近似 | 视觉差异轻�� | 等待 OpenTUI 支持 dim 修饰符 |
| **inverse 文本** | ANSI SGR reverse (前景/背景互换) | 固定 `fg="#000000" bg="#CCCCCC"` | 无法跟随主题自适应 | 等待 OpenTUI 支持 reverse 修饰符 |
| **stopImmediatePropagation** | 阻止事件冒泡到父级 useInput | no-op | 滚动/输入焦点可能冲突 | 等待 OpenTUI 事件冒泡 API |
| **ScrollBox.stickyScroll** | 原生粘性滚动 (新内容自动到底) | useEffect 模拟 | 可能有 1 帧延迟 | 等待 OpenTUI scrollbox 支持 |
| **ScrollBox handle 查询方法** | 精确的 scrollTop/viewportHeight/scrollHeight | 近似值 (基于 `process.stdout.rows`) | 滚动指示器精度下降 | 需要 OpenTUI scrollbox 暴露内部状态 |

---

## 6. 文件变更清单

### 6.1 新增文件

| 文件 | 用途 |
|------|------|
| `ui/src/compat/ink-compat.tsx` | 兼容层 — 唯一直接导入 `@opentui/*` 的文件 |

### 6.2 修改文件

| 文件 | 变更 |
|------|------|
| `ui/package.json` | 移除 `workspaces` + `ink-terminal` dep, 添加 `@opentui/core` + `@opentui/react` |
| `ui/tsconfig.json` | 添加 `"jsxImportSource": "@opentui/react"` |
| `ui/src/main.tsx` | `render()` → `createCliRenderer()` + `createRoot().render()` |
| `ui/src/components/App.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/Header.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/StatusBar.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/InputPrompt.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/MessageList.tsx` | import 改为 `../compat/ink-compat.js`, 移除 `ink-terminal/markdown` |
| `ui/src/components/MessageBubble.tsx` | import 改为 `../compat/ink-compat.js`, 移除 `ink-terminal/markdown` |
| `ui/src/components/PermissionDialog.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/Spinner.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/WelcomeScreen.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/Suggestions.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/ThinkingBlock.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/ToolUseBlock.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/ToolResultBlock.tsx` | import 改为 `../compat/ink-compat.js` |
| `ui/src/components/DiffView.tsx` | import 改为 `../compat/ink-compat.js` |

### 6.3 未��改文件

IPC 层 (`ui/src/ipc/`)、状态管理 (`ui/src/store/`)、Vim 模式 (`ui/src/vim/`)、工具函数 (`ui/src/utils.ts`) 均无变更 — 与渲染层解耦。

---

## 7. 回退方案

所有改动集中在 `ui/` 目录，Rust 后端零变更。ink-terminal git submodule 未删除。

**回退步骤:**
1. `ui/tsconfig.json` 移除 `jsxImportSource`
2. `ui/package.json` 恢复 `workspaces: ["ink-terminal"]`, 添加 `"ink-terminal": "workspace:*"`, 移除 `@opentui/*`
3. `ui/src/main.tsx` 恢复 `import { render } from 'ink-terminal'` 入口
4. 14 个组件文件的 import 从 `'../compat/ink-compat.js'` 改回 `'ink-terminal'`
5. `MessageList.tsx` 和 `MessageBubble.tsx` 恢复 `import { Markdown } from 'ink-terminal/markdown'`
6. 删除 `ui/src/compat/` 目录
7. `bun install` 重新安装

---

## 8. OpenTUI 独有能力 (ink-terminal 无)

以下是 OpenTUI 提供但 ink-terminal 不具备的能力，可在后续利用:

| 能力 | 说明 |
|------|------|
| **原生性能** | Zig 核心 → 渲染速度显著优于纯 JS |
| **tree-sitter 语法高亮** | 内置 `<code>` 组件支持多语言高亮 |
| **Diff 组件** | 内置 `<diff>` 支持 unified/split 视图 |
| **主题模式检测** | `useRenderer().themeMode` 自动检测 dark/light |
| **关键帧动画** | `useTimeline()` 支持 CSS-like 时间线动画 |
| **按键释放/重复事件** | `KeyEvent.eventType` 区分 press/release/repeat |
| **`<input>` / `<textarea>`** | 原生输入组件 (可替代手动光标管理) |
| **`<select>` / `<tab-select>`** | 原生选择组件 |
| **滚动条自定义** | 颜色、箭头、轨道样式 |
| **FrameBuffer** | 像素级终端绘制 |
| **ASCIIFont** | 大字体 banner 渲染 |
| **调试控制台** | `renderer.displayDebugConsole()` |
| **Slot 系统** | 组件插槽注册 + 扩展 |
