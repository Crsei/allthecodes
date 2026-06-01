# Phase 1: 运行时存储路径统一设计

> 日期: 2026-04-18
> 状态: 已批准（待 user review 落盘版本）
> 对应 issue: [GitHub #1 Web UI overhaul](https://github.com/crsei/cc-rust/issues/1) 的 Phase 1
> 前置: 无
> 后续: Phase 2 (Session API + project/workspace 分组模型) 依赖本 PR 合并

## 一、背景与目标

cc-rust 当前把运行时持久化数据写到多种位置，问题有三：

1. **仓库污染**：主 tracing 日志在 `~/.cc-rust/logs/` 创建失败时会 fallback 到 repo 根目录的 `.logs/`；dashboard 的 subagent 事件日志**无条件**写到 `{cwd}/.logs/subagent-events.ndjson`（即使 home 可用）。
2. **`CC_RUST_HOME` env 覆盖不完整**：只有 settings 加载、主 logs、audit runs、plugins 走 `global_claude_dir()` 尊重该 env；其余 ~9 个模块（sessions、credentials、daemon memory log、exports、audits、transcripts、skills、team memory、session insights）直接硬编码 `dirs::home_dir().join(".cc-rust")`，绕过覆盖机制。
3. **home 解析逻辑重复**：~6 个文件各写一份 `dirs::home_dir()` 的 fallback，难以审计与演进。

**Phase 1 目标**：引入单一 `paths` 模块集中管理所有运行时路径，保证"设 `CC_RUST_HOME=X` 后所有数据都落在 X"、"永远不向仓库目录写入"。

## 二、范围边界

### 包含

- 新模块 `src/config/paths.rs`，承载所有运行时路径解析
- 迁移 ~9 处调用点到 `paths::` 函数
- Dashboard subagent-events 日志从 `{cwd}/.logs/` 迁到 `$ROOT/runs/{session_id}/`
- 主 tracing logger 移除 `.logs/` fallback，改为 temp 目录 fallback
- `paths::data_root()` 的三级 fallback 策略 + 一次性 warn
- 新增文档 `docs/STORAGE.md`
- 新单元测试 + 一条集成冒烟测试

### 不包含

| 不做 | 理由 |
|------|------|
| 老数据自动迁移 | 用户手动处理；降低本 PR 复杂度 |
| Phase 2-5（session 分组、sidebar、composer、markdown 增强） | 独立 PR |
| `.cc-rust/settings.json | memory | skills` 项目级路径调整 | 有意保留类 `.git/config` 语义 |
| Codex CLI `~/.codex/` 读取逻辑 | 外部工具约定，正交 |
| credentials.json 加密/权限 hardening | 独立安全议题 |
| Team memory `projects/{sanitized_cwd}/` 的 sanitizer 改进 | 保留现状 |
| 删除仓库里已有 `.logs/` 或旧 artifact | 仅代码防止未来产生 |
| 彻底删除 `global_claude_dir()` | 保留为 `paths::data_root()` 的兼容 alias |

## 三、架构

### 3.1 新模块 `src/config/paths.rs`

无状态纯函数集合。每次调用即时解析 env，便于测试。

**核心入口：**

```rust
pub fn data_root() -> PathBuf;
```

三级 fallback：

```
1. std::env::var("CC_RUST_HOME")  (trim 后非空) → PathBuf::from(val)
2. dirs::home_dir().join(".cc-rust")
3. std::env::temp_dir().join("cc-rust")  + tracing::warn! 一次
```

- Level 1 的 trim-非空判断沿用 [src/config/settings.rs:98-102](src/config/settings.rs) 现有写法：trim 只用于判空，实际 `PathBuf::from(val)` 使用未 trim 的原始字符串（保留首尾空白是合法路径字符，与既有语义一致）。
- Level 3 命中用 `std::sync::Once` 去重，消息：
  ```
  WARN: unable to resolve home directory; cc-rust data will be written to
        <path> (non-persistent). Set CC_RUST_HOME to override.
  ```
- **不返回 `Result`**：调用点假设 `data_root() -> PathBuf`。

**分区函数（全部内部 = `data_root().join(...)`)：**

