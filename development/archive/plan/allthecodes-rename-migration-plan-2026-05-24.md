# allthecodes 全量改名迁移计划

日期：2026-05-24

目标：把本项目中作为本产品身份出现的 `Claude Code`、`claude-code`、`claude-code-rs`、`cc-rust`、`cc-*`、`cc_`、`.cc-rust`、`CC_RUST_*`、`CLAUDE_CODE_*` 迁移到 `allthecodes` 命名体系；把运行时项目指令文件从 `CLAUDE.md` 迁移到 `AGENTS.md`；覆盖项目目录、Cargo package/crate、函数名、配置文件、脚本、测试、SDK、TUI 展示和文档。

本计划由 3 个 subagent 并行做只读扫描后汇总：

- 代码/配置扫描：Cargo package、workspace crate、函数/API、路径、env、Keychain、SDK。
- 文档/脚本/测试扫描：README、AGENTS/CLAUDE、CI、Docker、message suite、快照。
- TUI 扫描：欢迎页、输入占位、登录面板、memory/mcp/hooks/permissions、快照。

## 1. 命名目标

统一采用小写产品名 `allthecodes`。需要标题化显示时使用 `allthecodes`，不引入 `AllTheCodes`，避免同一产品名多种写法。

| 当前命名 | 目标命名 | 适用范围 |
|---|---|---|
| `claude-code-rust` | `allthecodes` | 仓库目录、脚本路径、文档路径示例 |
| `crates/claude-code-rs` | `crates/allthecodes` | 主二进制 crate 目录 |
| package/bin `claude-code-rs` | `allthecodes` | Cargo package、二进制、Docker entrypoint、测试 binary lookup |
| `cc-rust` | `allthecodes` | 产品文案、Keychain service、路径说明、TUI 展示 |
| `cc-*` crate | `allthecodes-*` | workspace 内部 crate package 名 |
| Rust import `cc_config` | `allthecodes_config` | Rust crate import/module path |
| `.cc-rust` | `.allthecodes` | 项目级配置、skills、agents、plan、memory |
| `~/.cc-rust` | `~/.allthecodes` | 全局数据根 |
| `CC_RUST_*` | `ALLTHECODES_*` | 项目自有环境变量 |
| `CLAUDE_CODE_*` | `ALLTHECODES_*` | 项目自有 TUI/team/SDK 环境变量 |
| `CLAUDE.md` | `AGENTS.md` | 项目指令文件、memory、init、扫描器 |

## 2. 不能机械替换的保留项

这些不是本项目品牌，不应被全局替换：

- Anthropic 模型名：`claude-opus-*`、`claude-sonnet-*`、`claude-haiku-*`。
- Anthropic 服务域名：`claude.ai`、`platform.claude.com`。
- Anthropic 协议 beta header：`claude-code-20250219`。除非确认服务端协议已改名，否则只改常量注释，不改 header 值。
- 第三方 Cargo crate `cc`，例如 `Cargo.lock` 里的 `name = "cc"`。
- C/C++ 文件扩展名、`.cc` 语法标识、普通英文里的 `cc` 缩写。
- `OpenAI Codex`、`codex` backend、`~/.codex/auth.json`。这是第三方/兼容后端名，不属于 allthecodes 品牌。
- 历史上游对照路径 `claude-code-bun`。文档中作为上游 TypeScript 原版引用时可以保留，但必须标注为 upstream reference，不作为本项目名。

## 3. 兼容策略

改名后默认写入新位置，但至少一个版本内兼容读取旧位置。

路径解析顺序：

1. `ALLTHECODES_HOME` 非空时使用该路径。
2. 旧 `CC_RUST_HOME` 非空时读取并提示迁移。
3. `~/.allthecodes` 存在时使用它。
4. 旧 `~/.cc-rust` 存在且新目录不存在时读取旧目录并提示迁移。
5. 否则创建 `~/.allthecodes`。

项目路径解析顺序：

1. `.allthecodes/settings.json`、`.allthecodes/skills`、`.allthecodes/agents`。
2. 旧 `.cc-rust/*` 作为只读 fallback；任何新写入都写到 `.allthecodes/*`。

Keychain 解析顺序：

1. service `allthecodes`。
2. service `cc-rust` fallback。
3. 成功读取旧 service 后写入新 service；删除旧 entry 需要单独迁移命令或用户确认。

指令文件解析顺序：

1. `AGENTS.md` 优先。
2. `CLAUDE.md` fallback。
3. 同一目录同时存在时只读取 `AGENTS.md`，并在 `/memory path` 中显示 `CLAUDE.md ignored because AGENTS.md exists`。
4. `/init`、`/memory edit`、TUI memory selector 只创建/写入 `AGENTS.md`。
5. 仓库根目录最终删除 `CLAUDE.md`，保留 `AGENTS.md`。

