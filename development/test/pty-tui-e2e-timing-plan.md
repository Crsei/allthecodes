# PTY TUI E2E 测试时效优化计划

> **本文件是计划文件**。按 `development/workflow/2026-07-16-per-session-worktree-workflow-plan.md` §3-1，计划文件必须先单独 commit 到主分支 `allthecodes`，本文件本身不进 worktree 改。后续按本计划落地代码 / 文档 / artifact 时，再开 `worktree/pty-e2e-timing` 完成实际改动。

生效日期：2026-07-17
作用范围：`crates/allthecodes/tests/pty_tui_e2e/` 全部测试、`crates/allthecodes/logs/`（运行产物）、`development/test/README.md`、`development/test/pty-tui-e2e-timing-plan.md`（索引登记）。可选：`.config/nextest.toml`、`scripts/cargo-build-test.sh`。

## 实施状态（截至 2026-07-18，`f0b743d5`）

本计划的 Phase 1–3 及 review 修复已在 `allthecodes` 分支落地、fast-forward 合并并发布到 `origin/allthecodes`。下表区分“代码已落地”与“端到端阈值已验收”。

| Task | 状态 | 已有证据 / 边界 |
|------|------|----------------|
| 0 基线度量 | **部分完成** | 已记录历史基线：215 passed / 36 ignored / 1930.03s；未创建一次性 `_timing_baseline.rs` 探针，也没有其临时文件清理提交。 |
| 1 信号化原语 | **完成（等效实现）** | 实际 API 是 `TestStep::WaitForScreenText`，而不是草案中的 `WaitUntilScreen`；代表性 PTY 测试、`cargo check --tests` 与目标 clippy 已通过。 |
| 2 高频离线 Wait 替换 | **完成（本批范围）** | `ff9bbfd4`–`f9aeec14` 覆盖 11 个离线测试文件；代表样本实测降幅约 29–58%。完整离线套件墙钟已重测（见 Task 7）。 |
| 3 大 timeout 拆分 | **跳过（已确认范围）** | 不改 `#[ignore]` 在线测试语义；`test4` / `test5` 的字面拆分与 §7 冲突，`running_task_slash_commands.rs` 已无可拆分的固定等待。 |
| 4 解除全局串行锁 | **完成（已验证）** | `9fd69fd2` 已移除会话全程锁、保留 cleanup 短锁；`concurrent_sessions_do_not_serialize` 通过。`1fb98812` 把 nextest override 修正为 `binary(pty_tui_e2e)`；`c64bf37c` 再把默认 workspace 按 runner PID 隔离，`54196b3d` 首次启用 `max-threads=4`，Task 12 已把完整回归证明过的预算提升到 10。 |
| 5 收尾收窄 | **完成（已验证）** | `637a2985` 已实现 cleanup 节流、flush 200ms→100ms、reader join 500ms→250ms；`c64bf37c` 把节流状态改为 `Option<Instant>`，首次 cleanup 必定扫描，窗口内后续调用才跳过。harness tests 7/7 通过。 |
| 6 文档与索引 | **完成** | PTY README 已同步 runner PID 隔离、10-way nextest、首次 cleanup 必扫和串行 libtest 边界；`development/test/README.md` 已登记为 Phase 1–3 完成。 |
| 7 artifact 与合并 | **完成** | 旧 artifact 已纠正失败着色和缺失结果；新 artifact 为 `development/worktree-workflow-artifacts/2026-07-18-pty-timing-review-fixes.html`。完整 nextest：220 passed / 36 skipped / 0 failed，real 341.47s；串行 libtest：220 passed / 36 ignored / 0 failed，real 1263.19s；release workspace build exit 0。review worktree 已 ff-merge 至 `allthecodes`（`a9cd5cd9`）。 |
| 12 nextest 10-way | **完成（已验证）** | `0678ca05` 将 `tui_pty_e2e.max-threads` 提升到 10；最终完整回归 220 passed / 36 skipped / 0 failed，nextest summary 143.308s、real 144.65s，较 4-way 341.47s 再缩短约 57.6%。首次全跑暴露的陈旧 MCP 文本断言已由 `16626dc1` 修复；目标测试、MCP 模块、fmt、目标 clippy 与 release workspace build 均通过。 |

