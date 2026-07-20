# 代码拆分与编译性能总执行清单

> 建立日期：2026-07-20
> 基线源码：`ba1494e4`
> 状态：Active
> 定位：本目录中唯一维护执行顺序与勾选状态的台账
> 架构与范围：[`codebase-optimization-plan-2026-07-03.md`](codebase-optimization-plan-2026-07-03.md)
> 当前证据：[`current-state-audit-2026-07-20.md`](current-state-audit-2026-07-20.md)
> 编译专项：[`compile-performance-plan-2026-07-20.md`](compile-performance-plan-2026-07-20.md)
> Query 边界：[`query-loop-boundary-decision-2026-07-03.md`](query-loop-boundary-decision-2026-07-03.md)

## 使用规则

1. 只在本文件勾选执行状态；其他文档保存设计、基线和历史证据，不再维护平行执行清单。
2. 顶层 checkbox 是一个独立交付任务。开始前必须在主分支新增或更新该任务的 scoped plan 并单独提交，再创建专属 worktree。
3. worktree 内只修改、静态检查和提交；禁止运行 `cargo check/build/clippy/test/nextest` 或 Rust 测试二进制。
4. 候选提交必须 `git merge --ff-only` 到主分支后，才按分层 SOP 运行 Rust 验证；失败返回同一 worktree 修复，不在主分支直接改代码。
5. 每项只有在“代码/文档、测试、结构指标、行为边界、提交和推送”证据齐全后才能勾选。仅创建类型或移动文件不能算完成。
6. Full Build 行为不得缩减；禁止重建平行 `allthecodes-query`、复制权限/路径判定、删除 vendored OpenSSL 或让并行 worktree 共用 Cargo target。
7. 每完成一个文档更新任务单独 commit；只显式暂存本任务路径。

状态约定：

- `[x]`：已完成且证据已经落库。
- `[ ]`：未开始、进行中或仍有验收 residual；不能用文字“基本完成”代替勾选条件。
- `依赖`：必须先完成的任务 ID。
- `可并行`：接口冻结后允许与列出的任务并行；仍需不同 worktree 和不同 `CARGO_TARGET_DIR`。

## 总体依赖顺序

```text
BASE
 ├─> PERF-001 ─> PERF-002
 │             └─> BUILD-001 ─> CONTRACT-001/002 ─> WEB-002 ─> CI-001
 └─> GUARD-001 ─> HOOK-001 ─> TOOL-001 ─> PERM-001/FS-001
                                      └─> TOOL-002 ─> LIFECYCLE-001 ─> QUERY-001
                                                                    └─> SUBMIT-001 ─> HOOK-002/SESSION-001

GUARD-001 ─> RECORD-001 ────────────────────────────────────────────────> SUBMIT-001

CAP-001 ─> DISCOVERY-001
FS-001  ─> FILES-001 / WORKFLOW-001
PERM-001 + CAP-001 ─> TUI-001/002/003/004
```

## 并行与冲突规则

- PERF-001/002、GUARD-001 和不重叠路径的 characterization tests 可以并行；每个 worktree 必须使用独立 target。
- Query 与 Submit 不并行起步：先提交共同 event/terminal contract，再改 query producer，最后改 submit consumer。
- Tool policy、permission/FS contract、pipeline 必须按依赖串行；permission UI 与 FS consumers 在 contract 冻结后可分 lane 并行。
- Group Chat 业务 ownership 拆分与 Web 物理 crate 搬迁不得合成同一提交；先证明行为，再测量 crate 边界收益。
- Files/session domainization 与 FS/session consumer migration 修改重叠，必须串行；Group Chat 与 terminal lane 可在 shared contract 冻结后并行。
- tool/engine/config/commands/protocol 等高扇出 contract 每次只合入一个，以保留 reverse-dependency 和 timing A/B 因果。
- TUI App/input/render residual 与 MCP/agent typed state 分开提交，避免 snapshot 变化掩盖状态模型回归。
- 生产状态机拆分与纯测试文件分组不得合成一个“重构”提交。

## Phase 0：审计和基线（已完成）

- [x] **BASE-001 当前代码热点审计**
  - 产物：`current-state-audit-2026-07-20.md`。
  - 证据：1,455 个 Rust 文件、主要大函数/大文件、P0/P1/P2 ownership 边界和残留状态已记录。

