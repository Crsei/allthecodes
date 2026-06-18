# ink-ui — ink-terminal 终端 UI 实验

> 基于 ink-terminal 渲染库的第二个终端 UI，独立于现有 OpenTUI UI，复用相同的 IPC 协议。

---

## 1. 目标

- 在 `rust/ink-ui/` 创建独立的终端 UI 实验项目
- 使用 ink-terminal（git submodule）作为渲染层
- 复用现有 IPC 协议（spawn Rust 后端 + `--headless` JSONL stdio）
- 最小可用 REPL：消息列表 + 输入框 + 流式输出

## 2. 目录结构

```
rust/ink-ui/
├── ink-terminal/                  git submodule (Crsei/ink-terminal)
├── src/
│   ├── main.tsx                   入口：spawn 后端 + render React 树
│   ├── ipc/
│   │   ├── client.ts              RustBackend 类 (复制自 ui/src/ipc/client.ts)
│   │   ├── protocol.ts            IPC 协议类型 (复制自 ui/src/ipc/protocol.ts)
│   │   └── context.tsx            BackendProvider (复制自 ui/src/ipc/context.tsx)
│   ├── store/
│   │   ├── app-store.tsx          应用状态 (复制自 ui/src/store/app-store.tsx)
│   │   └── message-model.ts       消息模型 (复制自 ui/src/store/message-model.ts)
│   └── components/
│       └── App.tsx                根组件 (从零写)
├── package.json
├── tsconfig.json
├── run.sh
└── run.ps1
```

## 3. 文件来源

### 3.1 复制文件（从 `ui/src/` 原样复制）

| 源文件 | 目标文件 | 需要的修改 |
|--------|---------|-----------|
| `ui/src/ipc/client.ts` | `ink-ui/src/ipc/client.ts` | 无 |
| `ui/src/ipc/protocol.ts` | `ink-ui/src/ipc/protocol.ts` | 无 |
| `ui/src/ipc/context.tsx` | `ink-ui/src/ipc/context.tsx` | 改 import: `@opentui/react` → `ink-terminal/react` |
| `ui/src/store/app-store.tsx` | `ink-ui/src/store/app-store.tsx` | 改 import: `@opentui/react` → `ink-terminal/react` |
| `ui/src/store/message-model.ts` | `ink-ui/src/store/message-model.ts` | 无（纯逻辑，无 UI 依赖） |
| `ui/run.sh` | `ink-ui/run.sh` | 改路径指向 `ink-ui/src/main.tsx` |
| `ui/run.ps1` | `ink-ui/run.ps1` | 同上 |

### 3.2 新写文件

| 文件 | 说明 |
|------|------|
| `src/main.tsx` | 入口：解析 `CC_RUST_BINARY`，spawn 后端，创建 ink-terminal renderer |
| `src/components/App.tsx` | 根组件：消息列表 + 输入框 + 流式输出 |
| `package.json` | 独立包配置 |
| `tsconfig.json` | TypeScript 配置 |

## 4. package.json

```json
{
  "name": "cc-rust-ink-ui",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "bun run src/main.tsx",
    "build": "bun build src/main.tsx --target=bun --outdir=dist"
  },
  "dependencies": {
    "ink-terminal": "file:./ink-terminal",
    "react": "^19.0.0",
    "react-reconciler": "^0.33.0"
  },
  "devDependencies": {
    "@types/react": "^19.0.0",
    "typescript": "^5.0.0"
  }
}
```

## 5. tsconfig.json

```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "jsxImportSource": "ink-terminal/react",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "outDir": "dist"
  },
  "include": ["src"]
}
```

注意：ink-terminal 的 `exports` 中 `"./react"` 子路径在 bun 环境下解析为 `src/react/index.ts`。`jsxImportSource` 需要 ink-terminal 提供 `react/jsx-runtime` 导出。如果不支持，回退到 `"jsx": "react-jsx", "jsxImportSource": "react"` 并在组件中手动 import ink-terminal 组件。

## 6. main.tsx 入口

```typescript
import { render } from 'ink-terminal/react'
import { RustBackend } from './ipc/client.js'
import { BackendProvider } from './ipc/context.js'
import { AppStateProvider } from './store/app-store.js'
import App from './components/App.js'

// 1. 解析二进制路径
const binaryPath = process.env.CC_RUST_BINARY || findBinary()

// 2. Spawn Rust 后端
const backend = new RustBackend(binaryPath, process.argv.slice(2))

// 3. 渲染 React 树
const { waitUntilExit } = render(
  <BackendProvider backend={backend}>
    <AppStateProvider>
      <App />
    </AppStateProvider>
  </BackendProvider>
)

// 4. 清理
backend.on('exit', () => process.exit(0))
await waitUntilExit()
```

`findBinary()` 按优先级搜索：
1. `../target/release/claude-code-rs` (或 `.exe`)
2. `../target/debug/claude-code-rs`

## 7. 最小 App.tsx

初始版本只需验证 IPC 链路：

```typescript
import { Box, Text, useInput } from 'ink-terminal/react'
import { useBackend } from '../ipc/context.js'
import { useAppState } from '../store/app-store.js'

export default function App() {
  const backend = useBackend()
  const { state, dispatch } = useAppState()

  useInput((input, key) => {
    if (key.return && inputText.trim()) {
      backend.send({ type: 'submit_prompt', text: inputText, id: crypto.randomUUID() })
    }
  })

  return (
    <Box flexDirection="column" height="100%">
      {/* 消息列表 */}
      <Box flexGrow={1} flexDirection="column">
        {state.messages.map((msg, i) => (
          <Text key={i}>{msg.role}: {msg.text}</Text>
        ))}
      </Box>
      {/* 输入区 */}
      <Box borderStyle="single">
        <Text>{'> '}{inputText}</Text>
      </Box>
    </Box>
  )
}
```

具体组件 API 取决于 ink-terminal 实际导出，实现时按实际调整。

## 8. git submodule 设置

```bash
cd rust/ink-ui
git submodule add https://github.com/Crsei/ink-terminal.git ink-terminal
```

使用与 `ui/ink-terminal/` 相同的远程仓库。

## 9. 启动脚本

**run.sh:**
```bash
#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -z "$CC_RUST_BINARY" ]; then
  for candidate in \
    "$SCRIPT_DIR/../target/release/claude-code-rs" \
    "$SCRIPT_DIR/../target/debug/claude-code-rs"; do
    [ -x "$candidate" ] && CC_RUST_BINARY="$candidate" && break
  done
fi
export CC_RUST_BINARY
exec bun run "$SCRIPT_DIR/src/main.tsx" "$@"
```

**run.ps1:** 同逻辑的 PowerShell 版本，搜索 `.exe` 后缀。

## 10. 与现有 UI 的隔离

| 维度 | ui/ (OpenTUI) | ink-ui/ (ink-terminal) |
|------|-------------|----------------------|
| 包名 | `cc-rust-ui` | `cc-rust-ink-ui` |
| 渲染库 | `@opentui/core` + `@opentui/react` | `ink-terminal/react` |
| submodule | `ui/ink-terminal/`（未使用） | `ink-ui/ink-terminal/`（核心依赖） |
| IPC 协议 | `ui/src/ipc/` | `ink-ui/src/ipc/`（复制） |
| 状态管理 | `ui/src/store/` | `ink-ui/src/store/`（复制） |
| 启动脚本 | `ui/run.sh` | `ink-ui/run.sh` |
| workspace | bun workspace root | 独立（不参与 `ui/` workspace） |

两个 UI 完全独立，不共享 `node_modules`，不互相影响。
