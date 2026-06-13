# AGENTS.md - allthecodes (Full Build)

This file provides guidance to Codex when working with the Rust port in `rust/`.

## 当前阶段：全量构建（Full Build）

> **重要**：本分支历史上曾标记为 `rust-lite`（完整版的精简版）。现已进入**全量构建阶段**，目标是与上游完整版 (`master` / TypeScript `cc/src/`) 行为对齐。
>
> 书写与审阅规则：
> - **不再按 Lite 缩减**。新代码应覆盖上游对应模块的完整行为，不要以“精简版”为由省略分支、截断、错误恢复、沙箱、Rust TUI 细节等。
> - **已有的缩减实现视为 TODO**，不是既定边界。清单见 [`docs/archive/IMPLEMENTATION_GAPS.md`](docs/archive/IMPLEMENTATION_GAPS.md) §2 与 [`docs/archive/COMPLETED_SIMPLIFIED.md`](docs/archive/COMPLETED_SIMPLIFIED.md)；补齐后迁移到 [`docs/archive/COMPLETED_FULL.md`](docs/archive/COMPLETED_FULL.md)。
> - **历史 `Deferred` 清单需重评**。[`docs/WORK_STATUS.md`](docs/WORK_STATUS.md) §3 不再默认等于”不做”；触及这些条目时按上游完整实现对齐，除非另有书面确认。
> - **上游参考**：对照行为时读 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/`（TypeScript 原版 ）。
> - **如确需保留某项缩减**，在 PR 描述中显式说明，并在文档标注为“故意保留”（Intentional），而不是沉默继续按简化版写。


## Path Isolation (Critical)

allthecodes 和原版 Codex (TypeScript) 共存于同一台机器上，**所有持久化路径必须隔离**：

| 用途 | 原版 Codex | allthecodes (本项目) |
|------|-----------------|-----------------|
| 全局数据目录 | `~/.Codex/` | `~/.allthecodes/` |
| 项目配置 | `.Codex/settings.json` | `.allthecodes/settings.json` |
| 项目技能 | `.Codex/skills/` | `.allthecodes/skills/` |
| Keychain 服务名 | `"Codex"` | `"allthecodes"` |
| 项目指令文件 | `AGENTS.md` | `AGENTS.md` (共享) |

## Cargo / Build

Do not assume `cargo` is available from the default shell `PATH` on this
machine. The Rust toolchain for this project is installed under the workspace
parent directory:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```

Use the commands below from the repository root:

```bash
cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes

# Verify the local toolchain.
cargo --version
rustc --version
rustup show active-toolchain

# Build the whole workspace in release mode.
cargo build --workspace --release
```

The repository currently selects toolchain `1.91.1` via rustup. A separate
stable toolchain is also installed in the same local `.rust/` root, but builds
inside this repo should follow the repository-selected toolchain.

Known build warnings on this machine:

- `cc-browser/src/mcp_bridge.rs` currently has an unused `Context` import.
- `crates/allthecodes/src/tools/exec/process_control.rs` currently has an
  unused Unix `CommandExt` import.

## npm Release / Web UI Packaging

The npm release package is backend-only by default:

- Do not build or embed the sibling `../allthecodes-web` repository during npm
  packaging.
- Do not pass `--features web-ui` in the npm release workflow or staging script.
- `allthecodes --web` in npm builds starts the API/WS backend. The browser SPA
  must run from the standalone `allthecodes-web` repository and connect to this
  backend.

Full-platform npm releases must be produced by the GitHub Actions `release`
workflow from the release tag, because macOS, Windows, and Linux ARM artifacts
come from the workflow matrix. Local manual publishing is only appropriate for
quick linux-x64 validation or a deliberate linux-x64-only recovery.

For local linux-x64 validation, use:

```bash
cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes

# The Codex shell may not include cargo/rustc in PATH. If needed:
export PATH="/home/nzq/.rustup/toolchains/1.91.1-x86_64-unknown-linux-gnu/bin:$PATH"

rm -rf /tmp/allthecodes-npm-dist-0.1.5 /tmp/allthecodes-root-stage-0.1.5
mkdir -p /tmp/allthecodes-npm-dist-0.1.5

python3 scripts/stage_npm_packages.py \
  --release-version 0.1.5 \
  --package allthecodes-linux-x64 \
  --output-dir /tmp/allthecodes-npm-dist-0.1.5

python3 scripts/build_npm_package.py \
  --package allthecodes \
  --release-version 0.1.5 \
  --staging-dir /tmp/allthecodes-root-stage-0.1.5 \
  --pack-output /tmp/allthecodes-npm-dist-0.1.5/allthecodes-npm-0.1.5.tgz
```

Quick local install smoke test:

```bash
VERIFY=/tmp/allthecodes-npm-verify-0.1.5
OUT=/tmp/allthecodes-npm-dist-0.1.5
rm -rf "$VERIFY"
mkdir -p "$VERIFY"
cd "$VERIFY"
npm init -y >/dev/null
npm install --cache /tmp/npm-cache --ignore-scripts --no-audit --no-fund \
  "$OUT/allthecodes-npm-0.1.5.tgz" \
  "allthecodes-linux-x64@file:$OUT/allthecodes-npm-linux-x64-0.1.5.tgz"
node_modules/.bin/allthecodes --version
```

Publish ordering is platform packages first, then the root wrapper. The root
wrapper declares optional dependencies for every platform package, so publishing
the root before platform tarballs can make fresh installs fail.

The GitHub Actions publish job must skip versions that already exist on npm.
This is required for recovery cases where linux-x64 and/or the root wrapper were
published manually before the full matrix completed; the rerun should publish
only the missing platform versions.


提交本仓库时使用显式路径的手动流程，不要依赖仓库内脚本：

```bash
cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes

export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo build --workspace --release

git config user.name "Crsei"
git config user.email "Crsei@protonmail.com"

# 只暂存本任务明确涉及的路径。新增/修改文件用 git add -A；删除文件用 git add -u。
git add -A -- <changed-existing-paths...>
git add -u -- <deleted-paths...>

git commit -m "<short imperative summary>"
git push origin "$(git branch --show-current)"
```

提交注意事项：

- 本仓库当前远端是 `origin`，推送目标应是 `https://github.com/Crsei/allthecodes.git`，不要使用旧的 `claude-code-rust.git` 推送地址。
- 仓库根目录当前没有 `scripts/git-commit-update-and-push.sh`；不要调用其他仓库里的同名脚本来提交本仓库。
- 只暂存命令行显式传入的文件/目录，避免误提交共享 worktree 中无关修改。
- 如果修改包含“删除旧文件 + 新增目录模块”的重命名式拆分，先 `git add -A -- <new-directory>`，再 `git add -u -- <deleted-file>`。
- 如只需本地提交不推送，省略最后的 `git push`。

如果 `git push origin "$(git branch --show-current)"` 需要认证，可使用临时 `GIT_ASKPASS` 脚本读取
`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/github_token.txt`。不要读取、打印、提交或复制该 token 文件内容；脚本结束后必须删除临时文件：

```bash
set +x
ASKPASS_SCRIPT=$(mktemp)
cat > "$ASKPASS_SCRIPT" <<'EOF'
#!/bin/sh
case "$1" in
  *Username*) printf '%s\n' 'Crsei' ;;
  *Password*) cat /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/github_token.txt ;;
  *) printf '%s\n' 'Crsei' ;;
esac
EOF
chmod 700 "$ASKPASS_SCRIPT"
GIT_CONFIG_GLOBAL=/dev/null \
GIT_ASKPASS="$ASKPASS_SCRIPT" \
GIT_TERMINAL_PROMPT=0 \
git -c credential.helper= push https://github.com/Crsei/allthecodes.git "$(git branch --show-current)"
status=$?
rm -f "$ASKPASS_SCRIPT"
exit $status
```

`GIT_CONFIG_GLOBAL=/dev/null` is intentional here: this machine has a global
GitHub URL rewrite through `gh.llkk.cc`, and authenticated push should use the
canonical GitHub URL directly.