- [x] **BASE-002 编译性能基线**
  - 产物：`compile-performance-plan-2026-07-20.md`。
  - 证据：isolated cold release 401.7 秒、517 dirty units、Web 185.3 秒、OpenSSL 73.7 秒、SQLite 64.9 秒。

- [x] **BASE-003 计划状态校准**
  - 产物：权威总计划已区分 `open / partial / done-residual`；runtime boundary 计划已改为历史实施记录。
  - 证据：CS-001..017 和 CP-001..007 均有当前状态与对应详细文档。

## Phase 1：测量护栏和真实构建模式

- [ ] **PERF-001 固化 cold/warm/touched-crate 编译基准**（CP-001）
  - 依赖：BASE-002。
  - 实施：新增结构化 benchmark harness；每种场景用全新 isolated target 跑三次并取 median。
  - 记录：commit/dirty paths、toolchain、features、wall/CPU/RSS、dirty units、top 15 units、target 增量和 binary size。
  - 完成条件：结果可区分 Cargo timings 与 `/usr/bin/time -v`；warm no-op 不高于 2 秒；第二个 target 可复现。

- [ ] **PERF-002 建立跨 target compiler cache 与精确清理生命周期**（CP-002）
  - 依赖：PERF-001。
  - 可并行：GUARD-001。
  - 实施：A/B `sccache`；分别报告 Rust 与 C/C++ 命中；worktree 保持 `.tmp/atc-<slug>` target 隔离。
  - 清理：只在 worktree 已推送并删除后，对精确 target 先 `cargo clean --dry-run --target-dir <exact-path>`，再执行经确认的清理。
  - 完成条件：第二个 isolated target 的 Rust cache hit rate ≥ 80%；无 phantom metadata；target 增长和清理报告可审计。

- [ ] **BUILD-001 把 TUI/Web/daemon/ACP/storage 变成真实 Cargo feature**（CS-017 / CP-003）
  - 依赖：PERF-001。
  - 可并行：GUARD-001、HOOK-001（接口无交叉时）。
  - 实施：默认 `full` 保持发布行为；新增/校准 `tui`、`web`、`daemon`、`acp`、`sqlite-storage`、`json-storage` forwarding；mode-only 依赖改 optional 并显式关闭不需要的 default features。
  - 必查：`allthecodes-tools default=full`、`allthecodes-types default=runtime-types` 的无意传播。
  - 完成条件：minimal TUI normal+build 闭包不含 Web、Axum、SQLx、SQLite、daemon、ACP；目标包数 ≤ 300；默认 full 行为不变。

- [ ] **GUARD-001 补齐 P0 characterization/fault-injection 测试矩阵**
  - 依赖：BASE-001。
  - 可并行：PERF-001/002。
  - 覆盖：query recovery/abort、submit terminal/recorder failure、tool hook/permission/sandbox、unknown permission、FS symlink/workspace boundary、Group Chat terminal/SSE sequence。
  - 完成条件：每个后续 P0 重构都有可复用的行为基线；测试失败能指出 lifecycle stage，而不是只比对最终字符串。

- [ ] **HOOK-001 冻结 Full Build hook event matrix 与 runner contract**（CS-002 前置）
  - 依赖：GUARD-001。
  - 实施：定义 agent/prompt/file-watcher/api-query/skill-improvement 的 typed input/output、blocking、timeout、error 和 audit 行为；建立上游行为矩阵与 runner port。
  - 完成条件：contract/characterization tests 锁定全部事件；未知 output/error fail closed；runtime wiring 由 HOOK-002 完成。

- [ ] **GATE-1 Phase 1 退出门**
  - 必须完成：PERF-001、BUILD-001、GUARD-001、HOOK-001。
  - PERF-002 可在明确记录 cache 不影响语义的前提下稍后完成，但不得晚于 Web 物理拆 crate。

## Phase 2：安全决策与核心生命周期

- [ ] **RECORD-001 收敛 session/record-replay 单一真相源**（CS-009）
  - 依赖：GUARD-001；可与不重叠的 tool policy lane 并行。
  - 实施：消除静默 SQLite→JSON fallback、无边界双写和重复 legacy import；定义 persistence contract、migration marker 与故障语义。
  - 完成条件：每个 session mutation 只有一个 authoritative commit；migration 可重入且不重复导入；fallback 显式记录。