当前边界：nextest 每个 test case 使用独立 runner 进程，PID workspace 隔离成立；普通 libtest 的 case 共用 runner PID，因此全套仍须 `--test-threads=1`。显式 `E2E_WORKSPACE` 覆盖保持不变。

## 1. 背景：为什么耗时 1930s（历史基线）

实测耗时：**215 passed / 36 ignored / 0 failed，1930.03s**。

以下分析描述的是 Phase 1/2 落地**前**的基线状态，保留用于解释优化动机；当前实现以本文件顶部“实施状态”和 §6 验收清单为准。

逐项拆解后，历史时长来自**可叠加的三个结构性因素**，而不是单点故障：

### 1.1 全局串行锁（决定性因素）

在基线版本中，`crates/allthecodes/tests/pty_tui_e2e/harness.rs` 在 `PtySession::spawn` 内持有进程级 `static Mutex<()>` 直到 `finish_in_dir` 退出：

```rust
fn pty_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
```

基线版本的 `PtySession` 把 `_serial_guard: MutexGuard<'static, ()>` 存为结构体字段，锁生命周期与会话等长——测试一开始就 acquire，直到 `finish_in_dir`（写日志、回收 reader 线程、`cleanup_detached_workspace_processes`）返回后才 drop。

含义：
- 即便 `cargo test` 默认按线程并发调度（或后续切到 nextest），**任意时刻最多只有 1 个 PTY 测试在跑**。
- 215 个 passed 测试 = 215 次串行 spawn + 渲染 + 断言 + 收尾。
- 1930s / 215 ≈ **8.98s/test 平均墙钟**，与每个测试的固定 sleep 总量（见下）量级吻合，说明串行是主要瓶颈，而非单测本身慢到拖垮整体。

**当前状态：** `9fd69fd2` 已移除该会话全程锁，改为短时 `cleanup_lock()`；`c64bf37c` 已按 nextest runner PID 隔离默认 workspace，`54196b3d` 首次把 nextest 配置提升为 `max-threads = 4`，Task 12 的 10-way 完整回归全绿后已进一步提升为 `max-threads = 10`。

### 1.2 大量硬编码 `Wait(Duration::from_secs(N))` 等待

基线中的 `TestStep::Wait` 是**固定墙钟等待**，不依赖屏幕/输出信号，无论 TUI 是否已渲染完成都睡满。基线全量统计 `Wait(Duration::from_secs(N))` 出现次数：

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

每个测试 spawn 一次 `allthecodes` 二进制；`binary_path()` 解析后 `portable_pty::openpty` + spawn，进程冷启动 + TUI 初始化 + trust gate 渲染在慢盘 / 共享 CI 上常见的 0.5–1.5s。基线的 `finish_in_dir` 末尾固定消耗：
- `std::thread::sleep(Duration::from_millis(200))` 收尾；
- reader 线程 `join` 最多等 500ms 才 detach；
- `cleanup_detached_workspace_processes()` 扫描 `/proc`、按 `workspace` + `mcp-cli-daemon`/`mcp-cli-bridge` 关键字匹配残留进程并对齐 `SIGTERM` → 等 `KILL_REAP_TIMEOUT=2s` → 必要时 `SIGKILL`。

215 次串行收尾，`/proc` 全表扫描 × 215，加上零星 1–2s 的 reap 等待，是大批量离线测试下额外的 ~1–2s/test 隐藏成本。

**当前状态：** `637a2985` 已把 flush 收紧为 100ms、reader join 收紧为 250ms，并加入 2 秒 cleanup 节流；`c64bf37c` 保证首次 cleanup 必扫。完整 nextest 与串行 libtest 均已重测并全绿。

### 1.4 36 个 `#[ignore]` 在线测试

`README.md` 明示在线测试需真实 API key、`API_TIMEOUT=60s`，单轮对话耗时秒级到上限 60s 不等。这些测试在标准 `cargo test --test pty_tui_e2e`（不带 `--ignored`）下默认跳过，**不计入 1930s**。本计划聚焦离线套件墙钟；在线测试只做"不在本批被反复一起拉起"的隔离，不改其语义。

## 2. 目标