```rust
pub fn sessions_dir() -> PathBuf;
pub fn logs_dir() -> PathBuf;
pub fn daily_log_path(date: chrono::DateTime<Local>) -> PathBuf;  // $ROOT/logs/YYYY/MM/YYYY-MM-DD.md
pub fn credentials_path() -> PathBuf;
pub fn runs_dir(session_id: &str) -> PathBuf;                     // $ROOT/runs/{session_id}
pub fn exports_dir() -> PathBuf;
pub fn audits_dir() -> PathBuf;
pub fn transcripts_dir() -> PathBuf;
pub fn memory_dir_global() -> PathBuf;
pub fn session_insights_dir() -> PathBuf;
pub fn plugins_dir() -> PathBuf;
pub fn skills_dir_global() -> PathBuf;
pub fn team_memory_dir(cwd: &Path) -> PathBuf;                    // $ROOT/projects/{sanitized}/memory/team
```

**项目级路径独立：**

```rust
pub fn project_cc_rust_dir(cwd: &Path) -> PathBuf;   // {cwd}/.cc-rust
```

调用方式：`project_cc_rust_dir(cwd).join("settings.json")` 等。

### 3.2 向后兼容

[src/config/settings.rs:96](src/config/settings.rs) 现有 `global_claude_dir() -> Result<PathBuf>` 改为 `paths::data_root()` 的薄 wrapper：

```rust
pub fn global_claude_dir() -> Result<PathBuf> {
    Ok(crate::config::paths::data_root())
}
```

保持签名不变，所有现有调用点继续工作。**后续重构**可以逐步把 `global_claude_dir()?` 调用直接换成 `paths::data_root()`，Phase 1 不强制。

## 四、完整路径清单（canonical）

`$ROOT = paths::data_root()`

| 用途 | 最终路径 | 迁移前 | 迁移后调用点 |
|------|---------|--------|-------------|
| 全局 settings | `$ROOT/settings.json` | ✓ 已正确 | [src/config/settings.rs:112](src/config/settings.rs) 保持 |
| 项目 settings | `{cwd}/.cc-rust/settings.json` | ✓ 已正确 | 保持 |
| Sessions | `$ROOT/sessions/` | env 不 honor | [src/session/storage.rs:66](src/session/storage.rs) → `paths::sessions_dir()` |
| 主 tracing logs | `$ROOT/logs/` | 有 `.logs/` fallback | [src/main.rs:249-265](src/main.rs) → `paths::logs_dir()`，**删除 `.logs/` fallback** |
| Daemon 每日 MD | `$ROOT/logs/YYYY/MM/YYYY-MM-DD.md` | env 不 honor | [src/daemon/memory_log.rs:9](src/daemon/memory_log.rs) → `paths::daily_log_path()` |
| **Subagent events** | **`$ROOT/runs/{session_id}/subagent-events.ndjson`** | **`{cwd}/.logs/subagent-events.ndjson`** | [src/dashboard.rs:168](src/dashboard.rs) — 行为变更，见 §5 |
| Credentials | `$ROOT/credentials.json` | env 不 honor | [src/auth/token.rs:8](src/auth/token.rs) → `paths::credentials_path()` |
| Audit runs | `$ROOT/runs/{session_id}/` | ✓ 已 honor env | [src/observability/sink.rs:122](src/observability/sink.rs) → `paths::runs_dir()` （可选清理：删除 `.unwrap_or_else` 死 fallback） |
| Exports | `$ROOT/exports/` | env 不 honor | [src/session/export.rs:97](src/session/export.rs) → `paths::exports_dir()` |
| Audits (JSON) | `$ROOT/audits/` | env 不 honor | [src/session/audit_export.rs:506](src/session/audit_export.rs) → `paths::audits_dir()` |
| Transcripts | `$ROOT/transcripts/` | env 不 honor | [src/session/transcript.rs:41](src/session/transcript.rs) → `paths::transcripts_dir()` |
| 全局 memory | `$ROOT/memory/` | ✓（via `global_claude_dir`） | [src/session/memdir.rs:57](src/session/memdir.rs) 可选清理 → `paths::memory_dir_global()` |
| 项目 memory | `{cwd}/.cc-rust/memory/` | ✓ | 保持 |
| Session insights | `$ROOT/session-insights/` | 自制 HOME/USERPROFILE fallback | [src/services/session_memory.rs:35](src/services/session_memory.rs) → `paths::session_insights_dir()` |
| Plugins | `$ROOT/plugins/` | ✓ 已 honor env | [src/plugins/mod.rs:137](src/plugins/mod.rs) 可选清理 → `paths::plugins_dir()`（删除 `.unwrap_or_else` 死 fallback） |
| 全局 skills | `$ROOT/skills/` | env 不 honor | [src/skills/mod.rs:235](src/skills/mod.rs) → `paths::skills_dir_global()` |
| 项目 skills | `{cwd}/.cc-rust/skills/` | ✓ | 保持 |
| Team memory | `$ROOT/projects/{sanitized_cwd}/memory/team/` | env 不 honor | [src/daemon/team_memory_proxy.rs:54](src/daemon/team_memory_proxy.rs) → `paths::team_memory_dir(cwd)` |
| Codex CLI creds | `$CODEX_HOME/auth.json` 或 `~/.codex/auth.json` | 无关（外部） | 不动 |

