# claude-code-bun vs claude-code-rust: Utils 对比总览

对比范围：`claude-code-bun/src/utils/` (TypeScript) ↔ `claude-code-rust/crates/` (Rust)

## 总体规模

| 维度 | claude-code-bun (src/utils/) | claude-code-rust (crates/) |
|------|------------------------------|----------------------------|
| 文件数 | ~600+ (含测试) | ~500+ (含测试) |
| 核心代码行数 | ~90,000+ | ~80,000+ |
| 子模块数 | 37 目录 | 40 crates |
| 成熟度 | 完整产品 | ~70% 功能完整 |

## Rust 已实现的模块 (完整度 >= 90%)

| Crate | 对应 Bun 模块 | 说明 |
|-------|-------------|------|
| cc-types | types/* | 完整类型系统 |
| cc-config | settings/, config.ts | 多源配置 + validation |
| cc-models + cc-api | model/ | 模型 + 多 Provider API |
| cc-auth | auth/** | OAuth + API Key 认证 |
| cc-bootstrap | bootstrap/state, timing | 启动引导 |
| cc-session | session* | 会话持久化/恢复/导出 |
| cc-compact | (src/ 层 compaction) | 上下文压缩 |
| cc-tasks | task/ | 任务系统 (比 Bun 更完整) |
| cc-skills | skills/ | 技能系统 (含依赖解析) |
| cc-sandbox | sandbox/ | 沙箱 (bwrap/seatbelt) |
| cc-mcp | mcp/, services/mcp | MCP 客户端 |
| cc-lsp-service | (src/ 层 LSP) | LSP 服务 |
| cc-keybindings | keyboardShortcuts.ts | 快捷键绑定 |
| cc-browser | claudeInChrome/ | Chrome 浏览器集成 |
| cc-computer-use | computerUse/ | 跨平台计算机控制 |
| cc-daemon | (daemon/ 层) | 守护进程 |
| voice | (voice/ 层) | 语音控制 |
| cc-ipc* | background/, uds* | IPC 通信层 |
| gateway | (bridge/ 层) | 网关 |
| worktree | (worktree 相关) | 工作树 |
| cc-observability | telemetry/ (部分) | 可观测性基础类型 |
| cc-query | (src/ 层 query) | 查询循环 |
| cc-engine | (src/ 层 engine) | 核心引擎 |

## Rust 部分实现/待完善的模块

| Crate | 对应 Bun 模块 | 完整度 | 主要缺口 |
|-------|-------------|--------|---------|
| cc-utils | utils/* (~200 文件) | ~15% | 只移植了 9 个模块，大量工具函数缺失 |
| cc-permissions + cc-safety | permissions/ | ~60% | 规则引擎移植完成，LLM 分类器未集成 |
| cc-plugins | plugins/ | ~40% | 加载系统移植完成，Marketplace/MCPB 未移植 |
| cc-teams | swarm/ | ~50% | InProcess 后端移植完成，tmux/iTerm2 未移植 |
| cc-tools | tools/ (src 层) | ~70% | 核心工具完成，部分辅助工具缺失 |
| cc-services | services (src 层) | ~60% | 基础服务完成，部分高级功能缺失 |
| cc-engine/shell | bash/, shell/ | ~40% | Bash 解析器未移植，Shell Provider 未移植 |
| cc-engine/hooks | hooks/ | ~50% | Hook 执行框架部分移植 |
| Rust TUI | (Ink UI 层) | ~5% | 实现在 `crates/claude-code-rs/src/ui/`，无单独 `cc-ui` crate |

## 完全未移植的模块 (Rust 中无对应)

| Bun 模块 | 文件数 | 行数 | 说明 |
|----------|--------|------|------|
| suggestions/ | 6 | ~1,200 | 命令补全/路径补全/历史补全 |
| processUserInput/ | 5 | ~2,200 | 输入路由/斜杠命令处理 |
| telemetry/ (完整 OTel) | 9 | ~4,000 | OTel/Perfetto/BigQuery 未移植 |
| bash/bashParser.ts | 1 | ~4,400 | 纯 TS 的 Bash 解析器 (Rust 用 shell-words) |
| bash/ast.ts | 1 | ~2,700 | tree-sitter AST 安全分析 |
| shell/readOnlyCommandValidation.ts | 1 | ~1,900 | 只读命令验证 |
| permissions/yoloClassifier.ts | 1 | ~1,500 | 自动模式 LLM 分类器 |
| all 顶级单文件工具 | ~200 | ~ | 大量工具函数 |

## 按功能域划分的完整度

```
Bash 解析器          ████░░░░░░  40%
Shell 提供者          ██████░░░░  60%
权限系统              ██████░░░░  60% (规则 OK, 分类器缺失)
插件系统              ████░░░░░░  40% (加载 OK, 市场缺失)
技能系统              ██████████  100%
任务系统              ██████████  100%
设置管理              ██████████  100%
模型管理              ██████████  100%
Hook 系统             ██████░░░░  60%
Team/Swarm           ██████░░░░  60%
遥测/可观测性         ████░░░░░░  40%
Computer Use         ██████████  100%
MCP                  ██████████  100%
沙箱                  ██████████  100%
自动补全               ░░░░░░░░░░   0%
输入处理               ██░░░░░░░░  20%
工具函数库             ██░░░░░░░░  15%
```