| 阶段 | 目标 | 验收阈值 | 当前状态 |
|------|------|----------|----------|
| Phase 0 | 度量基线 + 失败测试暴露问题 | 单跑离线套件，记录耗时 + 每测试耗时分桶；新增 `Wait`/`WaitForAny` 信号化验收用例 | 历史基线已记录；临时探针未创建。 |
| Phase 1 | 把固定 `Wait(N)` 替换为信号化等待（`WaitForScreenText` / `WaitForText` / `WaitForAny` 带短 timeout） | 离线套件墙钟 ≤ 1200s（降幅 ≥ 38%） | 信号化替换已落地且代表样本下降 29–58%；当前 10-way 完整 nextest 为 143.308s，已满足阈值。 |
| Phase 2 | 有界并发：lift 全局串行锁，按"逻辑不冲突分组"并行 | 离线套件墙钟 ≤ 600s（较 Phase 1 再降 ≥ 50%） | 会话全程锁已解除，override 使用 `binary(pty_tui_e2e)`，默认 workspace 按 runner PID 隔离；`max-threads=10` 完整回归 220/220，通过且命令墙钟 144.65s。 |
| Phase 3 | 进程生命周期收尾收窄 + `/proc` 扫描降频 | 收尾非离线套件剩余时长的 ≥ 40% | flush/reader join 收紧、2 秒 throttle 和首次必扫均已落地；与 10-way 并发综合后，相对严格串行 1250.847s 降至 144.65s（约 88.4%）。 |

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
| `crates/allthecodes/tests/pty_tui_e2e/script.rs` | Modify | **已完成**：保留 `TestStep::Wait`，新增并使用 `WaitForScreenText` 信号化等待 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_core_info.rs` | Modify | 替换 `Wait(2s)` → `WaitForScreenText` / 轮询式 helper |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_mcp_plugin.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_aliases.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_permissions.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_kairos.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_memory_skills_hooks.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_agent_team.rs` | Modify | 同上 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_session.rs` | Modify | 同上 |
| 其余 `tests/commands_*.rs`、`test1_login_structure.rs`、`test3_plan_flow.rs` | Modify | 同上（少量） |
| `crates/allthecodes/tests/pty_tui_e2e/tests/test4_task_execution.rs`、`test5_compact.rs`、`running_task_slash_commands.rs`、`model_flow.rs` | Modify **谨慎** | Task 3 已按确认范围跳过字面拆分；保留在线测试语义，后续仅可在有独立验收时改动 |
| `crates/allthecodes/tests/pty_tui_e2e/README.md` | Modify | 更新"运行"段、新增"时效策略"段 |
| `development/test/README.md` | Modify | 在索引表登记 `pty-tui-e2e-timing-plan.md` |
| `development/test/pty-tui-e2e-timing-plan.md` | Plan | **本文件**（已在主分支前置 commit） |
| `development/worktree-workflow-artifacts/2026-07-17-pty-e2e-timing.html` | Artifact | worktree 阶段产出，含耗时 before / after 对比 |

> 计划文件本身按 §3-1 主分支前置流程提交，不在 worktree 内改；其余 Modify / Create 全部在 worktree `worktree/pty-e2e-timing` 内完成。

## 5. 任务拆分（TDD 形式）

> 每个 Task 末尾 commit。Commit 范围只暂存本 Task 列出的路径。
>
> 下方保留原始 TDD 分步以便追溯；Task 标题下的“状态”是当前权威结论。仅在有提交或运行记录可直接佐证时勾选原分步，未勾选不应覆盖该 Task 的已实现状态。

### Task 0：基线度量与失败测试

> **状态：部分完成。** 历史全套基线已写入 §1；一次性 `_timing_baseline.rs` 探针没有创建，因此不存在其失败测试、运行记录或清理提交。

**Files:**
- Create: `crates/allthecodes/tests/pty_tui_e2e/tests/_timing_baseline.rs`（临时，仅本 Task）
- Modify: `crates/allthecodes/tests/pty_tui_e2e/tests/mod.rs`（注册 `mod _timing_baseline;`）

**Interfaces:**
- Consumes: 现成 `PtySession`、`TestRunner`。
- Produces: 一次性 `#[test] fn timing_baseline_snapshot()`：spawn + skip trust + `/version` + 收尾，时间打印到 `--nocapture` 输出，作为本机基线参考。