**创建目录责任**：`paths::` 函数只返回路径，不 `mkdir`。创建仍由调用点负责（它们已在调 `create_dir_all`），保留现状。

## 五、Dashboard subagent-events 行为变更

唯一有行为变更的模块，单独成节。

### 现状

[src/dashboard.rs:168-178](src/dashboard.rs)：

```rust
fn event_log_path_for_base(base: &Path) -> PathBuf {
    base.join(".logs").join("subagent-events.ndjson")
}
// base = original_cwd() → 写到 {cwd}/.logs/subagent-events.ndjson
```

### 目标

`$ROOT/runs/{session_id}/subagent-events.ndjson`，与 [src/observability/sink.rs](src/observability/sink.rs) 的 `runs/{session_id}/events.ndjson` 并列，语义是"这个 session 的所有观测信号"。

### 改造

1. Dashboard 初始化时接收 `session_id: String` 参数，存到 dashboard state
2. `event_log_path()` 不再接 `base`，改为 `paths::runs_dir(&self.session_id).join("subagent-events.ndjson")`
3. 如果 session 建立前接到事件：优先 `drop` + `tracing::debug!`，避免引入 buffer 复杂度

### 风险与退路

⚠️ **实现 plan 第一步要确认 dashboard 初始化点能否拿到稳定的 session_id**。

- 若注入代价小：按上方方案推进
- 若 dashboard 生命周期早于 session 建立：两个选项
  - **选项 A**：delayed activation — dashboard 建立后不立即开文件，等收到 session_id 事件再打开日志文件
  - **选项 B**：退化为 per-workspace `$ROOT/projects/{sanitized_cwd}/subagent-events.ndjson`，与 team_memory_proxy 一致，不依赖 session_id

选项 B 是兜底方案。如果实现阶段需要切换，会回 design 文档 sync 后再继续。

### 不破坏点

Dashboard 事件格式、订阅者、消费方的读取路径（如有）都不动。

## 六、Fallback 策略与去重

见 §3.1。关键要点：

- `data_root()` 不返回 `Result`，永远能给出一个可写 `PathBuf`
- Level 3（temp fallback）每进程只 warn 一次（`std::sync::Once`）
- `CC_RUST_HOME=""` 或全空白被视为"未设置"

## 七、测试策略

### 7.1 `paths.rs` 单元测试（新增）

位置：`src/config/paths.rs` 的 `#[cfg(test)] mod tests`。全部用 `#[serial]`（`serial_test` crate）避免 env 污染。

- `test_data_root_uses_env_override` — `CC_RUST_HOME=/tmp/abc` → 返回 `/tmp/abc`
- `test_data_root_ignores_empty_env` — `CC_RUST_HOME=""` → 回落到 home
- `test_data_root_ignores_whitespace_env` — `CC_RUST_HOME="  "` → 回落到 home
- `test_data_root_temp_fallback` — 清空 `CC_RUST_HOME` + 清空 `HOME` / `USERPROFILE`。**注意**：`dirs::home_dir()` 在 Linux 上可能通过 `getpwuid_r` 仍返回 `/home/<user>`，在 Windows 上走 `SHGetKnownFolderPath` 完全不理会 env，所以 env-only 手段无法保证触发 temp fallback。此测试标记为 best-effort：若检测到 home 仍可解析则 `println!("skipped: host returns home via OS APIs")` 并退出；真正的 temp fallback 路径主要由 §7.5 手动验收覆盖。
- 每个分区函数一条：`sessions_dir()` 在 `CC_RUST_HOME=/tmp/x` 下返回 `/tmp/x/sessions`（冒烟级，确认都走 root）

### 7.2 集成冒烟测试（新增）

位置：`tests/cc_rust_home_isolation.rs`。

流程：
1. 创建 tempdir
2. 设 `CC_RUST_HOME=<tempdir>`
3. in-process 触发若干路径函数 + 一次 session save + 一次 logs 初始化
4. 断言：tempdir 下出现对应子目录；**仓库工作目录下无任何新建文件**（用 `git status` 断言）