## Project Structure

```
rust/
├── src/                     Rust 后端
│   ├── main.rs              入口 (Phase A/B/I lifecycle, --headless flag)
│   ├── types/               核心类型
│   ├── engine/              QueryEngine + 系统提示词
│   │   └── lifecycle/       QueryEngine 生命周期 (mod, types, submit_message, deps, helpers)
│   ├── query/               异步流式查询循环 (loop_impl + loop_helpers)
│   ├── tools/               工具系统（含 Agent / LSP / Web / Brief / Sleep）
│   ├── skills/              技能系统 (内置 + 用户自定义)
│   ├── compact/             上下文压缩管道
│   ├── commands/            斜杠命令系统
│   ├── api/                 API 客户端 (Anthropic / OpenAI / Google / Azure / OpenAI Codex)
│   ├── auth/                认证 (API Key + Keychain + OAuth + Codex CLI fallback)
│   ├── permissions/         权限系统
│   ├── config/              配置管理
│   ├── session/             会话持久化
│   ├── ipc/                 IPC 协议 + headless 模式 (JSONL over stdio)
│   ├── daemon/              daemon + Team Memory 代理
│   ├── web/                 Web 模式静态资源与路由支持
│   ├── services/            tool_use_summary / session_memory / prompt_suggestion / lsp_lifecycle
│   ├── crates/allthecodes/src/ui/  Rust TUI (ratatui + crossterm)
│   ├── utils/               工具函数
│   └── shutdown.rs          优雅关闭
└── docs/
    ├── WORK_STATUS.md       当前完成度 / 未完成项总览
    ├── IMPLEMENTATION_GAPS.md  注意点 / 缩减实现 / 设计限制总入口
    ├── KNOWN_ISSUES.md      用户可感知问题跟踪
    └── archive/             已完成功能的历史设计 / 计划 / 变更记录
```

### IPC 架构

Rust TUI 通过 `--headless` 模式与 Rust 后端通信:
- Rust 端: `src/ipc/protocol.rs` (协议类型) + `src/ipc/headless.rs` (事件循环)
- 这里仅指 `crates/allthecodes/src/ui/` 中的 Rust TUI；不要再引入其他非 Rust TUI 的表述

### 已移除的模块 (完整版有)

analytics, remote

### 文档入口

- `docs/WORK_STATUS.md`：当前完成度、未完成项、延期范围
- `docs/IMPLEMENTATION_GAPS.md`：注意事项、缩减实现、设计限制统一入口
- `docs/KNOWN_ISSUES.md`：用户可感知问题，持续追加
- `docs/archive/`：已经落地功能的历史方案、设计、日报、变更记录

### Auth Flow

```
ApiClient::from_backend()
  ├─ native  → auth::resolve_auth()
  │            ├─ ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN
  │            └─ ~/.allthecodes/credentials.json / 系统 Keychain ("allthecodes", fallback "cc-rust")
  └─ codex   → auth::resolve_codex_auth_token()
               ├─ OPENAI_CODEX_AUTH_TOKEN
               ├─ ~/.allthecodes/credentials.json
               └─ ~/.codex/auth.json
```

### 注意事项

- 每次写完代码，编译过后查有没有 warning，解决 warning（必须保证未使用的都在代码中起作用），然后构建相应的 e2e test
- Rust TUI 已知问题记录在 `docs/KNOWN_ISSUES.md`，用户反馈的问题追加到该文件
- Codex backend 当前行为看 `docs/codex-backend.md`；历史调研笔记已归档到 `docs/archive/implemented/codex-agent.md`
- 注意目前阶段修改 UI 代码只修改 `crates/allthecodes/src/ui/` 端的代码
- Windows 环境下如果 `omx explore` 的只读 harness 不可用，直接用 PowerShell + `rg` 做等价只读定位，不要把它当成仓库问题
- 文档更新按任务拆分，每完成一个文档更新任务就单独 commit；commit 描述保持一句话，直接说明这次提交的目的即可