- [ ] **Step 1: 写失败测试** — 新增 `_timing_baseline.rs`，断言 `Wait(Duration::from_secs(0))` 已淘汰（grep 方式：在测试源中检测 `Wait(Duration::from_secs(2))` 数量低于阈值），该测试初始应失败。
- [ ] **Step 2: 跑测试** — `cargo test -p allthecodes --test pty_tui_e2e timing_baseline -- --nocapture -Z unstable-options --format json` 不可用时退回 `--nocapture`，人工记录墙钟。
- [x] **Step 3: 记录基线** — 已把 215 passed / 36 ignored / 1930.03s 写进本计划 §1 顶部。
- [ ] **Step 4: Commit** — `git add -A -- crates/allthecodes/tests/pty_tui_e2e/tests/_timing_baseline.rs crates/allthecodes/tests/pty_tui_e2e/tests/mod.rs && git commit -m "test: add pty e2e timing baseline probe"`。

### Task 1：信号化等待原语（harness + script）

> **状态：完成（等效实现）。** 实现采用 `TestStep::WaitForScreenText`，不是草案命名 `WaitUntilScreen`；代表性 PTY 测点、`cargo check --tests` 和目标 clippy 均已有通过记录。原草案的逐步 TDD 过程未单独保留为独立提交，不将其反推为完整套件验收。

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`
- Modify: `crates/allthecodes/tests/pty_tui_e2e/script.rs`

**Interfaces:**
- Consumes: `current_screen()` / `current_text()` / `status_bar()`。
- Produces:
  - `PtySession::wait_for_screen_text(&self, needle: &str, timeout: Duration) -> bool`：轮询当前可见屏幕，命中即返回、超时返回 `false`。
  - `TestStep::WaitForScreenText(String, Duration)`：`Wait(secs)` 的语义化替代，screen 出现 needle 即返回。
- 兼容：保留 `TestStep::Wait(Duration)`，不动现有监控点，逐步替换。

- [ ] **Step 1: 写失败测试** — 在 `harness.rs` `#[cfg(test)] mod tests` 新增 `wait_for_screen_text_quick_returns_true_when_present`，构造伪 buffer 断言快速返回；当前无该函数，编译失败即红。
- [x] **Step 2: 实现** — 已在 `script.rs` 落地 `TestStep::WaitForScreenText` 分支；以当前实现为准，不再要求草案中的 `WaitUntilScreen` 名称。
- [ ] **Step 3: 跑测试** — `cargo test -p allthecodes --test pty_tui_e2e harness::tests -- --nocapture` 期望 PASS。
- [x] **Step 4: clippy** — 已通过 `cargo clippy -p allthecodes --test pty_tui_e2e --quiet -- -D warnings`；完整 `--tests` 范围也在 Phase 2 记录中通过。
- [x] **Step 5: 提交边界** — 信号化原语由随后的 Phase 1 提交共同落地；代表性替换提交为 `ff9bbfd4`–`f9aeec14`，不另补写与实际历史不符的 `feat` 提交。

### Task 2：离线小测试固定 Wait 替换（高频文件）

> **状态：完成（本批范围）。** 已覆盖 11 个离线测试文件（`commands_core_info`、`mcp_plugin`、`aliases`、`permissions`、`kairos`、`memory_skills_hooks`、`agent_team`、`session`、`auth`、`git`、`query`），提交范围为 `ff9bbfd4`–`f9aeec14`。代表样本降幅约 29–58%；完整套件墙钟仍待单独验收。

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

**Interfaces:** 使用 `TestStep::WaitForScreenText("<命令名首词>".into(), Duration::from_secs(3))` 替换适合信号化的 `TestStep::Wait(Duration::from_secs(2))`，或删除冗余等待 / 保留 `Wait(Duration::from_millis(300))`（极短间隔）。`WaitForAny` 既有用法不动。

