# PTY TUI E2E 测试时效优化计划

> **本文件是计划文件**。按 `development/workflow/2026-07-16-per-session-worktree-workflow-plan.md` §3-1，计划文件必须先单独 commit 到主分支 `allthecodes`，本文件本身不进 worktree 改。后续按本计划落地代码 / 文档 / artifact 时，再开 `worktree/pty-e2e-timing` 完成实际改动。

生效日期：2026-07-17
作用范围：`crates/allthecodes/tests/pty_tui_e2e/` 全部测试、`crates/allthecodes/logs/`（运行产物）、`development/test/README.md`、`development/test/pty-tui-e2e-timing-plan.md`（索引登记）。可选：`.config/nextest.toml`、`scripts/cargo-build-test.sh`。

## 1. 背景：为什么耗时 1930s

实测耗时：**215 passed / 36 ignored / 0 failed，1930.03s**。

逐项拆解后，时长来自**可叠加的三个结构性因素**，而不是单点故障：

### 1.1 全局串行锁（决定性因素）

`crates/allthecodes/tests/pty_tui_e2e/harness.rs` 在 `PtySession::spawn` 内持有进程级 `static Mutex<()>` 直到 `finish_in_dir` 退出：

```rust
fn pty_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
```

`PtySession` 把 `_serial_guard: MutexGuard<'static, ()>` 存为结构体字段，锁生命周期与会话等长——测试一开始就 acquire，直到 `finish_in_dir`（写日志、回收 reader 线程、`cleanup_detached_workspace_processes`）返回后才 drop。

含义：
- 即便 `cargo test` 默认按线程并发调度（或后续切到 nextest），**任意时刻最多只有 1 个 PTY 测试在跑**。
- 215 个 passed 测试 = 215 次串行 spawn + 渲染 + 断言 + 收尾。
- 1930s / 215 ≈ **8.98s/test 平均墙钟**，与每个测试的固定 sleep 总量（见下）量级吻合，说明串行是主要瓶颈，而非单测本身慢到拖垮整体。

### 1.2 大量硬编码 `Wait(Duration::from_secs(N))` 等待

`TestStep::Wait` 是**固定墙钟等待**，不依赖屏幕/输出信号，无论 TUI 是否已渲染完成都睡满。全量统计 `Wait(Duration::from_secs(N))` 出现次数：

| N（秒） | 出现次数 |
|---------|---------|
| 2 | 338 |
| 1 | 40 |
| 3 | 38 |
| 5 | 13 |
| 10 | 8 |
| 30 | 5 |
| 120 | 4 |
| 60 | 2 |
| 300 | 2 |
| 90 / 180 / 15 / 12 / 4 | 各 1 |

固定等待累计 ≈ **2700s ≈ 45min** 的"睡死"预算上限（按上限估，部分 `Wait(2s)` 复用于多步测试的内联 sleep）。其中 4×120s + 5×30s + 2×300s + 2×60s + 1×90 + 1×180 = **1380s** 集中在少数大测试（`test4_task_execution` / `test5_compact` / `running_task_slash_commands` / `model_flow` 在线测试），是离线套件外的主导项。即便并发行不通，仅把"等满 N 秒"换成"等到可视信号即返回"就足以把离线部分从 ~1500s 压到几百秒。

按文件分布（`Wait(secs)` 计数 Top）：

| 文件 | Wait 次数 |
|------|----------|
| `commands_core_info.rs` | 59 |
| `commands_mcp_plugin.rs` | 46 |
| `commands_aliases.rs` | 45 |
| `commands_permissions.rs` | 43 |
| `commands_kairos.rs` | 43 |
| `commands_memory_skills_hooks.rs` | 34 |
| `commands_agent_team.rs` | 32 |
| `commands_session.rs` | 22 |
| 其余 ≤ 18 | — |

离线小测试几乎都是 `SkipTrustGate + Wait(2s) + Command + Wait(2s) + Assert` 这个模板，固定 4s/command，~215 个离线测试 ≈ 860s 纯睡觉，与实测墙钟相当接近。