- [ ] **TOOL-001 收敛唯一 shell/tool policy decision**（CS-003）
  - 依赖：GUARD-001、HOOK-001。
  - 实施：统一 read/build/mutate/destructive/deploy/secret/parser-failure；输出 typed decision id、subject、sandbox requirement 和 audit labels。
  - 完成条件：engine、Auto mode、sandbox、UI 不再分别分类同一 command；Bash/PowerShell golden matrix 全覆盖。

- [ ] **PERM-001 完成 typed permission payload 与 UI fail-closed**（CS-006）
  - 依赖：TOOL-001。
  - 可并行：FS-001。
  - 实施：后端发送 `PermissionSubject`、`RiskClassification`、`AllowedResponses`、`DefaultResponse`、`DecisionId`；UI 只渲染 typed payload。
  - 完成条件：UI 不再解析 tool JSON 或 shell command；未知 kind/response/label 保持 deny/cancel；prompt、execution、audit 共用 decision id。

- [ ] **FS-001 建立单一 FsCapabilityService**（CS-011）
  - 依赖：TOOL-001。
  - 可并行：PERM-001。
  - 实施：统一 resolve/canonicalize/workspace/symlink/read/write/archive/extract/mutation policy。
  - 完成条件：Read/Edit/Write/ApplyPatch/Web files/workflow/archive 使用同一 capability/decision；handler/tool 不复制 path guard。

- [ ] **TOOL-002 完成 ToolExecutionPipeline 二次收口**（CS-001）
  - 依赖：TOOL-001、PERM-001、FS-001、HOOK-001。
  - 实施：immutable plan + typed stage result + decision ledger + terminal recorder；拆分 `maybe_prompt_user` 等二次巨型函数。
  - 完成条件：`execute_tool_impl` < 180 行；stage 不用多个 bool 组合；pre/post hook、permission、sandbox、audit 顺序可 fault-injection 测试。

- [ ] **CAP-001 完成 RuntimeCapabilityRegistry 消费闭环**（CS-010）
  - 依赖：BUILD-001；与 TOOL-002 改同一接口时串行。
  - 实施：command/tool/MCP/deferred/discovered state 全部消费 typed capability；移除名称列表和 magic visibility 分支。
  - 完成条件：新增 capability 只修改一个注册来源；`ExecuteExtraTool` 不再绕过 registry。

- [ ] **LIFECYCLE-001 冻结 QueryTurnEvent 与 terminal outcome contract**（CS-004/005 前置）
  - 依赖：HOOK-001、TOOL-002、GUARD-001。
  - 实施：定义 query producer 与 submit consumer 共用的 typed event、terminal outcome、flush/record ownership；先提交 contract 和 compatibility tests。
  - 完成条件：query/submit 可分三步迁移且旧新路径对同一 fixture 产生等价 event 序列。

- [ ] **QUERY-001 缩小唯一 QueryTurnState orchestrator**（CS-004）
  - 依赖：LIFECYCLE-001。
  - 实施：扩展现有 recovery；拆 model attempt、tool round、terminal outcome、record projection；只保留一个 engine 内 query 实现。
  - 完成条件：主 orchestrator < 250 行；abort、timeout、partial response、fallback、max token、tool error、stop hook 各有 typed transition 测试。

- [ ] **SUBMIT-001 让 SubmitTransaction 成为唯一 side-effect owner**（CS-005）
  - 依赖：LIFECYCLE-001、QUERY-001、RECORD-001。
  - 实施：append、usage/cost、session persistence、record、flush、terminal result 只经一个 commit/abort path。
  - 完成条件：主函数 < 250 行；重复 result、partial stream、abort、recorder failure 不产生重复 mutation；immediate boundary flush 语义保留。

- [ ] **HOOK-002 完成 hook runtime wiring 并移除 structural stub**（关闭 CS-002）
  - 依赖：TOOL-002、QUERY-001、SUBMIT-001。
  - 实施：把 HOOK-001 contract 接入 agent/prompt/file-watcher/api-query/skill-improvement 的完整 runtime path。
  - 完成条件：生产路径不再含 structural stub；被 ignore 的 full-build TODO 测试转为通过；blocking/timeout/error/audit 对齐上游。

