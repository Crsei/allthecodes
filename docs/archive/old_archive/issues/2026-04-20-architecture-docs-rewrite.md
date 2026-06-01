# Issue: 重写 `docs/architecture`，使其与当前 `cc-rust` 代码架构一致

Labels: `documentation`, `architecture`, `cleanup`
Priority: `P1`

## 背景

当前 `docs/architecture/` 下大量文档仍在介绍上游 Claude Code / CCB 的 TypeScript 架构，
例如直接引用：

- `src/main.tsx`
- `src/QueryEngine.ts`
- `src/query.ts`
- `src/tools.ts`
- `src/services/api/claude.ts`

但本仓库当前实际维护的是 `cc-rust`，核心代码结构已经变为：

- Rust 后端入口与启动链路：`src/main.rs`、`src/cli.rs`、`src/startup/*`
- QueryEngine 与生命周期：`src/engine/*`、`src/query/*`、`src/types/*`
- 工具与权限系统：`src/tools/*`、`src/permissions/*`、`src/sandbox/*`
- 上下文压缩、会话与状态：`src/compact/*`、`src/session/*`、`src/services/*`
- 前端与通信：`src/ipc/*`、`src/ui/*`、`src/web/*`、`ui/src/*`、`ink-ui/src/*`
- 扩展与高级能力：`src/skills/*`、`src/plugins/*`、`src/mcp/*`、`src/lsp_service/*`、`src/teams/*`、`src/daemon/*`、`src/browser/*`、`src/computer_use/*`、`src/voice/*`

结果是：当前 `docs/architecture/` 既不能作为本项目的源码导览，也不能准确反映 `rust-lite`
分支的能力边界与未完成项。

## 问题

### 1. 架构文档主体仍然是“上游产品说明”，不是“本项目源码架构说明”

典型例子：

- `docs/architecture/introduction/architecture-overview.mdx` 仍在描述五层 TS 架构与 React/Ink REPL
- `docs/architecture/features/all-features-guide.md` 仍在介绍 Buddy、Remote Control、Schedule、Voice、Chrome 等一整套并非 `rust-lite` 当前主线的能力

### 2. “当前架构”、“历史设计”、“实验方案”、“任务拆解”混在同一目录层级

`docs/architecture/` 下同时存在：

- canonical 架构介绍
- feature 设计文档
- task 拆解
- test plan
- 上游能力调研

导致读者无法判断：

- 哪些是当前代码已落地的事实
- 哪些只是设计草案或历史记录
- 哪些在 `rust-lite` 中明确不实现

### 3. 未完成状态标注方式不统一

当前仓库已有状态来源，但 `docs/architecture/` 没有统一消费：

- 能力完成度基线：[`docs/WORK_STATUS.md`](F:\AIclassmanager\cc\rust\docs\WORK_STATUS.md)
- 缩减实现 / 设计限制：[`docs/IMPLEMENTATION_GAPS.md`](F:\AIclassmanager\cc\rust\docs\IMPLEMENTATION_GAPS.md)
- 用户可感知问题：[`docs/KNOWN_ISSUES.md`](F:\AIclassmanager\cc\rust\docs\KNOWN_ISSUES.md)

结果是某些文档把未完成内容写成“已有能力”，某些文档完全不标，某些内容则只藏在其他文档里。

## 目标

把 `docs/architecture/` 改造成“以当前仓库代码为准”的架构文档集：

- 主要介绍当前 `cc-rust` 的代码结构、运行链路和模块边界
- 明确区分“已实现 / 部分实现 / 未完成 / rust-lite 不做”
- 把历史方案、实验能力、任务分解从 canonical 架构说明中剥离出来
- 让新开发者可以直接根据文档定位到真实源码

## 范围

### A. 重写 canonical 架构章节

以下目录应以“当前仓库代码”为依据重写，而不是继续沿用上游产品叙述：

- `docs/architecture/introduction/`
- `docs/architecture/conversation/`
- `docs/architecture/context/`
- `docs/architecture/tools/`
- `docs/architecture/safety/`
- `docs/architecture/agent/`
- `docs/architecture/extensibility/`

建议以真实源码模块组织内容：

1. 进程启动与运行模式
   - `src/main.rs`
   - `src/cli.rs`
   - `src/startup/*`
   - 覆盖 TUI / `--headless` / `--web` / daemon / fast path

2. QueryEngine 与单轮/多轮查询循环
   - `src/engine/*`
   - `src/engine/lifecycle/*`
   - `src/query/*`
   - `src/types/*`

3. 工具注册、执行与权限决策
   - `src/tools/*`
   - `src/permissions/*`
   - `src/sandbox/*`
   - `src/commands/*`

4. 上下文压缩、会话持久化与状态服务
   - `src/compact/*`
   - `src/session/*`
   - `src/services/*`

5. 前端与通信架构
   - `src/ipc/*`
   - `src/ui/*`
   - `src/web/*`
   - `ui/src/*`
   - `ink-ui/src/*`
   - `web-ui/*`