- [x] **Step 1: 替换文件 2.1** — `commands_core_info.rs` 已按信号化/删除冗余等待完成（`ff9bbfd4`）。
- [ ] **Step 2: 跑文件** — `cargo test -p allthecodes --test pty_tui_e2e commands_core_info -- --nocapture` 全绿且耗时显著下降（记录 before/after）。
- [x] **Step 3: Commit** — 已提交 `ff9bbfd4 test(pty_e2e): replace Wait(2s) with signal-driven waits in commands_core_info`。
- [x] **Step 4–N:** 2.2–2.9 的本批文件已分别完成并提交；具体文件与提交边界见本节状态说明及 artifact。逐文件完整运行记录未作为全套验收替代。

### Task 3：大 timeout 测试拆分

> **状态：跳过（已确认范围）。** 保持 `#[ignore]` 在线测试语义；`test4_task_execution.rs`、`test5_compact.rs` 的字面拆分没有执行，`running_task_slash_commands.rs` 已无可拆分的固定等待。此项不是“完成”，后续若调整范围须重新立项并定义在线测试验收。

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

> **状态：完成。** `9fd69fd2` 删除会话全程 `_serial_guard`，仅在 cleanup 中使用 `cleanup_lock()`；`c64bf37c` 按 runner PID 隔离默认 workspace，`54196b3d` 将验证通过的 nextest budget 提升到 4。完整 4-way 套件 220/220 通过。

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`
- Create: `.config/nextest.toml`（若不存在；若 `cargo-build-test-system-plan.md` 已建则 Extend）
- Modify: `scripts/cargo-build-test.sh`（nextest mode 若已有，则登记 `tui_pty_e2e` thread-budget；可选）

**Interfaces:**
- 删除会话全程 `pty_test_lock()`；仅为 `/proc` cleanup 保留短时互斥，不再包住整段会话。
- 改为细粒度 `cleanup_lock`：仅在 `cleanup_detached_workspace_processes` 中持锁，会话期间不持全锁。
- `.config/nextest.toml` 使用 `binary(pty_tui_e2e)` 归组、`slow-timeout=45s` 与 `test-groups.tui_pty_e2e.max-threads=4`。

**风险:** 显式把同一个 `E2E_WORKSPACE` 传给并发 case 仍会共享状态；默认路径和日志路径已经按 runner PID 隔离。

- [x] **Step 1: 写失败测试** — `harness::tests::concurrent_sessions_do_not_serialize` 已新增并作为 lock-lift invariant 保留。
- [x] **Step 2: 收窄锁** — `_serial_guard` 与 `pty_test_lock()` 已移除，`cleanup_lock()` 仅包住 cleanup 临界区。
- [x] **Step 3: 给 logs_dir 加并发隔离** — `logs_dir()` 已加入 PID 后缀，避免 nextest 多进程同秒目录碰撞。
- [x] **Step 4: 跑失败测试** — `concurrent_sessions_do_not_serialize` 已通过（0.77s）。
- [x] **Step 5: nextest 配置** — `binary(pty_tui_e2e)` 正确归组并启用 `max-threads=4`。
- [x] **Step 6: 跑 nextest** — 完整离线套件 220 passed / 36 skipped / 0 failed，real 341.47s。
- [x] **Step 7: 提升 max-threads=4** — 先完成 2/4 线程抽样，再以完整套件确认无 flake后保留。
- [x] **Step 8: Commit** — 已提交 `9fd69fd2 perf(pty-e2e): lift global pty serialization lock`；未改动的 `scripts/cargo-build-test.sh` 未被纳入提交。

### Task 5：进程生命周期收尾收窄

> **状态：完成。** `637a2985` 已落实 2 秒 cleanup 节流、flush 200ms→100ms、reader join 500ms→250ms；`c64bf37c` 修复首次 cleanup 被跳过。完整 nextest 与串行 libtest 均全绿。

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`

**Interfaces:**
- `cleanup_detached_workspace_processes()` 节流：用 `OnceLock<Mutex<Option<Instant>>>`，首次必扫，最近 2s 已扫过时才跳过后续调用。
- `capture_output_after_finish` 里 `Duration::from_millis(200)` 调到 100，reader join deadline 从 500ms 调到 250ms（确认无 flake 再保留）。