### 1.3 进程开销 + 收尾确定性花费

每个测试 spawn 一次 `allthecodes` 二进制；`binary_path()` 解析后 `portable_pty::openpty` + spawn，进程冷启动 + TUI 初始化 + trust gate 渲染在慢盘 / 共享 CI 上常见的 0.5–1.5s。`finish_in_dir` 末尾固定消耗：
- `std::thread::sleep(Duration::from_millis(200))` 收尾；
- reader 线程 `join` 最多等 500ms 才 detach；
- `cleanup_detached_workspace_processes()` 扫描 `/proc`、按 `workspace` + `mcp-cli-daemon`/`mcp-cli-bridge` 关键字匹配残留进程并对齐 `SIGTERM` → 等 `KILL_REAP_TIMEOUT=2s` → 必要时 `SIGKILL`。

215 次串行收尾，`/proc` 全表扫描 × 215，加上零星 1–2s 的 reap 等待，是大批量离线测试下额外的 ~1–2s/test 隐藏成本。

### 1.4 36 个 `#[ignore]` 在线测试

`README.md` 明示在线测试需真实 API key、`API_TIMEOUT=60s`，单轮对话耗时秒级到上限 60s 不等。这些测试在标准 `cargo test --test pty_tui_e2e`（不带 `--ignored`）下默认跳过，**不计入 1930s**。本计划聚焦离线套件墙钟；在线测试只做"不在本批被反复一起拉起"的隔离，不改其语义。

## 2. 目标

| 阶段 | 目标 | 验收阈值 |
|------|------|----------|
| Phase 0 | 度量基线 + 失败测试暴露问题 | 单跑离线套件，记录耗时 + 每测试耗时分桶；新增 `Wait`/`WaitForAny` 信号化验收用例 |
| Phase 1 | 把固定 `Wait(N)` 替换为信号化等待（`WaitForScreenText` / `WaitForText` / `WaitForAny` 带短 timeout） | 离线套件墙钟 ≤ 1200s（降幅 ≥ 38%） |
| Phase 2 | 有界并发：lift 全局串行锁，按"逻辑不冲突分组"并行 | 离线套件墙钟 ≤ 600s（较 Phase 1 再降 ≥ 50%） |
| Phase 3 | 进程生命周期收尾收窄 + `/proc` 扫描降频 | 收尾非离线套件剩余时长的 ≥ 40% |

非目标（Non-Goals）见 §7。

## 3. 测试层级与位置

| 层级 | 位置 | 本计划影响 |
|------|------|-----------|
| L1 Unit | `crates/allthecodes/` 内 `#[cfg(test)]` | 不改 |
| L2 Integration | `crates/allthecodes/tests/` | 本计划聚焦 `pty_tui_e2e/`（L3 也有交叉） |
| L3 E2E（PTY） | `crates/allthecodes/tests/pty_tui_e2e/` | 主要改动面 |
| L4 Golden | `tests/fixtures/` | 不改 |

## 4. 文件清单

| 文件 | 类型 | 用途 |
|------|------|------|
| `crates/allthecodes/tests/pty_tui_e2e/harness.rs` | Modify | lift 串行锁、收紧 `WAIT` 常量、新增信号化 helper、收尾收窄 |
| `crates/allthecodes/tests/pty_tui_e2e/script.rs` | Modify | `TestStep::Wait` 改为可选触发"短固定 + 信号"，新增 `WaitForScreenText` 默认实现复用 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_core_info.rs` | Modify | 替换 `Wait(2s)` → `WaitForScreenText` / 轮询式 helper |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_mcp_plugin.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_aliases.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_permissions.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_kairos.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_memory_skills_hooks.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_agent_team.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_session.rs` | Modify | 同上 |
| 其余 `tests/commands_*.rs`、`test1_login_structure.rs`、`test3_plan_flow.rs` | Modify | 同上（少量） |
| `crates/allthecodes/tests/pty_tui_e2e/tests/test4_task_execution.rs`、`test5_compact.rs`、`running_task_slash_commands.rs`、`model_flow.rs` | Modify **谨慎** | 大 timeout 测试拆分，120s/300s 收紧到事件驱动 + 阶段快照，**不改在线测试语义** |
| `crates/allthecodes/tests/pty_tui_e2e/README.md` | Modify | 更新"运行"段、新增"时效策略"段 |
| `development/test/README.md` | Modify | 在索引表登记 `pty-tui-e2e-timing-plan.md` |
| `development/test/pty-tui-e2e-timing-plan.md` | Plan | **本文件**（已在主分支前置 commit） |
| `development/worktree-workflow-artifacts/2026-07-17-pty-e2e-timing.html` | Artifact | worktree 阶段产出，含耗时 before / after 对比 |