6. 扩展能力
   - `src/skills/*`
   - `src/plugins/*`
   - `src/mcp/*`
   - `src/lsp_service/*`

7. 高级模块与特性边界
   - `src/teams/*`
   - `src/daemon/*`
   - `src/browser/*`
   - `src/computer_use/*`
   - `src/voice/*`
   - `src/auth/*`
   - `src/api/*`

### B. 对 `features/`、`internals/`、`task/`、`test-plans/` 做分流

这些目录不应默认被读者视为“当前正式架构说明”。

需要逐项处理为以下三类之一：

- 保留在 `docs/architecture/`，但明确标为“专题设计 / 实验能力 / 历史方案”
- 移到 `docs/archive/` 或其他更合适的位置
- 删除明显已经过时、且没有继续保留价值的文档

最少要做到：

- 目录首页或文档头部明确该文档类型
- 避免与 canonical 架构页混淆

### C. 更新图示与图片资源

`docs/architecture/images/`、`docs/architecture/diagrams/` 中的图不应继续表达上游 TS 结构。

至少需要替换为当前项目的真实链路：

- 启动与模式切换
- QueryEngine / query loop
- 工具执行与权限流
- IPC / headless / Web / TUI 关系
- 扩展层（skills / plugins / MCP / LSP）

## 未完成项统一标注规范

后续所有架构文档统一使用以下状态标签，不要自由发挥：

- `[Implemented]` 已按当前描述落地到代码
- `[Partial]` 已有代码与入口，但能力不完整、行为有明显限制
- `[Open]` 文档提到但当前尚未完成，或代码中仍是占位/未打通
- `[Deferred in rust-lite]` 明确不在当前 `rust-lite` 范围内
- `[Historical]` 历史方案或调研，仅供参考，不代表当前实现

### 标注规则

1. 任何涉及未完成功能的页面或章节，顶部必须出现“当前状态”段落。
2. “当前状态”必须链接到统一来源之一：
   - [`docs/WORK_STATUS.md`](F:\AIclassmanager\cc\rust\docs\WORK_STATUS.md)
   - [`docs/IMPLEMENTATION_GAPS.md`](F:\AIclassmanager\cc\rust\docs\IMPLEMENTATION_GAPS.md)
   - [`docs/KNOWN_ISSUES.md`](F:\AIclassmanager\cc\rust\docs\KNOWN_ISSUES.md)
3. 不能把“设计目标”写成“现有事实”。
4. 如果某能力在 `rust-lite` 中明确不做，必须写 `[Deferred in rust-lite]`，不能只写“未来计划支持”。

### 当前应明确标注的典型项

- API provider 中 Bedrock / Vertex 仍未实现
- Team Memory 客户端同步仍未打通
- OpenTUI / TS 前端 resize reflow 仍 open
- background agent 的 worktree isolation、permission callback、取消机制仍有设计限制
- 远程控制、server mode、部分 remote/monitor/MDM/analytics 能力不在 `rust-lite` 范围内

## 建议执行步骤

1. 先做一次 `docs/architecture/` 全量审计，给每篇文档打上：
   - `rewrite`
   - `archive`
   - `delete`
   - `keep-with-status`
2. 先重写首页与主干页：
   - `introduction`
   - `conversation`
   - `context`
   - `tools`
   - `safety`
3. 再处理 agent / extensibility / frontends / advanced features。
4. 最后再清理 feature/task/test-plan/historical 文档归档与链接。

## 验收标准

- `docs/architecture/` 的 canonical 页面不再把上游 Claude Code / CCB 的 TS 架构当成当前实现来介绍
- 每篇 canonical 架构页都能直接映射到本仓库的真实源码路径
- 所有未完成内容都使用统一状态标签
- 实验/历史/任务型文档与正式架构说明完成分层
- 目录内主要图示已经替换为 `cc-rust` 当前架构
- 新人仅阅读 `docs/architecture/` + `WORK_STATUS.md` + `IMPLEMENTATION_GAPS.md`，即可正确理解当前项目的代码结构和边界

## 参考依据

- [`src/main.rs`](F:\AIclassmanager\cc\rust\src\main.rs)
- [`Cargo.toml`](F:\AIclassmanager\cc\rust\Cargo.toml)
- [`docs/cc-rust-overview.md`](F:\AIclassmanager\cc\rust\docs\cc-rust-overview.md)
- [`docs/WORK_STATUS.md`](F:\AIclassmanager\cc\rust\docs\WORK_STATUS.md)
- [`docs/IMPLEMENTATION_GAPS.md`](F:\AIclassmanager\cc\rust\docs\IMPLEMENTATION_GAPS.md)
- [`docs/KNOWN_ISSUES.md`](F:\AIclassmanager\cc\rust\docs\KNOWN_ISSUES.md)
- [`docs/architecture/introduction/architecture-overview.mdx`](F:\AIclassmanager\cc\rust\docs\architecture\introduction\architecture-overview.mdx)
- [`docs/architecture/features/all-features-guide.md`](F:\AIclassmanager\cc\rust\docs\architecture\features\all-features-guide.md)