带 `#[serial]`。

### 7.3 依赖引入

`Cargo.toml` `[dev-dependencies]` 新增：
```toml
serial_test = "3"
```

### 7.4 不做的

- 不改造 ~9 个被迁移模块的既有测试。它们已用 `tempfile` 隔离，测的是业务逻辑。
- 不做压力 / 并发测试（路径解析是 pure 函数，无竞态风险）。

### 7.5 手动验收步骤（PR 描述里给出）

- 清空 `~/.cc-rust/`（由测试者手工）
- `cargo run --release -- --headless` 发一条 ping → 检查 `~/.cc-rust/` 出现预期目录，工作目录无新产物
- `CC_RUST_HOME=/tmp/cc-rust-test cargo run --release -- --headless` → 同上但数据落在 `/tmp/cc-rust-test`
- macOS/Linux：`HOME= USERPROFILE= unset CC_RUST_HOME; cargo run` → stderr 出现 warn，数据落在 temp

## 八、文档交付

### 8.1 新增 `docs/STORAGE.md`

面向用户的持久化路径参考。包含：

- 根目录规则（三级 fallback）
- 完整路径表（§4 的简化用户版：去掉"迁移前/后"列，加"用于什么"）
- 项目级 vs 全局级区分
- 跨平台 home / temp 解析说明
- 老数据迁移指引（手动 mv）

### 8.2 更新现有文档

- `README.md` — 在相关段落加一行指向 `docs/STORAGE.md`
- `CLAUDE.md` — "Path Isolation" 段落加 `CC_RUST_HOME` 一行说明与指针

## 九、`.gitignore` 现状确认

审计 [.gitignore](.gitignore) 结果：

- 行 32 `.logs/` — 已覆盖任意深度的 `.logs/` 目录（包括 repo 根）
- 行 59 `.cc-rust` — 已覆盖任意深度的 `.cc-rust/` 目录（注意：此规则副作用是**阻止**用户 commit 项目级 `.cc-rust/settings.json`，但这是另一个议题，不在本 PR 范围）
- 行 27-31 `logs/**/*.log` etc. — 针对 E2E 测试产物；`logs/.gitkeep` 是有意保留的

**结论：既有 gitignore 已足够，本 PR 不新增条目**。防御性保障由代码侧改动（删除 `.logs/` fallback、移除 `{cwd}/.logs/` 硬编码）实现。

## 十、提交切片建议（给 writing-plans）

概要次序（具体由实现 plan 细化）：

1. 新建 `src/config/paths.rs` + 所有分区函数 + 单元测试（可独立 land）
2. `global_claude_dir()` 改为 `paths::data_root()` wrapper
3. 逐模块迁移（每个模块独立 commit，便于 review / revert）：
   - a. `main.rs` 主 logger（删除 `.logs/` fallback）
   - b. `session/storage.rs`、`session/export.rs`、`session/transcript.rs`、`session/audit_export.rs`
   - c. `auth/token.rs`
   - d. `daemon/memory_log.rs`、`daemon/team_memory_proxy.rs`
   - e. `services/session_memory.rs`
   - f. `skills/mod.rs`
   - g. `dashboard.rs`（最复杂，最后做；Plan 第一步先验证 session_id 注入可行性）
   - h. 可选清理 commit：`session/memdir.rs`、`plugins/mod.rs`、`observability/sink.rs` 改走 `paths::`（删除冗余 `.unwrap_or_else` 死 fallback；行为不变）
4. 新增集成冒烟测试 `tests/cc_rust_home_isolation.rs`
5. 文档：`docs/STORAGE.md` + README/CLAUDE.md 更新

## 十一、验收标准

- [ ] `src/config/paths.rs` 存在，所有分区函数实现并通过单元测试
- [ ] 所有列入 §4 表"迁移后调用点"的文件完成迁移
- [ ] Dashboard subagent events 写入 `$ROOT/runs/{session_id}/subagent-events.ndjson`
- [ ] `main.rs` 中 `.logs/` fallback 已删除
- [ ] `data_root()` 支持 `CC_RUST_HOME` 覆盖并在无 home 时回落到 temp（带 warn）
- [ ] 集成冒烟测试通过
- [ ] `docs/STORAGE.md` 交付，README/CLAUDE.md 更新
- [ ] `cargo build --release` 无 warning
- [ ] 手动验收步骤走通（PR 描述含结果）