- [x] **Step 1: 写失败测试** — `cleanup_detached_workspace_processes_throttles` 已新增，使用扫描/跳过计数验证节流 invariant。
- [x] **Step 2: 实现节流** — 2 秒 throttle、flush 和 reader join 收紧均已落地。
- [x] **Step 3: 全跑** — 串行 `cargo test` 220 passed / 36 ignored / 0 failed；4-way nextest 220 passed / 36 skipped / 0 failed。
- [x] **Step 4: Commit** — 已提交 `637a2985 perf(pty-e2e): throttle detached cleanup + tighten reader join`。

### Task 6：文档与索引收尾

> **状态：完成。** PTY README 已同步信号化等待、runner PID 隔离、4-way nextest 和 cleanup 首次必扫；开发测试索引已标注 Phase 1–3 完成。

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/README.md`
- Modify: `development/test/README.md`

- [x] **Step 1: README 时效策略段** — 已加入“时效策略”及 Phase 2 并发/cleanup 说明，使用当前 API 名称 `WaitForScreenText`。
- [x] **Step 2: 索引登记** — 已在 `development/test/README.md` 登记本计划，状态为“Phase 1–3 已实现”。
- [x] **Step 3: Commit** — 已由 `911ead3b` 与 `cd30bd70` 完成文档/索引更新；提交信息按实际范围记录。

### Task 7：HTML artifact + ff 合并

> **状态：完成。** 原 artifact 已纠正历史结果并由新 artifact 补充 review 修复；完整 nextest、串行 libtest、release build 和 ff 合并均已完成。

**Files:**
- Create: `development/worktree-workflow-artifacts/2026-07-17-pty-e2e-timing.html`

- [x] **Step 1: 写 artifact** — artifact 已含任务目标、改动/提交清单、代表性 before/after、局部验证和遗留边界。
- [x] **Step 2: 全跑回归** — release build exit 0；串行 libtest 220/220 passed，real 1263.19s；4-way nextest 220/220 passed，real 341.47s。
- [x] **Step 3: Commit artifact** — artifact 与 Phase 1/2 文档已由 `911ead3b`、`cd30bd70` 提交。
- [x] **Step 4a: 本地 ff 合并** — 原 Phase 1/2 和本轮 review worktree 均已 ff 合并；本轮合并 HEAD 为 `a9cd5cd9`。
- [x] **Step 5: 确认无需移除临时基线探针** — `_timing_baseline.rs` 和对应 `mod` 从未创建，因此没有待删文件或额外提交。

## 6. 验收清单

- [x] 离线 `cargo nextest run -p allthecodes --test pty_tui_e2e --no-fail-fast` 墙钟 ≤ 1200s。当前 4-way 实测 220 passed / 36 skipped / 0 failed，nextest 340.329s、real 341.47s；同时满足 Phase 2 ≤600s 阈值。
- [x] `cargo clippy -p allthecodes --test pty_tui_e2e -- -D warnings` exit 0，无 warning；`cargo fmt --all --check` exit 0。
- [x] 固定 `Wait(secs ≥ 2)` 的剩余数量按 Phase 1 范围已替换（11 个离线测试文件，`ff9bbfd4`–`f9aeec14`）；本期不重跑全局量化门槛（与原 §7 非目标一致）。
- [x] nextest profile 达到 `tui_pty_e2e.max-threads = 4`，且完整 220-test 离线套件全绿；不是用 discovered/matched 数代替 executed passed 数。
- [x] `development/test/README.md` 已登记本计划，状态更新为 Phase 1–3 已实现。
- [x] `crates/allthecodes/tests/pty_tui_e2e/README.md` 已有“时效策略”/“并发与锁”/“收尾 / cleanup 节流”/“历史滤片 bug 与修复”节，并列出正确的离线执行方式。
- [x] 两份 HTML artifact 已用 HTML5 兼容解析器读取通过；旧 artifact 保留历史并纠正失败结果，新 artifact 记录当前命令、退出码、墙钟和边界。
- [x] review worktree 已 ff 到 `allthecodes`：主分支当前包含 `a9cd5cd9`。
- [x] 本轮提交已推送至 `origin/allthecodes`；`.worktrees/pty-timing-review-fixes` 与 `worktree/pty-timing-review-fixes` 分支均已删除。

## 7. 非目标

- 不改 `#[ignore]` 在线测试的语义、API 调用路径、`API_TIMEOUT=60s`；只允许替换其内部与在线端无关的固定 `Wait`。
- 不引入 Bazel、不改 npm release 策略（与 `cargo-build-test-system-plan.md` 非目标一致）。
- 不在本计划内做大规模 rustfmt 重排、import 重排，避免 churn 掩盖性能 diff。
- 不删除 `TestStep::Wait`：它是合法原语，仅约束"勿用 `Wait(secs>1)` 等待 UI"。
- 在完成信号化等待后才解除串行锁；该前置已满足，Phase 2 lock lift 与 Phase 3 runner workspace 隔离均已落地。
- 不重写 `vt100` / `portable_pty` 集成层。