- [ ] **SESSION-001 下沉 SessionMutationService**（CS-008）
  - 依赖：SUBMIT-001、RECORD-001。
  - 实施：resume/branch/feedback/engine replacement/transcript mutation 离开 HTTP handler。
  - 完成条件：handler 不直接重建或替换 active engine；service 有 transaction/rollback tests。

- [ ] **GATE-2 Phase 2 退出门**
  - 必须完成：RECORD-001、TOOL-001/002、PERM-001、FS-001、CAP-001、LIFECYCLE-001、QUERY-001、SUBMIT-001、HOOK-002。
  - SESSION-001 必须在 Web sessions 拆分前完成。

## Phase 3：高扇出 contracts、Web 与领域服务

- [ ] **CONTRACT-001 物理拆分 tool contracts 与 implementation**（CP-005）
  - 依赖：BUILD-001、TOOL-002。
  - 实施：engine/MCP/LSP/commands 的 contract-only 消费不再拉入 browser/session/sandbox/reqwest 等 full 实现。
  - 完成条件：reverse-dependency 和 incremental timing 有 before/after；无新依赖环；query implementation 不复制。

- [ ] **CONTRACT-002 收窄 engine/config/types 高扇出边界**（CP-005）
  - 依赖：CONTRACT-001；各子边界独立任务/commit。
  - 实施：engine ports/events、config schema vs runtime I/O、经 churn/timing 证明的 types domain split。
  - 完成条件：每次基础类型变更触发的 workspace crates 明显下降；default feature 不把 runtime 实现传播给 schema-only 消费者。

- [ ] **COMMAND-001 拆 commands metadata/dispatch 与 protocol catalog**（CP-005）
  - 依赖：CAP-001、CONTRACT-002。
  - 实施：commands 先抽稳定 metadata/dispatch；protocol 分 runtime wire types、API catalog、codegen renderer；IPC handler 分 validation/controller/projection。
  - 完成条件：保留一个生成真相源；不把 275 operations 改成第二套手写表；API/command 新增点数量下降。

- [ ] **WEB-001 拆 Group Chat 领域服务**（CS-016）
  - 依赖：GUARD-001；与 Web compile crate 物理切分串行。
  - 实施：routes、room service、invite service、message store、delegation runtime、SSE projection。
  - 完成条件：handler 不直接 mutation/runtime dispatch；store 与 SSE 共用 event sequence/terminal state；HTTP/SSE 行为不变。

- [ ] **WEB-002 把 allthecodes-web 拆成可并行编译单元**（CS-016 / CP-004）
  - 依赖：BUILD-001、CONTRACT-001/002、WEB-001、PERF-002。
  - 实施：稳定 `web-core`、2–4 个互不依赖的 domain crate、`web-runtime`、薄 composition crate。
  - 完成条件：最大 Web Rust unit ≤ 90 秒；full cold median ≤ 300 秒；修改一个 domain 不重编其他独立 domain。

- [ ] **WEB-003 让 ApiOperationRegistry 成为唯一分发来源**（CS-007）
  - 依赖：COMMAND-001；与 WEB-002 先冻结 registry contract。
  - 实施：消除 92 项平行手写表、巨型 dispatcher match 和重复 HTTP registry 组合。
  - 完成条件：新增 operation 只注册一次；transport/legacy adapter 从同一 descriptor 生成或消费。

- [ ] **FILES-001 拆 Web files read/mutation/archive 服务**（CS-016）
  - 依赖：FS-001、WEB-002 的 core contract。
  - 实施：metadata/read、mutation、archive 分域；全部消费 FsCapabilityService。
  - 完成条件：symlink/workspace/archive traversal tests 通过；handler 无 path policy。

- [ ] **TERMINAL-001 拆 terminal transport/session/process 状态机**（CS-016）
  - 依赖：WEB-002 的 runtime contract。
  - 实施：transport、session state、process backend、I/O buffer、resize、cleanup/health。
  - 完成条件：disconnect/resize/kill/idle cleanup 有 deterministic state tests；无 orphan process。

- [ ] **WORKFLOW-001 拆 file workflow parser/store/transition/projection**
  - 依赖：FS-001、RECORD-001。
  - 实施：definition parser、run store/locking、state transition、task projection、thin tool adapter。
  - 完成条件：authorization/path policy 不复制；transition 可独立 fault-injection；legacy/task adapter 不持有 store internals。

