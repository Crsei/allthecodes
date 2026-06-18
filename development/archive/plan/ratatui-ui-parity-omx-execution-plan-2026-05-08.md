# Ratatui UI parity OMX/subagent execution plan

日期: 2026-05-08

目标: 基于 [`ratatui-ui-parity-untracked-gap-plan-2026-05-08.md`](ratatui-ui-parity-untracked-gap-plan-2026-05-08.md)
执行 Rust ratatui UI parity 缺口补齐。执行必须通过 OMX/Codex 批处理会话，固定
`gpt-5.5` 和 `medium` reasoning，并在每个批次后做 review、重构检测、验证和
commit gate。

本文档是执行计划；实际批处理入口为
[`scripts/run-ratatui-ui-parity-omx.ps1`](../../scripts/run-ratatui-ui-parity-omx.ps1)，
任务清单位于
[`docs/scripts/achieve/ratatui-ui-parity-omx-tasks.txt`](../scripts/achieve/ratatui-ui-parity-omx-tasks.txt)。

## 0. 固定执行契约

| 约束 | 要求 |
| --- | --- |
| 执行入口 | 使用 `scripts/run-ratatui-ui-parity-omx.ps1`，它调用 `scripts/codex-task-sequence.ps1` 并把 `-Codex` 固定为 `omx` |
| 模型 | `gpt-5.5`，脚本参数使用 `ValidateSet("gpt-5.5")` 防止误改 |
| 思考程度 | `medium`，脚本参数使用 `ValidateSet("medium")` 防止误改 |
| 输出风格 | 每个任务只返回 concise completion note；不要输出长推理过程 |
| 子代理 | 每个 Codex session 可按需使用最多 2 个 native subagents 做独立探索/审阅/验证；不得递归扩张 |
| 每批任务数量 | 动态批量：默认从 2 个任务开始，连续 2 个 green batch 后最多扩到 4；失败、超时或 diff churn 后缩小 |
| commit | 每个 green batch 自动 commit，review/final checkpoint 必须单独批次 |
| dirty worktree | 脚本启动时记录 baseline dirty paths，只提交启动后新增/修改的路径，避免卷入用户已有改动 |
| 错误报告 | 失败必须记录 `ERROR` / `BLOCKER` / `WARNING` / `PASS`，含命令、短输出、影响和下一步 |

## 1. 任务切分原则

1. 每个 task 只拥有一个 UI 领域或一个 review/closeout gate。
2. 单个 implementation task 默认不超过 3-5 个源文件；需要更多文件时拆任务。
3. `review`、`checkpoint`、`final` 标记任务强制单独 batch，不能和实现任务合并。
4. 任务 prompt 明确禁止 `git commit`；commit 由 wrapper gate 统一执行。
5. 执行 agent 只修改自己的 owned files；遇到共享模块需求，先添加最小 shared helper，再让后续任务使用。
6. 每个 task 必须运行与改动领域相关的最小测试；final gate 再运行 broader tests。

## 2. 动态批量策略

脚本规则：

1. `InitialBatchSize = 2`。
2. `MaxBatchSize = 4`。
3. `MinBatchSize = 1`。
4. 当前 task 是 `[checkpoint]`、`[review]` 或 `[final]` 时，batch size 强制为 1。
5. 连续 2 个 batch 满足以下条件后，batch size +1：
   - `omx exec` 退出码为 0；
   - diff guard 无 BLOCKER；
   - `git diff --check` 通过；
   - 自动 commit 成功或 `-SkipCommit` 明确禁用。
6. 出现失败、guard violation、commit failure 时，batch size 降到 `max(MinBatchSize, current - 1)`；
   默认停止执行，只有显式 `-ContinueOnError` 才继续。

## 3. Review 和重构检测 gate

每个 batch 完成后自动做：

1. `git diff --check -- <new paths>`。
2. `git diff --numstat HEAD -- <new paths>`。
3. 单文件变更超过 300 added+removed lines 时 BLOCK。
4. 单文件变更超过当前文件行数 25% 时 BLOCK。
5. 单个 `.rs` 文件超过默认 900 行时 BLOCK，要求拆分或写明保留理由。
6. 同一 batch 变更文件超过 8 个时 BLOCK。
7. rename/copy storm 或签名 churn 由 `git diff --name-status` 检测，出现 `R`/`C` 时 BLOCK。

Review checkpoint 额外要求：

1. 阅读上一批 `target/codex-runs/ratatui-ui-parity-omx/**/task-*.last-message.txt`。
2. 审查文件归属、测试证据、是否存在过度抽象或冗余安全层。
3. 将问题分成：
   - `BLOCKER`: 必须先修。
   - `WARNING`: 可进入下一批但需要跟踪。
   - `FOLLOW-UP`: 不阻塞本阶段，写入 closeout。