## 8. 风险与回退

| 风险 | 触发条件 | 回退 |
|------|----------|------|
| 并发撞共享 workspace 状态 | 显式 `E2E_WORKSPACE` 被多个测试共同设置，或新增跨 workspace 的全局状态 | 默认路径已按 runner PID 隔离并通过 4-way 完整回归；显式共享时由调用者负责，发现回归时降回已验证的较低 thread budget |
| nextest profile 未安装 | CI 仍用 `cargo test` | 不替换 CI；nextest 本地验证用 |
| 信号化等待漏判 | `WaitForScreenText` needle 选错，测试莫名通过 | 在每个替换处保留 `AssertScreenContains` 双重确认 |
| 在线测试误伤 | Task 3 拆 `test4` 时动到 `#[ignore]` 段 | Task 3 Step 1 仅碰离线断言段，不动 `API_TIMEOUT` 行为 |
| 后续合并或清理误操作 | 主分支再推进，或误处理陈旧 locked worktree | 以当前已合并事实为准；清理前先核验 worktree 状态和祖先关系，不产 `--no-ff` |

## 9. 执行顺序

已执行：主分支前置计划 → Phase 1（Task 1/2）→ Phase 2（Task 4/5）→ override 滤片修复 → Phase 3 runner workspace 隔离 → 2/4 线程抽样 → 4-way 完整 nextest → 串行 libtest → fmt/clippy/release build → 文档与 artifact → ff 合并。

交付已完成：最终状态已提交并推送，`pty-timing-review-fixes` worktree 与临时分支已清理。

## 10. 2026-07-18 review 修复计划

> 本节是本轮 `pty-timing-review-fixes` 的主分支前置计划。代码、README、旧 artifact 修正和新 artifact 只在 `worktree/pty-timing-review-fixes` 中完成；本文件待 worktree ff 合并后再在主分支单独同步最终状态。

### Task 8：修复首次 cleanup 被节流跳过

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`

**问题:** `LAST_CLEANUP_SCAN` 以 `Instant::now()` 初始化，首次调用立即满足 `elapsed < CLEANUP_THROTTLE` 并返回。nextest 每个 test case 使用独立进程，导致推荐执行方式下每个测试唯一一次 cleanup 都可能不扫描 `/proc`。

- [x] 把节流状态改为 `Option<Instant>`，保证首次调用扫描、窗口内后续调用跳过、窗口后恢复扫描（`c64bf37c`）。
- [x] 测试已收紧为“首次调用恰好扫描 1 次、随后窗口内调用跳过”，不再允许 `scans == 0` 通过。
- [x] cleanup targeted test、harness tests（7/7）、fmt 和目标 clippy 全部通过。

### Task 9：隔离 nextest workspace 并恢复有界并发

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/harness.rs`
- Modify: `.config/nextest.toml`

**边界:** 保留显式 `E2E_WORKSPACE` 覆盖；默认 workspace 至少按 runner 进程 PID 隔离，使 nextest 并发 test case 不再共享 `settings.json` / `sessions.db`。普通 `cargo test` 仍要求 `--test-threads=1`，不把进程内多线程 libtest 宣称为已隔离。

- [x] 新增默认 workspace runner PID 隔离与显式覆盖测试。
- [x] `commands_core_info` 在 `max-threads=2` 为 25/25、59.529s；`max-threads=4` 为 25/25、32.027s。
- [x] 完整 4-way 离线套件 220/220 通过后才保留提升后的 thread budget。
- [x] 完整 nextest：220 passed / 36 skipped / 0 failed，nextest 340.329s、real 341.47s。