- [ ] **DISCOVERY-001 拆 discovery providers/orchestrator/ranking/adapters**
  - 依赖：CAP-001、CONTRACT-001。
  - 实施：provider contracts、并发聚合、ranking/redaction、tool DTO adapters。
  - 完成条件：provider failure/partial result/deterministic ranking 独立测试；无 process-wide mutable runtime 旁路。

- [ ] **DAEMON-001 拆 daemon route/controller/gateway lifecycle**
  - 依赖：SESSION-001、COMMAND-001。
  - 实施：route DTO、controller、process/status projection；gateway tests 按 handshake/recovery/shutdown 分组。
  - 完成条件：route 不直接管理 process/session；shutdown 和 descendant cleanup 有端到端证据。

- [ ] **AGENT-001 拆 AgentRuntime 与 worktree lifecycle**
  - 依赖：RECORD-001、CAP-001。
  - 实施：process lifecycle、event/output projection、forced shutdown、worktree prepare lease、execution、finalization。
  - 完成条件：cancel/crash/timeout/finalize 各有唯一终态；临时 worktree/child process 无泄漏。

- [ ] **GATE-3 Phase 3 退出门**
  - 必须完成：CONTRACT-001/002、WEB-001/002、WEB-003、FILES-001、TERMINAL-001。
  - WORKFLOW/DISCOVERY/DAEMON/AGENT 可分批完成，但未完成项不得被 Web/contract 重构隐藏。

## Phase 4：TUI、shell 和测试可维护性

- [x] **TUI-BASE stores/overlay/view-model 初始结构**（CS-012/013/014 的历史实现）
  - 已有：Conversation/Prompt/Session/Layout stores、OverlayState、MessageListViewModel、RuntimeViewState。
  - 注意：仅代表基础结构完成，以下 residual 仍全部未完成。

- [ ] **TUI-001 缩小 App facade 与 typed input router**（CS-012/013）
  - 依赖：PERM-001、CAP-001、SESSION-001。
  - 实施：global guard → overlay → selection → command/prompt mode → fallback；拆 `handle_key_event`、`dispatch_bound_action`。
  - 完成条件：`App` 直接字段目标 < 40；主 key router < 160 行；overlay render/input/focus 仍同源。

- [ ] **TUI-002 收口 render/view-model 单次 projection**（CS-014）
  - 依赖：TUI-001。
  - 实施：每 frame 只构造一次 message view-model；render 不做业务数据变形；拆主 render ownership。
  - 完成条件：主 render 明显缩小；snapshot 覆盖 welcome/session/task/permission/overlay；无重复 projection。

- [ ] **TUI-003 移除 MCP magic index 与 agent wizard usize step**（CS-015）
  - 依赖：CAP-001；可与 TUI-002 并行。
  - 实施：typed MCP action enum、typed wizard step/state transition。
  - 完成条件：删除 100/101/1000/2000 等 action encoding；非法 step 无法构造；navigation tests 全覆盖。

- [ ] **TUI-004 拆 run_tui runtime lifecycle**
  - 依赖：TUI-001、SESSION-001。
  - 实施：terminal guard、event loop、engine event routing、shutdown/export 分离。
  - 完成条件：`run_tui` 不再同时拥有所有 runtime lifecycle；terminal 恢复和 shutdown 在 panic/error/cancel 下可验证。

- [ ] **SHELL-001 拆 Bash execution 并声明化 Git read-only rules**
  - 依赖：TOOL-001/002。
  - 实施：Bash call 分 process/sandbox/output/cleanup；Git 规则改 per-subcommand declarative table，仍只有一个 classifier。
  - 完成条件：安全分类 golden tests 不回退；超时/kill/output truncation 行为不变。

- [ ] **TEST-001 按领域拆巨型测试模块**
  - 依赖：对应生产领域任务完成后逐域执行，不做一次性全仓搬迁。
  - 范围：engine lifecycle、TUI app、config settings、semantic tools、MCP client、daemon gateway。
  - 完成条件：fixture 只提供 construction/helper；测试按 phase/failure/protocol scenario 可定位；不制造万能上下文。

- [ ] **GATE-4 Phase 4 退出门**
  - 必须完成：TUI-001/002/003/004、SHELL-001；相关 PTY 快照统一审阅。

## Phase 5：CI 去重、最终性能和收尾