环境变量兼容：

- 新变量统一为 `ALLTHECODES_*`。
- 旧 `CC_RUST_*` 和项目自有 `CLAUDE_CODE_*` 作为 fallback。
- help 文案和 docs 只展示新变量。
- 测试覆盖“新变量优先于旧变量”。

## 4. 分阶段执行

### Phase 0：冻结基线与残留清单

目标：形成可验证的改名前基线，不开始修改行为。

任务：

- 记录当前脏工作区，避免覆盖用户已有变更。
- 运行命中扫描并保存结果到临时文件：

```bash
rg -n --hidden -S "claude code|Claude Code|claude-code|claude-code-rs|cc-rust|\\.cc-rust|CC_RUST|CLAUDE_CODE|CLAUDE\\.md|\\bcc-[A-Za-z0-9_-]+|\\bcc_[A-Za-z0-9_]+" -g '!target/**' -g '!.git/**'
```

- 建立允许残留白名单：模型名、Anthropic header、上游参考路径、第三方 `cc` crate、Codex 兼容名。

验证：

- `git status --short`
- `cargo metadata --no-deps`

### Phase 1：`AGENTS.md` 运行时迁移

目标：先完成 `CLAUDE.md -> AGENTS.md`，避免后续路径/品牌改名时重复修改 memory 链路。

主要文件：

- `crates/cc-config/src/claude_md.rs`：重命名为 `agents_md.rs` 或新增 wrapper，函数从 `find_claude_md_files` 迁移为 `find_agents_md_files`，旧函数可暂留 deprecated alias。
- `crates/cc-config/src/lib.rs`：导出新模块名。
- `crates/cc-commands/src/init.rs`：模板改为 `# AGENTS.md`，创建 `AGENTS.md`。
- `crates/cc-commands/src/memory.rs`、`crates/cc-commands/src/memory/claude_md.rs`：展示、路径、编辑目标改为 `AGENTS.md`。
- `crates/claude-code-rs/src/ui/memory/**`、`command_surface/adapters/memory.rs`、`command_surface/surfaces/memory.rs`：TUI memory 文案和路径改为 `AGENTS.md`。
- `sync-docs.sh`：不再同步 `CLAUDE.md`。
- 仓库根目录：删除 `CLAUDE.md`，保留并更新 `AGENTS.md`。

测试：

```bash
cargo test -p cc-config agents_md claude_md
cargo test -p cc-commands init memory
cargo test -p claude-code-rs ui::memory ui::command_surface
```

验收：

- 新项目 `/init` 只创建 `AGENTS.md`。
- 只有 `CLAUDE.md` 的旧项目仍能读取项目指令。
- 同目录同时存在时 `AGENTS.md` 优先。

### Phase 2：路径、配置、Keychain、env 兼容层

目标：把持久化位置改到 allthecodes，同时不丢旧用户数据。

主要文件：

- `crates/cc-config/src/paths.rs`
  - `data_root()` 支持 `ALLTHECODES_HOME`。
  - 默认目录改为 `~/.allthecodes`。
  - fallback 读取 `CC_RUST_HOME` 和 `~/.cc-rust`。
  - `project_cc_rust_dir` 改为 `project_allthecodes_dir`，保留 deprecated alias。
  - `.cc-rust` 改为 `.allthecodes`，旧目录 fallback。
- `crates/cc-config/src/settings/**`、`runtime_settings.rs`、`mdm/**`
  - `CC_RUST_MANAGED_SETTINGS` -> `ALLTHECODES_MANAGED_SETTINGS`。
  - `CC_RUST_ENFORCE_POLICY` -> `ALLTHECODES_ENFORCE_POLICY`。
  - schema `$id` 改为 `https://allthecodes/settings.schema.json`。
- `crates/cc-auth/src/api_key.rs`
  - Keychain service 改为 `allthecodes`，旧 `cc-rust` fallback。
- `crates/cc-auth/**`
  - credential 文案和测试路径改为 `~/.allthecodes/credentials.json`。
- `crates/cc-session/**`、`cc-skills/**`、`cc-plugins/**`、`cc-teams/**`、`cc-daemon/**`
  - 所有通过 path helper 的路径跟随迁移；硬编码 `.cc-rust` 的位置改为 helper。

测试：

```bash
cargo test -p cc-config paths settings mdm
cargo test -p cc-auth
cargo test -p cc-session
cargo test -p cc-plugins
cargo test -p cc-skills
```

验收：

- 新环境创建 `~/.allthecodes` 和 `.allthecodes`。
- 旧 `~/.cc-rust` 可以读取。
- 新旧 env 同时存在时 `ALLTHECODES_HOME` 优先。
- 不再新增写入 `.cc-rust`。

### Phase 3：Cargo package、crate、目录、函数/API 改名

