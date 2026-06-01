# `claude-code-bun` 中 `entrypoints` 与前端 UI 的结构图

> 目标：说明 `src/entrypoints/*` 在 `claude-code-bun` 中的主要职责，以及它们和 Ink/React 终端 UI 的调用关系。

## 一、核心结论

`entrypoints` 不是前端 UI 本身，而是 **启动与分流层**：

- 先判断这次进程启动应该走哪条路径
- 再做全局初始化
- 最后才决定是否挂载交互式 REPL UI

所以它和前端 UI 的关系不是“同层”，而是：

```text
entrypoints / main
    ↓
初始化 + 模式分流
    ↓
交互式路径时才进入 App + REPL
```

## 二、总结构图

```text
                                      +----------------------------------+
                                      | src/entrypoints/cli.tsx          |
                                      | 最外层启动入口 / fast-path 分流   |
                                      +----------------+-----------------+
                                                       |
                      +--------------------------------+----------------------------------+
                      |                                |                                  |
                      | fast-path                       | fast-path                         | 默认完整 CLI
                      |                                |                                  |
          +-----------v-----------+        +-----------v------------+         +------------v-------------+
          | remote-control/daemon |        | mcp / runner / bg 等  |         | src/main.tsx            |
          | bridgeMain / daemon   |        | 非 REPL 专用入口       |         | 完整 CLI 装配中心        |
          +-----------------------+        +------------------------+         +------------+-------------+
                                                                                           |
                                                                                           |
                                                                                +----------v-----------+
                                                                                | src/entrypoints/     |
                                                                                | init.ts              |
                                                                                | 全局初始化           |
                                                                                +----------+-----------+
                                                                                           |
                                                                                           v
                                                                                +----------+-----------+
                                                                                | main.tsx 模式分流     |
                                                                                +----+-----------+-----+
                                                                                     |           |
                                                                      interactive    |           | headless / SDK
                                                                                     |           |
                                                        +----------------------------+           +-----------------------------+
                                                        |                                                              |
                                              +---------v----------+                                         +---------v----------+
                                              | getRenderContext   |                                         | headlessStore      |
                                              | createRoot         |                                         | + AppState         |
                                              +---------+----------+                                         +---------+----------+
                                                        |                                                              |
                                              +---------v----------+                                         +---------v----------+
                                              | showSetupScreens   |                                         | cli/print.ts       |
                                              | trust/onboarding   |                                         | runHeadless        |
                                              +---------+----------+                                         +---------+----------+
                                                        |                                                              |
                                              +---------v----------+                                         +---------v----------+
                                              | src/replLauncher   |                                         | StructuredIO /     |
                                              | launchRepl         |                                         | RemoteIO           |
                                              +---------+----------+                                         +---------+----------+
                                                        |                                                              |
                                              +---------v----------+                                         +---------v----------+
                                              | src/components/    |                                         | src/entrypoints/   |
                                              | App.tsx            |                                         | sdk/*              |
                                              | AppStateProvider   |                                         | 通信协议类型/Schema |
                                              +---------+----------+                                         +--------------------+
                                                        |
                                              +---------v----------+
                                              | src/screens/REPL   |
                                              | 交互式前端 UI      |
                                              +--------------------+
```

## 三、交互式 UI 路径

### 1. `src/entrypoints/cli.tsx`

作用：

- 作为真正的进程入口
- 优先处理 `--version`、`remote-control`、`daemon`、runner、`mcp` 等 fast path
- 只有当这些分支都不命中时，才导入 `main.tsx`

这一步和 UI 的关系：

- 它决定“要不要进入 UI”
- 它自己不渲染 UI

### 2. `src/main.tsx`

作用：

- 完整解析 CLI 参数
- 调用 `init()`
- 构建 session、model、permission、MCP、resume、remote 等运行上下文
- 在 interactive / headless / assistant / remote attach 等模式之间分流

这一步和 UI 的关系：

- 它是 **UI 的上游装配器**
- UI 需要的初始状态、上下文、挂载条件，都是它准备的

### 3. `src/entrypoints/init.ts`

作用：

- 启用配置系统
- 应用环境变量
- 配置代理、mTLS、遥测、cleanup、远程设置加载等

这一步和 UI 的关系：