- [ ] **CI-001 把 PTY 相同闭包 compile 从 4 次降为 1 次**（CP-006）
  - 依赖：BUILD-001、WEB-002、TUI-004。
  - 实施：一次 build + 可重定位 nextest archive/artifact；若不安全则单 job nextest 最多 10 个独立 test process。
  - 完成条件：保持 shard coverage、隔离目录、失败日志和 cleanup；同一 OS/feature compile 4→1；compute minutes/cache size 有对比。

- [ ] **PERF-003 运行架构阶段最终 benchmark**（CP-004/005）
  - 依赖：WEB-002、CONTRACT-001/002、CI-001。
  - 完成条件：no-op ≤ 2 秒；第一阶段 full cold median ≤ 300 秒；架构阶段 ≤ 280 秒；Web 最大 unit ≤ 90 秒；minimal TUI ≤ 300 packages。
  - 若未达标：保留 raw timings，按 top unit/critical path 新建后续任务，禁止调整口径掩盖失败。

- [ ] **PERF-004 处理小收益依赖/profile/linker 实验**（CP-007）
  - 依赖：PERF-003；不得抢在结构优化前。
  - 范围：tokio-tungstenite 0.26/0.29、rand 0.8/0.9、test/debug profile、release-fast、Web codegen-units、安装后 A/B mold/lld。
  - 完成条件：每项有独立 A/B、binary size/startup/throughput 回归；正式 release profile 与 vendored release 约束不被替代。

- [ ] **CLOSE-001 完成全量验证、文档归档和远端确认**
  - 依赖：所有必须项和 PERF-003。
  - 主分支验证：format → clippy → 非 PTY workspace lib → targeted suites → PTY nextest → workspace release build。
  - 规则：普通 libtest PTY 保持 `--test-threads=1`；nextest 由配置最多 10 process；完整 `cargo test --workspace` 单任务最多 2 次。
  - 文档：更新本清单 checkbox、权威计划 residual、WORK_STATUS/KNOWN_ISSUES；历史计划只追加结果，不改写证据。
  - 完成条件：本地/远端 SHA 一致；任务 worktree 和精确 target 按流程清理；无未解释 warning、失败或行为缩减。

## CS / CP 覆盖映射

| 原 ID | 本清单任务 |
|---|---|
| CS-001 | TOOL-002 |
| CS-002 | HOOK-001, HOOK-002 |
| CS-003 | TOOL-001 |
| CS-004 | LIFECYCLE-001, QUERY-001 |
| CS-005 | LIFECYCLE-001, SUBMIT-001 |
| CS-006 | PERM-001 |
| CS-007 | WEB-003 |
| CS-008 | SESSION-001 |
| CS-009 | RECORD-001 |
| CS-010 | CAP-001 |
| CS-011 | FS-001 |
| CS-012 | TUI-BASE, TUI-001 |
| CS-013 | TUI-BASE, TUI-001 |
| CS-014 | TUI-BASE, TUI-002 |
| CS-015 | TUI-003 |
| CS-016 | WEB-001/002, FILES-001, TERMINAL-001 |
| CS-017 | BUILD-001 |
| CP-001 | PERF-001 |
| CP-002 | PERF-002 |
| CP-003 | BUILD-001 |
| CP-004 | WEB-002, PERF-003 |
| CP-005 | CONTRACT-001/002, COMMAND-001, PERF-003 |
| CP-006 | CI-001 |
| CP-007 | PERF-004 |

## 每项提交前统一证据模板

- [ ] scoped plan 已在主分支单独提交。
- [ ] 专属 worktree/branch/target 名称已记录，且 worktree 内未运行 Rust。
- [ ] characterization tests 在改动前可复现，新增 fault-injection 覆盖关键分支。
- [ ] 行为、结构和编译 before/after 指标已记录；没有仅以文件数量作为成果。
- [ ] `cargo fmt --all --check` 在主分支通过。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 在主分支通过。
- [ ] `cargo test --workspace --exclude allthecodes --lib` 及 touched-crate targeted tests 通过。
- [ ] 涉及 TUI 时，nextest PTY suite 按最多 10 process 完成；普通 libtest 保持单线程。
- [ ] 最终要求的 `cargo build --workspace --release` 在主分支通过且无新增 warning。
- [ ] HTML artifact 包含目标、步骤、改动、计划、commit、验证依据。
- [ ] 只显式暂存任务路径；ff-only 合并；远端 SHA 核对；完成后才删除 worktree/branch/target。