目标：完成工程结构层改名。

主二进制：

- `crates/claude-code-rs` -> `crates/allthecodes`。
- `crates/claude-code-rs/Cargo.toml` package `name = "allthecodes"`。
- `main.rs` version/log 输出 `allthecodes`。
- 测试中的 `cargo_bin("claude-code-rs")` -> `cargo_bin("allthecodes")`。
- `CARGO_BIN_EXE_claude-code-rs` -> `CARGO_BIN_EXE_allthecodes`。
- Docker entrypoint、CI image、message suite 默认 binary 改为 `allthecodes`。

Workspace crate：

- `crates/cc-config` -> `crates/config`，package `config`，import `config`。
- `crates/cc-auth` -> `crates/auth`，import `auth`。
- 对所有 `crates/cc-*` 逐一执行同样规则。
- 根 `Cargo.toml` workspace dependencies 从 `cc-*` 改为 `allthecodes-*`。
- Rust 函数/type 中明确产品名的 `cc_rust`、`CcRust` 改为 `allthecodes`、`Allthecodes`。
- 不替换泛用 `cc` 或第三方 crate `cc`。

建议执行顺序：

1. 先改主二进制 crate。
2. 再改基础库：config、types、auth、utils、models、keybindings。
3. 再改运行时：api、permissions、sandbox、session、skills、mcp、tools、engine、query。
4. 最后改 UI/daemon/tests。

测试：

```bash
cargo metadata --no-deps
cargo fmt --all --check
cargo build --workspace --release
```

验收：

- `target/release/allthecodes --version` 输出 `allthecodes <version>`。
- `cargo test -p allthecodes` 可运行。
- `rg -n "claude-code-rs|cc-rust|\\bcc_[A-Za-z0-9_]+|\\bcc-[A-Za-z0-9_-]+" crates Cargo.toml` 只剩允许残留。

### Phase 4：CLI 可见入口与隐藏内部入口

目标：用户可见的产品身份使用 `allthecodes`，但登录协议入口保持兼容稳定；项目内 SDK、脚本和 CI 作为隐藏内部项，不作为公开品牌迁移范围。

CLI/login：

- `/login claude-code` 不改名，继续作为 Anthropic-compatible 登录入口。
- auth profile id `claude_code` 不改名，避免破坏已有 settings、credentials 和 profile selection。
- TUI 登录标题、help 文案、command palette metadata 可以展示产品名 `allthecodes`，但命令示例仍保留 `/login claude-code`。
- 不新增 `/login allthecodes`，除非后续单独设计 profile alias 和迁移测试。

隐藏内部项：

- SDK package/module/API 不在本阶段公开改名；只在 binary/package 改名导致 discovery 失效时做内部兼容更新。
- 脚本和 CI 不作为用户文档里的品牌入口展示；只同步必要的二进制名、路径和测试命令，保证构建、测试、发布链路可运行。
- `.github/**`、`Dockerfile.ci`、`docker-compose.test.yml`、`scripts/**`、`tests/message_suite/**` 的改动以内部可运行为准，不要求清除所有历史命名注释。

测试：

```bash
cargo test -p allthecodes --test e2e_cli
cargo test -p allthecodes --test pty_ui
```

验收：

- Docs 和 help 中不再推荐 `claude-code-rs`、`cc-rust`、`.cc-rust`、`CLAUDE.md`。
- Docs 和 help 继续推荐 `/login claude-code` 作为登录命令。
- SDK、脚本、CI 不出现在面向用户的品牌迁移清单中。
- 旧 binary/env aliases 有测试覆盖；`claude_code` profile 作为稳定兼容名保留。

### Phase 5：TUI 展示与快照

目标：用户可见 TUI 只展示 allthecodes 和 AGENTS.md。

主要文件：

- `crates/allthecodes/src/ui/components/welcome.rs`：欢迎页 `cc-rust` -> `allthecodes`。
- `ui/app/render.rs`、`ui/prompt_input.rs`：`Message cc-rust` -> `Message allthecodes`。
- `ui/permissions/bypass_permissions_mode_dialog.rs`：权限警告品牌名。
- `ui/tui/export.rs`、`ui/app.rs`：debug/export 文件名和标题。
- `ui/memory/**`、`ui/mcp/**`、`ui/hooks/**`、`ui/agents/**`：`.allthecodes`、`AGENTS.md` 展示。

快照更新策略：

- 先改源代码和测试断言，再运行 targeted tests 生成 `.snap.new`。
- 人工审阅 `.snap.new`，确认只有品牌/路径/指令文件名变化。
- 接受快照后再做残留扫描。
- snapshot 文件名和 header 中的旧 crate 路径只在 Phase 3 crate 目录改名后由测试自然刷新；不要提前手改。

测试：