### Task 10：关闭脚本时序失败和验证证据缺口

**Files:**
- Modify（如能复现并需要）: `crates/allthecodes/tests/pty_tui_e2e/script.rs`
- Modify（如能复现并需要）: 对应 `script::tests`

- [x] `script::tests` 在完整串行 run 中通过；实际可复现失败全名为 `tests::commands_memory_skills_hooks::memory_set_get_rm_cycle`，根因是 `test_value` 在 120 列状态栏裁成 `test_v`，已用短唯一值修复（`448e9a9c`）。
- [x] 完整串行 libtest exit 0：220 passed / 0 failed / 36 ignored，real 1263.19s。
- [x] `cargo build --workspace --release` exit 0，无 warning，real 312.61s。

### Task 11：同步状态与 artifact

**Files:**
- Modify: `crates/allthecodes/tests/pty_tui_e2e/README.md`
- Modify: `development/test/README.md`
- Modify: `development/worktree-workflow-artifacts/2026-07-17-pty-e2e-timing.html`
- Create: `development/worktree-workflow-artifacts/2026-07-18-pty-timing-review-fixes.html`
- Modify（ff 合并后在主分支单独提交）: `development/test/pty-tui-e2e-timing-plan.md`

- [x] 旧 artifact 不再把 `216 passed / 1 failed` 标成成功；已补齐历史全 nextest 结果并删除“完整套件未跑”的过期陈述。
- [x] Task 4/5/7 顶部状态、验收清单和执行顺序已同步到当前实现。
- [x] `show-config` 的历史 219 条归组记录与当前实际 executed 220 passed 已明确区分。
- [x] 新 artifact 记录计划路径、commit、精确命令、退出码、墙钟与残余边界，并通过 HTML 解析验证。

## 11. 2026-07-18 提升 nextest 并发到 10

> **状态：完成。** `worktree/pty-timing-max-threads-10` 中的配置、断言、README 和 artifact 已 fast-forward 合并到主分支；本节已在主分支同步最终验收状态。

### Task 12：把 `tui_pty_e2e.max-threads` 从 4 提升到 10

**Files:**
- Modify: `.config/nextest.toml`
- Modify: `crates/allthecodes/tests/pty_tui_e2e/README.md`
- Modify: `development/test/README.md`
- Create: `development/worktree-workflow-artifacts/2026-07-18-pty-timing-max-threads-10.html`
- Modify（ff 合并后在主分支单独提交）: `development/test/pty-tui-e2e-timing-plan.md`

**验收边界:** nextest 每个 test case 已按 runner PID 隔离默认 workspace，因此可以提高进程级并发；普通 libtest 仍共用 runner PID，继续要求 `--test-threads=1`。显式 `E2E_WORKSPACE` 覆盖不改变。

- [x] `.config/nextest.toml` 的 `tui_pty_e2e.max-threads` 已改为 10，并同步注释（`0678ca05`）。
- [x] `cargo nextest show-config test-groups` 已确认 `pty_tui_e2e` binary 仍归入 `tui_pty_e2e (max threads = 10)`。
- [x] `commands_core_info` 10-way 为 25/25 passed（real 17.85s）；首次完整回归唯一失败是 `mcp_help_no_args` 等待陈旧文本，`16626dc1` 改为断言当前稳定可见语义 `MCP servers` 后，目标测试 1/1、MCP 模块 19/19、最终完整套件 220/220 均通过。
- [x] 最终完整结果：220 passed / 36 skipped / 0 failed，nextest summary 143.308s、命令 real 144.65s；较 4-thread 341.47s 基线再缩短约 57.6%。
- [x] `cargo fmt --all --check`、`cargo clippy -p allthecodes --test pty_tui_e2e -- -D warnings`、`cargo build --workspace --release` 均 exit 0，且无新增 warning。
- [x] README、测试索引和 `development/worktree-workflow-artifacts/2026-07-18-pty-timing-max-threads-10.html` 已更新并 ff 合并；`allthecodes` 已推送，`pty-timing-max-threads-10` worktree 与临时分支已清理。