- 提供运行环境
- 不参与 UI 渲染

### 4. `getRenderContext()` + `createRoot()` + `showSetupScreens()`

作用：

- 创建 Ink 根节点
- 建立 FPS / stats / render options
- 在真正进入 REPL 之前显示 trust、onboarding、权限模式提示等 setup UI

这一步和 UI 的关系：

- 这是 **UI 挂载前的前置交互层**
- 仍然属于启动流程，而不是 REPL 主界面本体

### 5. `src/replLauncher.tsx`

作用：

- 真正把 `<App>` 和 `<REPL>` 接起来

它做的事情非常薄：

```tsx
<App {...appProps}>
  <REPL {...replProps} />
</App>
```

这一步和 UI 的关系：

- 这是 entrypoint/启动层和前端 UI 的“最后一跳”

### 6. `src/components/App.tsx`

作用：

- 包装 `AppStateProvider`
- 提供 stats / fps / theme / app state 上下文

这一步和 UI 的关系：

- 它是 UI 根包装层
- 但还不是具体界面逻辑

### 7. `src/screens/REPL.tsx`

作用：

- 真正的交互式前端 UI
- 负责消息渲染、输入框、快捷键、权限弹窗、bridge 状态、remote session、task 列表等

这一步和 UI 的关系：

- 这里才是“前端 UI 本体”

## 四、headless / SDK 路径

并不是所有 `entrypoints` 都会走到 REPL。

当 `main.tsx` 发现：

- `--sdk-url`
- `-p` / `--print`
- stream-json 输入输出

它会转去 headless 路径：

```text
main.tsx
  -> 构造 headlessStore
  -> cli/print.ts runHeadless()
  -> StructuredIO / RemoteIO
  -> 通过 entrypoints/sdk/* 里定义的协议和外部宿主通信
```

这一条路径和 UI 的关系：

- 不挂载 `REPL.tsx`
- 前端不再是本地 Ink UI
- “前端”可能变成 IDE、bridge、remote session host、其他 SDK consumer

也就是说：

- `entrypoints/sdk/*` 更像 **外部前端/宿主的通信合同**
- `components/App.tsx` / `screens/REPL.tsx` 才是 **本地终端 UI**

## 五、`entrypoints` 目录里各文件的角色

| 文件 | 主要作用 | 与前端 UI 的关系 |
|---|---|---|
| `src/entrypoints/cli.tsx` | 最外层进程入口，fast-path 分流 | 决定是否进入 UI |
| `src/entrypoints/init.ts` | 一次性初始化 | 给 UI 备环境，不渲染 UI |
| `src/entrypoints/mcp.ts` | 启动 MCP stdio server | 与 REPL UI 并列，不属于 UI |
| `src/entrypoints/sdk/*` | SDK/bridge/control 协议 schema 和类型 | 定义外部宿主与 CLI 的通信边界 |
| `src/entrypoints/agentSdkTypes.ts` | SDK 公共导出面 | 给 SDK 消费方用，不是 UI 组件 |
| `src/entrypoints/sandboxTypes.ts` | sandbox 相关共享类型 | 间接服务 UI/SDK，但不是 UI |
| `src/entrypoints/src/*` | 生成出来的 type stub / 兼容层 | 基本不参与运行时 UI 逻辑 |

## 六、最简调用链

### 交互式终端 UI

```text
cli.tsx
  -> main.tsx
  -> init.ts
  -> getRenderContext + createRoot
  -> showSetupScreens
  -> launchRepl
  -> App.tsx
  -> REPL.tsx
```

### headless / remote / SDK

```text
cli.tsx
  -> main.tsx
  -> init.ts
  -> headlessStore
  -> cli/print.ts runHeadless
  -> StructuredIO / RemoteIO
  -> entrypoints/sdk/*
```

## 七、一句话总结

`entrypoints` 是 **Claude Code Bun 的启动边界和协议边界**。

它们的主要职责是：

- 进程入口
- 模式分流
- 初始化
- 协议定义

前端 UI 的职责则是：

- 接收已经装配好的 state / session / IO
- 渲染交互界面
- 承接用户输入和会话呈现

所以从架构上说：

```text
entrypoints 在 UI 上游
UI 是 entrypoints 选择出的其中一条执行路径
```