> 计划文件本身按 §3-1 主分支前置流程提交，不在 worktree 内改；其余 Modify / Create 全部在 worktree `worktree/pty-e2e-timing` 内完成。

## 5. 任务拆分（TDD 形式）

> 每个 Task 末尾 commit。Commit 范围只暂存本 Task 列出的路径。

### Task 0：基线度量与失败测试

**Files:**
- Create: `crates/allthecodes/tests/pty_tui_e2e/tests/_timing_baseline.rs`（临时，仅本 Task）
- Modify: `crates/allthecodes/tests/pty_tui_e2e/tests/mod.rs`（注册 `mod _timing_baseline;`）

**Interfaces:**
- Consumes: 现成 `PtySession`、`TestRunner`。
- Produces: 一次性 `#[test] fn timing_baseline_snapshot()`：spawn + skip trust + `/version` + 收尾，时间打印到 `--nocapture` 输出，作为本机基线参考。

- [ ] **Step 1: 写失败测试** — 新增 `_timing_baseline.rs`，断言 `Wait(Duration::from_secs(0))` 已淘汰（grep 方式：在测试源中检测 `Wait(Duration::from_secs(2))` 数量低于阈值），该测试初始应失败。
- [ ] **Step 2: 跑测试** — `cargo test -p allthecodes --test pty_tui_e2e timing_baseline -- --nocapture -Z unstable-options --format json` 不可用时退回 `--nocapture`，人工记录墙钟。
- [ ] **Step 3: 记录基线** — 把 215 passed / 36 ignored / 1930s 写进本计划 §1 顶部（已完成）。
- [ ] **Step 4: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/tests/_timing_baseline.rs crates/allthecodes/tests/pty_tui_e2e/tests/mod.rs && git commit -m "test: add pty e2e timing baseline probe"`。

### Task 1：信号化等待原语（harness + script）

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`
- Modify: `crates/allthecodes/tests/pty_tui_e2e/script.rs`

**Interfaces:**
- Consumes: `current_screen()` / `current_text()` / `status_bar()`。
- Produces:
  - `pub const RENDER_WAIT_FAST: Duration = Duration::from_millis(500);`（默认渲染等待，用于替换不需要 2s 的 `Wait(2s)`）。
  - `pub fn wait_for_screen_text_quick(&self, needle: &str, deadline: Duration) -> bool`：以 50ms 粒度轮询 `current_screen()`，最多 `deadline`。
  - `TestStep::WaitUntilScreen(String, Duration)` 新.step：`Wait(secs)` 的语义化替代，screen 出现 needle 即返回。
- 兼容：保留 `TestStep::Wait(Duration)`，不动现有监控点，逐步替换。

