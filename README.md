# allthecodes

allthecodes 是一个参考 Claude Code 的交互模型、工具系统和 agent 工作流，用 Rust 重新构建的高性能 AI 编程助手。它不是 Claude Code 的官方版本，也不是对原实现的逐文件翻译；这个项目在对照 Claude Code 行为的基础上，重新整理工程结构、运行时边界、IPC、Rust TUI、权限模型和工具系统，并融合了作者自己对 coding agent 的理解。

源项目地址：<https://github.com/Crsei/claude-code-rust.git>

创建这个新项目的直接原因很简单：原来的项目在长期快速迭代后，工程结构、模块边界和前后端耦合都变得比较混乱。继续在原结构里堆功能会让维护成本越来越高，所以 allthecodes 选择另开一个 Rust workspace，把核心能力重新梳理一遍，用更清晰的类型边界、更强的路径隔离和更可维护的模块组织来承载后续功能。

## 项目状态

allthecodes 目标是尽可能完整地覆盖 Claude Code 类工具的核心体验，并在 Rust 实现中补齐高性能、强类型、可观测和可扩展的工程基础。

目前功能完整度已经非常高，覆盖了日常 agent 编程工作所需的大部分能力。项目仍在持续更新，后续会继续对齐核心体验，并开发新的功能和自己的扩展能力。

这个项目仍可能存在 bug、边界行为不一致或平台相关问题。如果你在使用中遇到问题，可以通过 Issue 或 PR 反馈。

反馈入口：<https://github.com/Crsei/claude-code-rust/issues>

## 核心目标

- 用 Rust 构建高性能、可维护的 coding agent 运行时。
- 参考 Claude Code 的成熟交互体验，但不被原项目的历史包袱限制。
- 保持完整功能取向，不以 Lite 或 Demo 为边界。
- 强化工具执行、权限控制、会话恢复、路径隔离和 Rust TUI 的工程质量。
- 在兼容主流工作流的同时，逐步加入 allthecodes 自己的新功能。

## 主要能力

- 流式对话引擎：负责消息提交、上下文管理、工具调用、预算控制和结果输出。
- Rust TUI：基于 ratatui/crossterm 的终端交互界面。
- Headless IPC：支持 JSONL over stdio，方便外部前端、自动化测试和集成场景接入。
- 工具系统：覆盖文件、搜索、Shell、任务、Agent、LSP、Web、MCP、Computer Use 等能力。
- 权限与沙箱：按工具和运行模式控制高风险操作，降低误执行风险。
- Skills 与插件：支持内置能力、用户技能和插件扩展。
- 多后端模型接入：支持 Anthropic、OpenAI、Google、Azure、OpenAI Codex 等提供商相关路径。
- 会话与记忆：支持会话持久化、恢复、上下文压缩和长期记忆相关能力。
- Daemon/Web 模式：为后台任务、服务化运行和 Web UI 预留并持续完善运行时基础。

## 路径隔离

allthecodes 与 Claude Code/Codex 等工具可以共存在同一台机器上。为了避免互相污染，持久化路径默认使用 allthecodes 自己的目录。

| 用途 | 路径 |
| --- | --- |
| 全局数据目录 | `~/.allthecodes/` |
| 项目配置 | `.allthecodes/settings.json` |
| 项目技能 | `.allthecodes/skills/` |
| 凭据文件 | `~/.allthecodes/credentials.json` |
| Keychain 服务名 | `allthecodes` |

也可以通过 `ALLTHECODES_HOME` 指定全局数据目录。

## 构建

请先确保本机已安装 Rust toolchain，并且 `cargo`、`rustc`、`rustup` 可在当前 shell 中使用：

```bash
cargo --version
rustc --version
rustup show active-toolchain
```

从仓库根目录构建：

```bash
cargo build --workspace --release
```

运行版本检查：

```bash
target/release/allthecodes --version
```

交互式启动：

```bash
target/release/allthecodes
```

非交互式执行：

```bash
target/release/allthecodes -p "summarize this repository"
```

Web UI 模式：

```bash
target/release/allthecodes --web --web-port 3001
```

发布构建使用 GitHub Actions 的 `release` workflow。推送 `vX.Y.Z` tag
时，tag 版本必须与 `crates/allthecodes/Cargo.toml` 一致；手动
`workflow_dispatch` 设置 `dry_run=true` 时只构建并暂存 npm tarball，不创建
GitHub Release，也不发布 npm。

本地验证 npm staging 时使用：

```bash
npm_config_cache=/tmp/allthecodes-npm-cache \
python3 scripts/stage_npm_packages.py \
  --release-version 0.1.0-test.1 \
  --package allthecodes-linux-x64 \
  --output-dir /tmp/allthecodes-npm-host-stage
```

`npm_config_cache` 只在当前机器的默认 npm cache/log 目录不可写时需要。

## 开发说明

仓库按 Rust workspace 拆分，核心 crate 位于 `crates/` 下。

| 模块 | 说明 |
| --- | --- |
| `crates/allthecodes` | CLI、TUI、模式选择和应用入口 |
| `crates/allthecodes-engine` | QueryEngine、agent runtime 和核心生命周期 |
| `crates/allthecodes-query` | 流式查询循环 |
| `crates/allthecodes-tools` | 工具注册、工具实现和工具策略 |
| `crates/allthecodes-commands` | Slash command 系统 |
| `crates/allthecodes-config` | 配置、路径和环境变量管理 |
| `crates/allthecodes-auth` | API key、OAuth、credentials 和 keychain 相关逻辑 |
| `crates/allthecodes-permissions` | 权限模式和危险操作控制 |
| `crates/allthecodes-session` | 会话持久化与恢复 |
| `crates/allthecodes-ipc*` | IPC 协议、传输和适配层 |
| `crates/allthecodes-skills` | Skills 加载、定义和使用统计 |
| `crates/allthecodes-plugins` | 插件发现、安装和运行时接入 |
| `crates/allthecodes-daemon` | 常驻后台模式和任务运行时 |
| `crates/allthecodes-web` | Web UI 服务和静态资源入口 |

开发时建议优先运行：

```bash
cargo fmt --all --check
cargo build --workspace --release
```

针对具体 crate 的测试示例：

```bash
cargo test -p allthecodes
cargo test -p allthecodes-tools
```

## 兼容与差异

allthecodes 会参考 Claude Code 的核心体验，但不会把原项目的历史结构照搬进 Rust。对于上游已有但当前实现仍有差异的部分，会按完整实现目标逐步补齐；如果某些缩减是有意保留，会在文档或变更说明中明确标注。

这意味着 allthecodes 的方向不是做一个最小可用替代品，而是做一个长期维护的 Rust 高性能版本。功能对齐、架构整理和新功能开发会同时推进。

## Bug 反馈

如果遇到问题，请尽量提供以下信息：

- 使用的系统和终端环境。
- `allthecodes --version` 输出。
- 触发问题的命令或操作步骤。
- 相关配置路径是否使用了 `ALLTHECODES_HOME`。
- 可公开的错误日志、截图或最小复现仓库。

Issue 地址：<https://github.com/Crsei/claude-code-rust/issues>

## License

allthecodes is licensed under the Apache License 2.0.