4. 若 review 发现需要修复，修复后单独 commit，不和下一批实现混合。

## 4. 错误和安全设计要求

本执行计划不追求“多包几层安全兜底”。要求是：

1. 配置、状态、解析、权限、持久化错误必须在最早边界可见。
2. 不允许把 present-but-invalid 状态静默降级成 empty/default。
3. 错误文案必须包含：
   - 失败对象；
   - 来源路径或 command；
   - 影响；
   - 建议下一步。
4. 如果需要 fallback，必须区分：
   - absent/unsupported；
   - present-invalid；
   - runtime-failed；
   - user-cancelled。
5. 不新增重复 validator 或 parallel safety layer；优先复用已有 config/permission/command API。

## 5. 执行阶段

### Phase 0. Baseline/status gate

无写任务。确认：

- `RATATUI_UI_PARITY.md`、本计划、untracked gap plan、command settings audit 都存在。
- `omx exec` 可通过 wrapper 调用。
- dirty worktree baseline 已记录。
- 输出目录存在。

### Phase 1. Shared primitives

任务：

- tabs/status icon/shortcut hint。
- selection preview/actions。

目标：

- 消除后续 UI surface 的重复 footer/tab/list 逻辑。
- 不做大规模迁移，只迁一个小 consumer 验证 API。

### Review A. Shared primitive review

检查：

- primitive 是否过度泛化。
- API 是否强迫消费者绕过本地业务状态。
- snapshot 是否覆盖空态/窄宽/disabled 状态。

### Phase 2. Settings / Sandbox / Permissions

任务：

- `/config` dashboard 扩展。
- `/sandbox` tabs 扩展。
- `/permissions` surface + Auto/Bypass safety dialogs。

目标：

- 让最常触达和风险最高的设置进入专门 UI。
- 错误和 managed-policy 状态明显，不被 default 吞掉。

### Review B. Settings and safety review

检查：

- mutation 是否复用已有 command/settings API。
- `.cc-rust` 路径隔离是否保持。
- auto/bypass/sandbox 权限文案是否足够清楚。

### Phase 3. Messages / Composer

任务：

- message selection/actions/timestamps/path/image references。
- compact/interrupted/error/rejected renderers。
- prompt mode indicator、placeholder、large paste、input truncation。

目标：

- 消息和输入不再只是文本流，而能承载可审计动作与清晰状态。

### Phase 4. Agents / Teams / Tasks

任务：

- `/agents` list/detail/create wizard 接线。
- agent tree/coordinator status/team cards/team summary。
- shell/bash progress detail。

目标：

- Agent/Team 执行状态能解释“谁在做什么、为什么阻塞、下一步是什么”。

### Review C. Cross-surface review

检查：

- 是否出现跨领域大改。
- 是否有文件超过体积阈值。
- 是否有重复安全或重复 renderer。
- 是否需要把某些功能拆到后续跟踪。

### Phase 5. Navigation / Session / Help

任务：

- persistent history、quick open、global search、log selector。
- session preview/export、exit flow。
- Help V2、diagnostics、keybinding warnings、onboarding。

目标：

- 用户能找到、恢复、导出和诊断已有状态。

### Phase 6. IDE / LSP / Chrome / Remote decision

任务：

- IDE picker/status surface。
- LSP server card。
- Chrome onboarding。
- remote/teleport 裁剪或接线决策。

目标：

- 能交付的集成进入 UI；不交付的集成写成 crop/deferred，不保留无归属缺口。

### Final. Verification and documentation closeout

必须保存：

1. 实现效果：完成了哪些 UI surface、用户如何触达、关键截图/snapshot。
2. 缺陷：仍失败的测试、未接线的后端、已知 UI 限制。
3. 后续跟踪方案：
   - 更新 `RATATUI_UI_PARITY.md` 状态；
   - 更新 `WORK_STATUS.md`；
   - 必要时更新 `IMPLEMENTATION_GAPS.md` / `KNOWN_ISSUES.md`；
   - 写入 `development/archive/ratatui-ui-parity-omx-execution-report-YYYY-MM-DD.md`。

## 6. 推荐运行命令

先 dry-run：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1 -DryRun
```

正式执行：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1
```

失败后继续后续 batch 仅用于收集更多失败信息：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1 -ContinueOnError
```

只运行任务但不自动提交：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1 -SkipCommit
```

## 7. 完成标准

1. 每个 batch 都有 `target/codex-runs/ratatui-ui-parity-omx/batch-XX.summary.md`
   和 `.json`。
2. 每个 green batch 都有 commit，且不包含启动前 dirty paths。
3. Review checkpoint 没有未关闭 `BLOCKER`。
4. Final closeout 写入实现效果、缺陷和后续跟踪方案。
5. `RATATUI_UI_PARITY.md` 中本阶段完成的 `⚠️/❌` 不再保持无归属状态。
