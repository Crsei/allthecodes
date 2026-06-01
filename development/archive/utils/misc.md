# 其他模块和综合总结

## 已完整移植 (无重大缺口)

### cc-engine/cc-query
Bun 的查询循环和引擎层大部分已移植到 Rust 的 `cc-engine` 和 `cc-query` crate。  
主要特点: 生命周期子系统、查询循环、系统提示组装 (1,542 行)、Agent 子系统、工具运行时。

**已知小缺口**:
- `cc-engine/src/mcp/` 目录为空 (尚未提取)
- `cc-engine/src/teams/` 目录为空 (尚未提取)  
- `cc-engine/src/prompt_sections/` 目录为空
- `skill_tool.rs`: Fork 执行模式 "尚未实现，回退到内联"
- `cc-engine/codex_exec.rs`: 最小实现 (仅 backend 名称规范化)
- `keep-coding-instructions" 尚未实现`

### cc-types
完整的类型层级移植，包含消息、Agent、MCP、权限、类型。  
17 个模块，全部功能性。

### cc-session
完整的会话持久化系统，包含导出/恢复/分叉/记忆目录。

### cc-compact
完整的上下文压缩系统，包含自动压缩、微压缩、部分压缩、上下文折叠、snip 等。

### cc-daemon
完整的守护进程实现。

### cc-lsp-service
完整的 LSP 客户端、转换格式、工具包装。

### cc-keybindings
完整的键绑定系统。

### cc-browser
完整的 Chrome 浏览器集成。

### voice
完整的语音控制模块。

### cc-ipc*
完整的 IPC 通信层 (protocol/client/transport/adapters)。

### gateway / web / worktree / start-up
完整的网关、Web、工作树和启动模块。

## 综合移植优先级

| 优先级 | 模块 | 估计工作量 | 依赖 |
|--------|------|-----------|------|
| **P0 (安全关键)** | 只读命令验证 (1,893 行) | 2-3 天 | 无 |
| **P0 (安全关键)** | 文件权限检查 (1,782 行) | 3-5 天 | cc-permissions |
| **P0 (核心功能)** | Shell 提供者 (bash + powershell) | 2-3 天 | cc-engine |
| **P1 (自动模式)** | YOLO/LLM 分类器 (1,510 行) | 5-7 天 | cc-safety, cc-api |
| **P1 (UX)** | 命令补全系统 (1,200 行) | 3-5 天 | 无 |
| **P1 (UX)** | 输入路由/斜杠命令 (2,200 行) | 5-7 天 | cc-engine |
| **P2 (功能)** | Marketplace 移植 (2,600 行) | 5-10 天 | cc-plugins |
| **P2 (功能)** | MCPB Handler (966 行) | 2-3 天 | cc-plugins |
| **P2 (功能)** | Hook 配置管理 (400 行) | 1-2 天 | cc-tools/hooks |
| **P3 (补充)** | 通用工具函数 (~200 文件) | 10-15 天 | 无 |
| **P3 (补充)** | 遥测/OTel (4,000 行) | 5-10 天 | cc-observability |
| **P3 (补充)** | MDM 企业设置 (526 行) | 2-3 天 | cc-config |
| **P3 (补充)** | Win32 COM (Word/Excel) | 3-5 天 | cc-computer-use |

## 架构差异要点

1. **Bash 解析**: Bun 有自实现的 TypeScript 解析器 (4,400 行)，Rust 使用 `shell-words` crate。建议 Rust 直接集成 `tree-sitter-bash`，避免重写。

2. **插件运行时**: Bun (基于 JavaScript) 可以直接运行插件代码；Rust 只能通过子进程执行。这是一个根本性的架构差异。

3. **团队后端**: Bun 支持 tmux/iTerm2/Windows Terminal 多后端；Rust 有意识地裁剪为仅 InProcess。

4. **遥测**: Bun 有完整的 OpenTelemetry/Perfetto 集成；Rust 只有基础事件类型。这可能是故意的简化。

5. **设置变更检测**: Bun 使用 chokidar 文件监控；Rust 的 cc-config 似乎使用更简单的加载策略。

6. **UI 层**: Bun 使用 React/Ink 的完整 TUI；Rust TUI 实现在 `crates/claude-code-rs/src/ui/`，不再保留单独的 `cc-ui` 占位 crate。这表明 Rust 使用了不同的终端 UI 框架。

## 文件清单

以下文件已保存到 `claude-code-rust/docs/utils/`:

| 文件 | 说明 |
|------|------|
| overview.md | 综合对比总览 |
| core-utilities.md | 通用工具函数缺口 (~200 文件) |
| bash-shell.md | Bash 解析和 Shell 提供者缺口 |
| permissions-classifier.md | 权限系统和 LLM 分类器缺口 |
| plugins-marketplace.md | 插件系统缺口 |
| suggestions-input.md | 建议系统和输入处理缺口 |
| hooks.md | Hook 系统缺口 |
| telemetry-observability.md | 遥测/可观测性缺口 |
| teams-swarm.md | Team/Swarm 系统缺口 |
| settings-mdm.md | 设置系统和 MDM 缺口 |
| computer-use.md | Computer Use 缺口 |
