# 测试隔离 / 长跑门禁 — KNOWN_ISSUES 与 CLAUDE/AGENTS SOP 落地计划

> 计划日期：2026-07-18
> 任务 slug：`test-isolation-known-issues`
> 触发来源：复盘 Codex session `019f668f-207c-7853-aa03-e9f0755bcd9e`（2026-07-15 → 2026-07-16），抓出 5 轮全仓 `cargo test --workspace` + 95 分钟轮询守候的真实瓶颈。

## 1. 背景与目标

那次 session 末尾 18 小时几乎全在反复跑全仓门禁：

| 维度 | 数据 |
|---|---|
| 全仓 `cargo test --workspace` 跑了 | 5 次（4 次未拿 exit 0） |
| PTY 套件单轮耗时 | 32–34 分钟（215 项并发 PTY） |
| "守候式轮询" agent 消息 | 77 条，占全部 agent 消息 47% |
| 三段最长连续守候 | 36.8 + 36.1 + 21.9 ≈ 95 分钟纯等待 |
| 串行暴露的独立根因 | 5 类（PTY snapshot / PTY 陈旧断言 / daemon 时序 / IPC null / 第三轮截断） |

根因不是模型不努力，而是：

1. 单一巨型 PTY 套件绑架了所有验证，daemon/IPC 失败本可 5 分钟捕获却被 PTY 32 分钟屏蔽
2. 测试夹具漏掉的隔离（CODEX_HOME / HTTP_PROXY / CARGO_TARGET_DIR 跨树共享 / Optional 字段反序列化）以"测试慢或挂"形式暴露，调试成本极高
3. Codex exec 工具是同步 10s yield 轮询，长跑没有完成回调，模型一边等一边发同义状态消息

## 2. 本次落地范围（只改文档，不碰代码/产物）

1. `CLAUDE.md`：新增「## 测试分层验证 SOP」一节，把"立即可行"的 5 条优化写成可背诵的硬性约束。
2. `AGENTS.md`：同步新增同一节（两文件各自维护，分别 commit）。
3. `development/archive/KNOWN_ISSUES.md`：新增章节记录本次 session 踩到的 **4 个根因**作为「已修复但需防回归」条目，便于后续审查与回归监控。

> 不在本次范围：测试 binary 拆分、Codex exec 后台任务模型、CI 改造、产品代码修复（IPC/daemon 修复分别在 commit `46b22cad` / `71bb6768` 已落地）。

## 3. 4 个根因条目（写入 KNOWN_ISSUES）

- **TESTISO-001 / Target dir 跨工作树共享**：并行 worktree 共用 `…/.tmp/allthecodes-target`，跨树 build/test 会链接到对方分支的陈旧 `types` 元数据，导致 phantom 编译错误。
  - 规约：并行 worktree 必须各自 `CARGO_TARGET_DIR=…/.tmp/atc-<slug>`。
  - 已确认根因：session 2026-07-16 复盘。
- **TESTISO-002 / `CODEX_HOME` 测试未隔离**：ACP 鉴权测试在非隔离环境下读取真机 Codex 登录态，把"格式无效但非空的 API key"误判为可用凭据，从而触发非确定性失败。
  - 规约：所有涉及凭据/认证的集成测试必须在夹具启动时显式 `CODEX_HOME=<tmp>`，并在测试环境强制隔离。
  - 已落地：ACP 测试隔离修复（session 同期 commit）。
- **TESTISO-003 / `HTTP_PROXY` 接管内部回环请求**：Team Memory 向固定 loopback endpoint 发请求被系统 `HTTP_PROXY` 接管，引起错误超时分类，并把内部 secret 发给环境代理。
  - 规约：对所有 loopback / 内部 RPC 客户端，强制 `no_proxy()`；secret-bearing 客户端禁止使用环境代理。
  - 已落地：commit `71bb6768`（daemon loopback 隔离 + submit abort reset）。
- **TESTISO-004 / Optional 字段反序列化用非 Optional 结构**：IPC `security: None` 序列化为 JSON `null`，legacy adapter 按非 Optional 结构反序列化触发 `InvalidPayload` panic；测试任务 panic 后接收端仍持 sender，造成无限等待。
  - 规约：协议 DTO 对所有可空字段声明 `Option<T>`，反序列化对 missing / null 双兼容，非法非 null 仍 fail-closed；adapter 转换提前到 pending 注册之前；交互测试对每条接收路径加 5 秒接收边界。
  - 已落地：commit `46b22cad`（IPC `security` 兼容缺失/null）。

## 4. SOP 五条（写入 CLAUDE.md / AGENTS.md）

1. **分层验证**：先 fmt / clippy / lib 单测（秒级～分钟级），再 `-exclude allthecodes` 跑非-PTY crate，**最后** 单独跑 `-p allthecodes --test pty_tui_e2e`。任何一次 commit 前的全仓验证都按此顺序，禁止一开始就 `cargo test --workspace`。
2. **快照一次性 batch**：首次出现 PTY snapshot 失败时，用 `INSTA_UPDATE=always` 一次性更新所有 `.snap.new`，统一肉眼审阅后提交；不再"修一个 → 跑全套 → 再发现下一个"。
3. **跨 worktree target 隔离**：每个 worktree 各自 `CARGO_TARGET_DIR=…/.tmp/atc-<slug>`。CLAUDE.md 现有的"全局 target 路径"在并行 worktree 场景下必须让位于本规则。
4. **凭据/环境隔离前置守卫**：涉及认证、网络或凭据的测试在夹具启动时显式隔离 `CODEX_HOME` / `HTTP_PROXY` / `NO_PROXY=*`；不在测试靠真实环境被动发现未隔离。
5. **全仓.workspace 测试一次任务上限**：超过 N=2 次完整 `cargo test --workspace` 必须降级到分 crate 跑并先解释为什么分 crate 不足以定位；不允许"修一项就重跑全套"的循环。

## 5. 流程步骤

1. 本计划文件单独 commit 到主分支 `allthecodes`。
2. `git worktree add -b worktree/test-isolation-known-issues .worktrees/test-isolation-known-issues allthecodes`。
3. 在 worktree 内：
   - 改 `CLAUDE.md`（追加 SOP 节）→ commit。
   - 改 `AGENTS.md`（同步 SOP 节）→ commit。
   - 改 `development/archive/KNOWN_ISSUES.md`（追加章节 14）→ commit。
   - 写 `development/worktree-workflow-artifacts/2026-07-18-test-isolation-known-issues.html` → commit。
4. `git merge --ff-only worktree/test-isolation-known-issues` → `git push origin allthecodes` → `git worktree remove` + `git branch -d`。

## 6. 验证依据

- 文档 diff 只追加不删改既有结构。
- KNOWN_ISSUES 新章节 ID 连续（section 14），ID 命名 `TESTISO-00x`。
- CLAUDE/AGENTS SOP 节标题一致，约束条数对齐 §4。
- artifact 含本计划路径、4 个 commit hash、改动文件清单。
- 主分支 ff-only 合并成功，无 `--no-ff` merge commit。

## 7. 非目标

- 不修复代码bug（已在历史 commit 完成）。
- 不引入 CI 改造、不拆测试 binary、不重构 PTY 套件。
- 不动 Per-Session Worktree Workflow 主流程文档。