- [ ] **Step 1: 写失败测试** — 在 `harness.rs` `#[cfg(test)] mod tests` 新增 `wait_for_screen_text_quick_returns_true_when_present`，构造伪 buffer 断言快速返回；当前无该函数，编译失败即红。
- [ ] **Step 2: 实现** — 在 `PtySession` 添加 `wait_for_screen_text_quick`；在 `script.rs` `TestStep` 新增 `WaitUntilScreen` 分支，复用上述 helper。
- [ ] **Step 3: 跑测试** — `cargo test -p allthecodes --test pty_tui_e2e harness::tests -- --nocapture` 期望 PASS。
- [ ] **Step 4: clippy** — `cargo clippy -p allthecodes --tests -- -D warnings`，无新增警告。
- [ ] **Step 5: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/harness.rs crates/allthecodes/tests/pty_tui_e2e/script.rs && git commit -m "feat(pty-e2e): add signal-driven screen wait helpers"`。

### Task 2：离线小测试固定 Wait 替换（高频文件）

**Files:** Task 2.x 子项，每子项一个 commit；下一文件开始前先确认上一文件 cargo test 全绿。

| 子任务 | 文件 | Wait 次数 |
|--------|------|----------|
| 2.1 | `tests/commands_core_info.rs` | 59 |
| 2.2 | `tests/commands_mcp_plugin.rs` | 46 |
| 2.3 | `tests/commands_aliases.rs` | 45 |
| 2.4 | `tests/commands_permissions.rs` | 43 |
| 2.5 | `tests/commands_kairos.rs` | 43 |
| 2.6 | `tests/commands_memory_skills_hooks.rs` | 34 |
| 2.7 | `tests/commands_agent_team.rs` | 32 |
| 2.8 | `tests/commands_session.rs` | 22 |
| 2.9 | 其余 `commands_*.rs` 中 ≤ 18 的文件 | 合计 ~100 |

**Interfaces:** Consumes Task 1 helper。每条 `TestStep::Wait(Duration::from_secs(2))` 改为 `TestStep::WaitUntilScreen("<命令名首词>".into(), Duration::from_secs(3))` 或保留 `Wait(Duration::from_millis(300))`（极短间隔）。`WaitForAny` 既有用法不动。

- [ ] **Step 1: 替换文件 2.1** — `commands_core_info.rs` 全文件 `Wait(secs)` → 信号化或 ≤ 300ms。
- [ ] **Step 2: 跑文件** — `cargo test -p allthecodes --test pty_tui_e2e commands_core_info -- --nocapture` 全绿且耗时显著下降（记录 before/after）。
- [ ] **Step 3: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/tests/commands_core_info.rs && git commit -m "test(pty-e2e): signal-drive waits in commands_core_info"`。
- [ ] **Step 4–N:** 对 2.2–2.9 各文件重复 Step 1–3，逐文件 commit。

### Task 3：大 timeout 测试拆分

**Files:**
- Modify: `tests/test4_task_execution.rs`
- Modify: `tests/test5_compact.rs`
- Modify: `tests/running_task_slash_commands.rs`
- **不改** `model_flow.rs`、`conversation.rs` 中的 `#[ignore]` 在线测试语义（只允许替换其内非在线依赖的固定 Wait）。

**Interfaces:** `API_TIMEOUT` 不变。离线断言段拆成 ≥ 2 个独立 `#[test]`，互相不串接；单测试目标墙钟 ≤ 30s。

- [ ] **Step 1: 拆 test4** — 把"任务执行 + 后置断言"拆为 `test4_task_start`、`test4_task_finish`，各自有 `API_TIMEOUT` cap。
- [ ] **Step 2: 跑 test4** — `cargo test -p allthecodes --test pty_tui_e2e test4_ -- --nocapture` 全绿，单测墙钟 ≤ 30s。
- [ ] **Step 3: 拆 test5 / running_task** — 同模式。
- [ ] **Step 4: 跑全部离线** — `cargo test -p allthecodes --test pty_tui_e2e`（不带 `--ignored`），目标墙钟降到 §2 Phase 1 阈值。
- [ ] **Step 5: Commit** — 一个文件一 commit，message 前缀 `test(pty-e2e): split large timeout tests`。