```bash
cargo test -p allthecodes ui::components ui::app ui::command_surface ui::command_palette ui::permissions ui::hooks ui::mcp ui::memory ui::runtime
cargo test -p allthecodes --test pty_ui
```

验收：

- TUI 欢迎页、输入框、命令面板、login、memory、mcp、hooks、permissions 不出现旧品牌。
- 快照没有未审阅 `.snap.new`。

### Phase 6：文档、历史材料、最终残留清理

目标：当前文档、用户文档和开发文档全部改为 allthecodes；历史上游对照只保留明确说明。

优先改：

- `README.md`
- `AGENTS.md`
- `docs/README.md`
- `development/archive/USAGE_GUIDE.md`
- `development/archive/CLI_REFERENCE.md`
- `development/archive/COMMAND_REFERENCE.md`
- `development/archive/COMMAND_UI_REFERENCE.md`
- `docs/claude-code-configuration/**` 目录重命名为 `docs/allthecodes-configuration/**`
- `architecture/**` 当前架构文档
- `docs/schemas/**`

最后处理：

- `development/archive/**`
- `docs/tmp/**`
- 历史计划和调研材料

验收扫描：

```bash
rg -n --hidden -S "claude code|Claude Code|claude-code|claude-code-rs|cc-rust|\\.cc-rust|CC_RUST|CLAUDE_CODE|CLAUDE\\.md|\\bcc-[A-Za-z0-9_-]+|\\bcc_[A-Za-z0-9_]+" -g '!target/**' -g '!.git/**'
```

每个残留必须属于白名单，并在最终 PR/commit 说明中列出。

## 5. Subagent 并行分工

迁移执行时建议启动 6 个 worker，并明确文件所有权，避免互相覆盖。

| Subagent | 所有权 | 输出 |
|---|---|---|
| A：paths/env/auth | `crates/allthecodes-config`、`crates/allthecodes-auth`、settings schema、路径测试 | 新旧路径/env/keychain 兼容层 |
| B：AGENTS memory | `agents_md` 模块、`init`、`memory`、project instruction tests | `AGENTS.md` 优先、`CLAUDE.md` fallback |
| C：Cargo/crate rename | `Cargo.toml`、`crates/*/Cargo.toml`、import 路径、主 binary | workspace 可 build |
| D：TUI | `crates/allthecodes/src/ui/**`、TUI tests/snapshots | 用户可见品牌和路径更新 |
| E：hidden tests/CI/scripts | `.github`、Docker、scripts、message suite、e2e/pty tests | 内部测试入口和 binary lookup 更新，不作为公开品牌入口 |
| F：docs/hidden SDK | README、docs、architecture、SDK package/API | 用户文档更新；SDK 仅做内部兼容说明 |

协调规则：

- 所有 worker 都不能还原别人改动。
- 每个 worker 只改自己拥有的路径。
- 每阶段合并前由主 agent 运行残留扫描和 targeted tests。
- Phase 3 之后所有 subagent 必须使用新 crate 路径 `crates/allthecodes`。

## 6. 风险与回滚点

高风险：

- Workspace crate 全量改名会造成大量 import 编译错误。
- 路径迁移可能让已有 credentials/settings/session 暂时不可见。
- Keychain service 改名可能导致登录状态丢失。
- `CLAUDE.md` 改名会影响旧项目指令加载。
- TUI snapshot 会出现大批变化，需要分批审阅。

回滚点：

- Phase 1 完成后可独立发布：只改变 project instruction 新写入目标，并兼容旧文件。
- Phase 2 完成后可独立发布：新路径可用，旧路径 fallback。
- Phase 3 是最大破坏性阶段，必须在独立分支完成并一次性修到 `cargo build --workspace --release` 通过。

## 7. 最终验收矩阵

必须通过：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo fmt --all --check
cargo build --workspace --release
cargo test --workspace
cargo test -p allthecodes --test pty_ui
cargo test -p allthecodes --test e2e_cli
```

残留扫描必须只剩白名单：

```bash
rg -n --hidden -S "claude code|Claude Code|claude-code|claude-code-rs|cc-rust|\\.cc-rust|CC_RUST|CLAUDE_CODE|CLAUDE\\.md|\\bcc-[A-Za-z0-9_-]+|\\bcc_[A-Za-z0-9_]+" -g '!target/**' -g '!.git/**'
```

人工验收：

- 新用户首次运行只创建 `~/.allthecodes`。
- 新项目 `/init` 只创建 `AGENTS.md` 和 `.allthecodes/settings.json`。
- 旧项目只有 `CLAUDE.md` 时仍加载指令。
- TUI 第一屏、输入框、命令面板、login、memory、mcp、hooks、permissions 全部展示 `allthecodes`。
- `allthecodes --version` 输出新 binary 名。
- Docker/CI/message suite 不再依赖 `claude-code-rs` binary。