### Task 4：lift 全局串行锁（受控并发）

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`
- Create: `.config/nextest.toml`（若不存在；若 `cargo-build-test-system-plan.md` 已建则 Extend）
- Modify: `scripts/cargo-build-test.sh`（nextest mode 若已有，则登记 `tui_pty_e2e` thread-budget；可选）

**Interfaces:**
- 删除 / 缩窄 `pty_test_lock()`：保留锁只为 **读 `/proc` 清理 + DSR 自动回写互斥**，不再包住整段会话。
- 改为细粒度 `cleanup_lock`：仅在 `cleanup_detached_workspace_processes` 与 reader 线程 DSR 写入处持锁，会话期间不持全锁。
- 在 `.config/nextest.toml` 给 `test-groups.tui_pty_e2e.max-threads` 设为 N（先 2，验证后提至 4），并 `slow-timeout=45s`。

**风险:** PTY / 共享 `E2E_WORKSPACE` 并发会撞 workspace 目录与日志目录；hash logs_dir 用 `Local::now()` 在同一秒并发跑会写同一目录。需在 §6 验证。

- [ ] **Step 1: 写失败测试** — `harness::tests::concurrent_sessions_do_not_serialize`：spawn 两个空 `PtySession`，断言有重叠时间窗（用 `thread::spawn` + `Arc<AtomicUsize>` concurrent counter > 1）。初始红（锁会保证只 1）。
- [ ] **Step 2: 收窄锁** — 把 `_serial_guard` 从 `PtySession` 字段中移除，`pty_test_lock()` 改名为 `cleanup_lock()`，仅在 cleanup / DSR 写入持锁。
- [ ] **Step 3: 给 logs_dir 加并发隔离** — `logs_dir()` 增加每会话 `test_subdir` 用 PID + 计数器防碰撞。
- [ ] **Step 4: 跑失败测试** — 期望 PASS。
- [ ] **Step 5: nextest 配置** — 给 `tui_pty_e2e` 设 `max-threads = 2`（保守起步），`slow-timeout=45s`。
- [ ] **Step 6: 跑 nextest** — `cargo nextest run -p allthecodes --test pty_tui_e2e` 全绿，墙钟减半。
- [ ] **Step 7: 提升 max-threads=4** — 跑一次确认无 flake，否则回 2。
- [ ] **Step 8: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/harness.rs .config/nextest.toml scripts/cargo-build-test.sh && git commit -m "perf(pty-e2e): lift global pty serialization lock"`。

### Task 5：进程生命周期收尾收窄

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`

**Interfaces:**
- `cleanup_detached_workspace_processes()` 节流：用 `OnceLock<Mutex<Instant>>`，最近 2s 已扫过则跳过；遇残留才升级完整扫描。
- `capture_output_after_finish` 里 `Duration::from_millis(200)` 调到 100，reader join deadline 从 500ms 调到 250ms（确认无 flake 再保留）。

- [ ] **Step 1: 写失败测试** — 断言 215 测试场景下 `find_detached_workspace_processes` 调用次数显著下降（用 `AtomicUsize` 计数器）。
- [ ] **Step 2: 实现节流** — 加 throttle。
- [ ] **Step 3: 全跑** — `cargo test -p allthecodes --test pty_tui_e2e` 全绿，墙钟再降一档。
- [ ] **Step 4: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/harness.rs && git commit -m "perf(pty-e2e): throttle detached workspace cleanup"`。

### Task 6：文档与索引收尾

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/README.md`
- Modify: `development/test/README.md`

- [ ] **Step 1: README 时效策略段** — 在 `pty_tui_e2e/README.md` 加"## 时效策略"段：解释串行锁历史、信号化等待、`WaitUntilScreen` 用法、nextest thread-budget、不应再加 `Wait(secs>1)`。
- [ ] **Step 2: 索引登记** — 在 `development/test/README.md` 表格新增一行指向本计划，状态标 ⭐ P0。
- [ ] **Step 3: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/README.md development/test/README.md && git commit -m "docs(pty-e2e): document timing strategy and index plan"`。

### Task 7：HTML artifact + ff 合并

**Files:**
- Create: `development/worktree-workflow-artifacts/2026-07-17-pty-e2e-timing.html`

- [ ] **Step 1: 写 artifact** — 含：任务目标、§3 流程步骤、改动列表（对应 §4 文件）、计划文件路径（本文件）、commit 列表、验证依据（`cargo test` before/after 墙钟数字、`cargo clippy` 无新警告）。
- [ ] **Step 2: 全跑回归** — `cargo build --workspace --release` + `cargo test -p allthecodes --test pty_tui_e2e`，记录最终墙钟。
- [ ] **Step 3: Commit artifact** — `git add -A -- development/worktree-workflow-artifacts/2026-07-17-pty-e2e-timing.html && git commit -m "docs(pty-e2e): add timing plan worktree artifact"`。
- [ ] **Step 4: ff 合并 + 删树** — 按 `development/workflow/2026-07-16-per-session-worktree-workflow-plan.md` §3-5/6 执行 `git merge --ff-only worktree/pty-e2e-timing` + `git push origin allthecodes` + `git worktree remove .worktrees/pty-e2e-timing` + `git branch -d worktree/pty-e2e-timing`。
- [ ] **Step 5: 移除临时基线探针** — 删 `_timing_baseline.rs` + `tests/mod.rs` 中 `mod _timing_baseline;`，单独 commit。

## 6. 验收清单

- [ ] 离线 `cargo test -p allthecodes --test pty_tui_e2e` 墙钟 ≤ 600s（§2 Phase 2）。
- [ ] `cargo clippy -p allthecodes --tests -- -D warnings` 无新增警告。
- [ ] `grep -rE "Wait\(Duration::from_secs\([2-9]\|[12][0-9]\)" crates/allthecodes/tests/pty_tui_e2e/tests --include=*.rs | wc -l` 显著减少（≤ Phase 1 前 10%，大 timeout 例外记录在计划中）。
- [ ] nextest profile 存在 `tui_pty_e2e.max-threads ≥ 2`，且 `cargo nextest run -p allthecodes --test pty_tui_e2e` 全绿。
- [ ] `development/test/README.md` 索引登记本计划且状态为 ⭐ P0。
- [ ] `crates/allthecodes/tests/pty_tui_e2e/README.md` 有"时效策略"段。
- [ ] HTML artifact 路径存在且独立可读，包含 before 1930s / after 数字。
- [ ] `git log --oneline allthecodes..worktree/pty-e2e-timing` 全部为 ff 可达。

## 7. 非目标

- 不改 `#[ignore]` 在线测试的语义、API 调用路径、`API_TIMEOUT=60s`；只允许替换其内部与在线端无关的固定 `Wait`。
- 不引入 Bazel、不改 npm release 策略（与 `cargo-build-test-system-plan.md` 非目标一致）。
- 不在本计划内做大规模 rustfmt 重排、import 重排，避免 churn 掩盖性能 diff。
- 不删除 `TestStep::Wait`：它是合法原语，仅约束"勿用 `Wait(secs>1)` 等待 UI"。
- 不在 Phase 2 之前解除串行锁：避免在信号化等待尚未达成时引入并发竞态。
- 不重写 `vt100` / `portable_pty` 集成层。

## 8. 风险与回退

| 风险 | 触发条件 | 回退 |
|------|----------|------|
| 并发撞 `E2E_WORKSPACE` 日志目录 | Phase 2 同秒多测试写 `logs_dir()` | 给 `logs_dir` 加 PID/计数，回 `max-threads=1` 重跑确认 |
| nextest profile 未安装 | CI 仍用 `cargo test` | 不替换 CI；nextest 本地验证用 |
| 信号化等待漏判 | `WaitUntilScreen` needle 选错，测试莫名通过 | 在每个替换处保留 `AssertScreenContains` 双重确认 |
| 在线测试误伤 | Task 3 拆 `test4` 时动到 `#[ignore]` 段 | Task 3 Step 1 仅碰离线断言段，不动 `API_TIMEOUT` 行为 |
| ff 失败 | 主分支在 task 期间被推进 | 按 §3-5 先 rebase `worktree/pty-e2e-timing` 再 ff，不产 `--no-ff` |

## 9. 执行顺序

主分支前置（本文件 commit）→ worktree `worktree/pty-e2e-timing` → Task 0 → Task 1 → Task 2.1..2.9 → Task 3 → Task 6（README 部分可与 Task 4 并行写）→ Task 4 → Task 5 → Task 7（artifact + ff 合并 + 删树）。
