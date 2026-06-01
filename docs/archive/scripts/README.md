# scripts 使用说明

本文档根据 `scripts/` 下的实际脚本实现整理，说明每个可执行入口的用途、参数、任务文件格式、输出产物、失败处理和恢复方式。命令示例默认从仓库根目录 `F:\AIclassmanager\cc\rust` 执行。

## 快速选择

| 场景 | 推荐入口 |
| --- | --- |
| 顺序执行一组独立 Codex 任务 | `scripts/codex-task-sequence.ps1` |
| 跑当前待执行任务列表 | `scripts/run-standard-task-list-omx.ps1 -TasksFile docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt` |
| 跑通用标准任务列表批处理计划 | `scripts/run-standard-task-list-omx.ps1` |
| 观察标准任务列表已完成批次并只续跑未完成任务 | `scripts/standard_task_list_omx_supervisor.py` |
| 跑 workspace crate extraction 批处理计划 | `scripts/run-workspace-crate-extraction-omx.ps1` |
| 直接调试 workspace crate extraction Python runner | `scripts/workspace_crate_extraction_omx.py` |
| 观察已完成批次并只续跑未完成任务 | `scripts/workspace_crate_extraction_omx_supervisor.py` |
| 跑 ratatui UI parity 批处理计划 | `scripts/run-ratatui-ui-parity-omx.ps1` |
| 生成或校验 Rust TUI 快照 | `scripts/export-ui-snapshots.ps1` |
| 查看历史专项自动化脚本 | `scripts/achieve/` 和 `scripts/tmp/` |

## 当前任务清单

`docs/scripts/` 只保留仍需要执行或仍作为当前入口的任务清单。已经完成的清单放在 `docs/scripts/achieve/`，仅用于复现历史运行或阅读执行证据。

| 清单 | 状态 | 处理流程 |
| --- | --- | --- |
| `docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt` | 当前待执行任务列表；打包了非 workspace crate extraction 的未完成事项 | `scripts/standard_task_list_omx_py/` |
| `docs/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt` | workspace crate extraction 当前长任务清单 | `scripts/workspace_crate_extraction_omx_py/` |
| `docs/scripts/workspace-crate-extraction-omx-remaining-from-api-models-03.txt` | workspace crate extraction 历史续跑/诊断清单 | 只在明确需要从该点恢复时使用 |
| `docs/scripts/achieve/*.txt` | 已完成任务清单 | 历史复现，不作为当前待执行入口 |

当前非 workspace 待执行任务必须走通用标准任务列表流程，也就是 `scripts/standard_task_list_omx.py` 或 `scripts/standard_task_list_omx_supervisor.py`。不要再为它新增专项 PowerShell runner。

### OMX + subagent 标准执行计划

这套计划用于把 `docs/scripts/` 中仍待执行的任务构建成标准任务队列，并通过 OMX/Codex runner 小批量执行。目标是稳定推进长任务，而不是一次性把多个无关改动塞进同一个会话。

固定执行配置：

- 子任务输出保持简洁，不写长 reasoning transcript；只报告行动、改动文件、验证、风险和下一步。
- 底层执行优先走 `scripts/run-standard-task-list-omx.ps1` 或 `scripts/standard_task_list_omx_supervisor.py`；需要普通只读定位时可用 `omx sparkshell`、`omx explore` 或 `rg`。Windows 下 `omx explore` 不可用时，用 PowerShell + `rg` 等价定位即可。

任务队列构建：

1. 从 `docs/scripts/` 中保留的待执行清单开始，不从 `docs/scripts/achieve/` 重新取已完成任务。
2. 每行只放一个可验证任务；空行和 `#` 注释允许存在。
3. 高风险或边界任务使用 `[checkpoint]`、`[review]`、`[final]`，由标准 runner 强制拆成单任务 batch。
4. 普通 `[build]` 任务只在文件所有权清楚、风险低、验证相同的情况下才允许合并。
5. 当前非 workspace 待执行入口是 `docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt`；它把剩余事项打包成一个 `[final]` 任务，实际执行时仍由标准 runner 写 batch summary、execution report 和 final report。

按需分配每批任务数量：

- 初始 batch size 为 `1`。
- `[checkpoint]`、`[review]`、`[final]` 永远单独执行。
- 连续 green batch 后可以增加 batch size，最高不超过标准 runner 的 `--max-batch-size`。
- 只在任务改动域相同、测试命令相同、文件所有权不重叠时合并多个 `[build]` 任务。
- 出现失败、跨域改动、单文件 diff 过大、agent 报告 blocker 时，下一批收缩到单任务。

subagent 使用规则：

- 只在独立、边界清楚的定位、审查或验证工作上使用 native subagent。
- 实现任务由当前 batch 的主 agent 负责；subagent 不直接扩大 scope，不提交，不重写全局计划。
- 可用 subagent 让一个会话做只读代码定位，另一个做 diff/review 风险检查；二者结果由主 agent 整合。
- 中途 review gate 优先使用 `[review]` 任务承载；review 只修 blocker 或测试失败，不新增功能。

提交和历史：

- green batch 由标准 runner 统一 `git add` 当前 batch changed paths 并自动 commit。
- 子任务 agent 不直接 commit。
- commit message 遵守本仓库 Lore Commit Protocol，至少说明约束、验证和未测项。
- 如果运行时存在 unrelated dirty worktree，runner 和 agent 都不得 revert；只提交本 batch 实际变更。

重构和单文件代码量检测：

- 标准 runner 会检查 `--max-files-per-batch`、`--max-per-file-diff-lines`、`--warn-rust-file-lines`、`--max-rust-file-lines` 和 oversized Rust 文件数量。
- 触发 `WARNING` 或 `BLOCKER-RISK` 时，优先拆任务、抽小模块、删除重复代码或复用已有 helper。
- 不为了“安全”再叠一层重复 guard；优先让现有边界返回清晰错误。
- 单文件继续膨胀时，必须在当前任务范围内拆分，或在 batch summary 中说明为什么不能拆。

错误和诊断要求：

- 错误必须足够明显，方便后续 agent 从日志直接定位；不要把存在但损坏的配置、凭据、manifest 或状态文件静默当成不存在。
- 诊断至少包含可行动的 message、相关路径或命令、建议动作；能稳定编码的错误要保留 stable code。
- absent 和 invalid 要区分：确实缺失可以空结果；存在但不可读、不可解析、权限错误或 schema 错误必须显式暴露。
- 减少冗余安全设计：不要用多层模糊 fallback 掩盖真实失败；需要 fail closed 的路径给出明确 blocker，best-effort 路径给出 warning。

收尾产物：

- 每个 batch 保留 `batch-XX.summary.md/json` 和 `batch-XX/task-report.md/json`。
- 整次运行保留 `execution-report.md/json` 和 `final-report.md/json`。
- 完成后把实现效果、已知缺陷、未测项和后续跟踪方案写入对应 docs 或 `docs/archive/` 报告。
- 用户可感知问题写入 `docs/KNOWN_ISSUES.md`；只记录当前仍存在的问题，不保留已经修复的历史噪声。

## 通用约定

### 运行环境

多数脚本假定以下命令可用：

- `powershell`
- `git`
- `codex`
- `python` 或 `py -3`
- `cargo`
- `omx`，仅部分历史或 OMX 专项脚本需要

PowerShell wrapper 会把相对路径解析到仓库根目录下。建议始终从仓库根目录运行，避免输出目录和任务文件位置混乱。

### 任务文件格式

任务文件采用“一行一个任务”的格式：

```text
# comment
[checkpoint] read plan and verify baseline
[build] implement one bounded change
[review] inspect diff and fix blockers only
[final] run final verification and update docs
```

规则：

- 空行会被忽略。
- 以 `#` 开头的行会被忽略。
- `standard task list`、`workspace crate extraction` 和 `ratatui UI parity` wrapper 会把以 `[checkpoint]`、`[review]`、`[final]` 开头的任务强制拆成单任务 batch。
- 普通 `[build]` 任务可按 batch size 合并执行。

### 输出目录

默认输出都写在 `target/codex-runs/` 下。这个目录用于保存 runner 产物、最后一条 Codex 消息、批次总结、失败记录和最终验证日志。

常见产物：

- `task-XX.last-message.txt`：单个 Codex 任务的最终回复。
- `task-XX.summary.json`：单个 Codex 任务的状态、退出码、改动文件、中间输出路径和中断原因。
- `task-report.md` / `task-report.json`：底层任务序列器在每个 task 结束后刷新的 task 级滚动报告。
- `batch-XX.tasks.txt`：实际传给底层 runner 的任务文件，通常会加上全局约束。
- `batch-XX/`：某个 batch 内每个任务的 last-message 文件。
- `batch-XX.summary.md` / `batch-XX.summary.json`：batch 状态、任务、改动文件、guard 结果、诊断摘要。
- `failures.md` / `failures.jsonl`：失败 batch 的诊断记录。
- `execution-report.md` / `execution-report.json`：Python batch wrapper 的 run-level task 汇总，聚合每个 task 的结果、改动文件、输出位置和中断原因。
- `final-validation/`：最终 `cargo` 验证日志。
- `final-report.md` / `final-report.json`：整次运行总结。

## `codex-task-sequence.ps1`

路径：`scripts/codex-task-sequence.ps1`

这是最底层的通用任务序列器。它按顺序读取任务列表，并为每个任务启动一个新的 `codex exec` 会话。每个会话只处理当前任务，不会自动继续到后续任务。

### 典型命令

从任务文件运行：

```powershell
.\scripts\codex-task-sequence.ps1 `
  -TasksFile .\docs\scripts\achieve\p1-defensive-fail-fast-after-phase1-tasks.txt `
  -WorkDir . `
  -OutputDir .\target\codex-runs\p1-defensive-after-phase1 `
  -Sandbox danger-full-access `
  -Model "gpt-5.5" `
  -ReasoningEffort "medium"
```

直接传入内联任务：

```powershell
.\scripts\codex-task-sequence.ps1 `
  -Tasks @(
    "Task 1 - inspect current behavior"
    "Task 2 - patch only the narrow failure"
    "Task 3 - run focused verification"
  ) `
  -Model "gpt-5.5" `
  -ReasoningEffort "medium"
```

### 参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `-Tasks` | `@()` | 内联任务数组；只要非空，就优先于 `-TasksFile`。 |
| `-TasksFile` | `codex-tasks.txt` | 任务文件路径。相对路径按仓库根目录解析。 |
| `-Codex` | `codex` | 要调用的 Codex 可执行文件；也可传 `omx` 作为兼容 transport。 |
| `-WorkDir` | 当前目录 | 传给 `codex exec --cd` 的工作目录。 |
| `-Model` | 空 | 非空时传给 `codex exec --model`。 |
| `-ReasoningEffort` | 空 | 非空时传 `--config model_reasoning_effort="..."`，可选 `minimal`、`low`、`medium`、`high`、`xhigh`。 |
| `-Profile` | 空 | 非空时传给 `codex exec --profile`。 |
| `-Sandbox` | `danger-full-access` | 传给 `codex exec --sandbox`。 |
| `-OutputDir` | `target/codex-runs` | 保存 `task-XX.last-message.txt` 的目录。 |
| `-TaskNumberOffset` | `0` | batch wrapper 传入的全局任务序号偏移，用来显示真实任务进度。 |
| `-TotalTaskCount` | `0` | batch wrapper 传入的全局任务总数；为 `0` 时使用当前任务文件数量。 |
| `-ContinueOnError` | 关闭 | 某个任务失败后继续运行后续任务。 |

### 执行行为

每个任务都会被包装成如下结构后送入 `codex exec`：

```text
You are executing task N of M.
Work only on this task. Do not continue to later tasks.

Task:
<task text>

Return a concise completion note with changed files, verification run, and any remaining risk.
```

报告策略：

- 每个任务结束后写 `task-XX.summary.json`。
- 每个任务结束后刷新 `task-report.md` / `task-report.json`。
- 报告包含 task 文本、状态、退出码、changed files、last-message 路径、输出目录和中断原因。

失败策略：

- 默认遇到非零退出码立即停止。
- 启用 `-ContinueOnError` 后继续执行后续任务。
- 每个任务的最终回复写入 `<OutputDir>\task-XX.last-message.txt`。

## `run-standard-task-list-omx.ps1`

路径：`scripts/run-standard-task-list-omx.ps1`

这是通用标准任务列表的批处理入口，是从 workspace crate extraction Python runner 复制并泛化出来的版本。它适合新的长任务清单，默认读取 `codex-tasks.txt`，输出到 `target/codex-runs/standard-task-list-omx`。

常用命令：

```powershell
.\scripts\run-standard-task-list-omx.ps1 -TasksFile .\codex-tasks.txt -DryRun
.\scripts\run-standard-task-list-omx.ps1 -TasksFile .\codex-tasks.txt
python .\scripts\standard_task_list_omx_supervisor.py --tasks-file .\codex-tasks.txt --plan-only
python .\scripts\standard_task_list_omx_supervisor.py --tasks-file .\codex-tasks.txt -- --skip-final-validation
```

它保留动态 batch、`[checkpoint]` / `[review]` / `[final]` 单独成批、diff guard、blocker-risk review、green batch 自动提交、final report 和 supervisor 续跑能力。需要跳过自动提交时传 `-SkipCommit`；需要避免最终 `cargo` 长验证时传 `-SkipFinalValidation`。

### 当前待执行任务列表

当前待执行任务列表是：

```text
docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt
```

它已经按 `standard_task_list_omx_py.tasks.load_task_list()` 的格式整理为一行一个任务，注释行以 `#` 开头，并使用 `[final]` 前缀强制成为单任务 batch。这样 supervisor 能按 batch summary 判断完成状态，失败后也能重新生成 remaining task 文件。

预览 batch，不启动 Codex：

```powershell
.\scripts\run-standard-task-list-omx.ps1 `
  -TasksFile .\docs\scripts\non-workspace-unfinished-standard-task-2026-05-11.txt `
  -OutputRoot .\target\codex-runs\non-workspace-unfinished-standard-task `
  -DryRun
```

只生成完成度和剩余任务文件：

```powershell
python .\scripts\standard_task_list_omx_supervisor.py `
  --tasks-file docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt `
  --observed-output-root target/codex-runs/non-workspace-unfinished-standard-task `
  --supervisor-output-root target/codex-runs/non-workspace-unfinished-standard-task-supervisor `
  --plan-only
```

执行或续跑剩余任务：

```powershell
python .\scripts\standard_task_list_omx_supervisor.py `
  --tasks-file docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt `
  --observed-output-root target/codex-runs/non-workspace-unfinished-standard-task `
  --supervisor-output-root target/codex-runs/non-workspace-unfinished-standard-task-supervisor `
  -- --skip-final-validation
```

如果需要完整 release gate，移除最后一行的 `--skip-final-validation`。如果只是验证任务清单仍能进入标准流程，使用 `-DryRun` 和 `--plan-only`，不要直接启动 Codex 执行。

## `run-workspace-crate-extraction-omx.ps1`

路径：`scripts/run-workspace-crate-extraction-omx.ps1`

这是 workspace crate extraction 计划的 PowerShell 入口。当前实现是 Python runner 的兼容 wrapper，实际逻辑在 `scripts/workspace_crate_extraction_omx.py` 和 `scripts/workspace_crate_extraction_omx_py/` 中。

它负责：

- 读取 workspace crate extraction 任务文件。
- 动态组 batch。
- 调用 `codex-task-sequence.ps1` 执行每个 batch。
- 根据 git diff 和 agent last-message 执行 guard。
- 为通过的 batch 自动提交。
- 记录 batch summary、failure log 和最终 run report。
- 在未跳过时运行最终 `cargo fmt/check/clippy/test` 验证。

### 典型命令

预览会执行哪些 batch，不启动 Codex：

```powershell
.\scripts\run-workspace-crate-extraction-omx.ps1 -DryRun
```

正式执行：

```powershell
.\scripts\run-workspace-crate-extraction-omx.ps1
```

执行但不自动提交：

```powershell
.\scripts\run-workspace-crate-extraction-omx.ps1 -SkipCommit
```

失败后继续收集更多诊断：

```powershell
.\scripts\run-workspace-crate-extraction-omx.ps1 -ContinueOnError
```

跳过最终长验证：

```powershell
.\scripts\run-workspace-crate-extraction-omx.ps1 -SkipFinalValidation
```

### 主要参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `-TasksFile` | `docs/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt` | 主任务清单。 |
| `-Runner` | `scripts/codex-task-sequence.ps1` | 底层任务序列器。 |
| `-Codex` | `codex` | 底层命令。 |
| `-WorkDir` | `.` | Codex 工作目录。 |
| `-OutputRoot` | `target/codex-runs/workspace-crate-extraction-omx` | runner 总输出目录。 |
| `-Model` | `gpt-5.5` | 固定为 `gpt-5.5`。 |
| `-ReasoningEffort` | `medium` | 固定为 `medium`。 |
| `-Sandbox` | `danger-full-access` | Codex sandbox。 |
| `-InitialBatchSize` | `1` | 初始 batch 大小。 |
| `-MinBatchSize` | `1` | 最小 batch 大小。 |
| `-MaxBatchSize` | `3` | 最大 batch 大小。 |
| `-WarnRustFileLines` | `500` | Rust 文件行数 warning 阈值。 |
| `-MaxRustFileLines` | `2000` | Rust 文件行数 blocker-risk 阈值。 |
| `-MaxFilesPerBatch` | `12` | 单 batch 改动文件数 warning 阈值。 |
| `-MaxPerFileDiffLines` | `600` | 单文件 diff 行数 warning 阈值。 |
| `-MaxOversizedRustFilesBeforeBlocker` | `3` | 单 batch 中超过 `-MaxRustFileLines` 的 Rust 文件达到该数量时标记 `BLOCKER-RISK`。 |
| `-BlockerReviewSandbox` | `read-only` | blocker-risk review agent 使用的 sandbox。 |
| `-BlockerReviewTimeoutSeconds` | `900` | blocker-risk review agent 超时时间。 |
| `-CommitBaselineDirtyChanges` | `$true` | 启动前已 dirty 的文件如果在 batch 中发生内容变化，也纳入 commit 候选。 |
| `-ContinueOnError` | 关闭 | batch 失败后继续。 |
| `-SkipCommit` | 关闭 | 不自动 commit。 |
| `-SkipBlockerReview` | 关闭 | 不启动只读 blocker-risk review agent。 |
| `-SkipFinalValidation` | 关闭 | 不跑最终 `cargo` 验证。 |
| `-DryRun` | 关闭 | 只打印计划，不执行。 |

### 直接运行 Python runner

PowerShell wrapper 会查找 `python`，找不到时回退到 `py -3`。调试时可以直接调用 Python 入口：

```powershell
python .\scripts\workspace_crate_extraction_omx.py --dry-run
```

常用 Python 参数与 PowerShell 参数一一对应：

```powershell
python .\scripts\workspace_crate_extraction_omx.py `
  --tasks-file docs/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt `
  --output-root target/codex-runs/workspace-crate-extraction-omx `
  --skip-commit `
  --skip-final-validation
```

baseline dirty 策略使用互斥参数：

```powershell
python .\scripts\workspace_crate_extraction_omx.py --commit-baseline-dirty-changes
python .\scripts\workspace_crate_extraction_omx.py --no-commit-baseline-dirty-changes
```

### batch 策略

runner 会维护动态 batch size：

- 初始值为 `InitialBatchSize`。
- `[checkpoint]`、`[review]`、`[final]` 任务总是单独一个 batch。
- 连续两个干净 batch 后，如果未超过 `MaxBatchSize`，batch size 增加 1。
- batch 失败后，batch size 减少 1，但不会低于 `MinBatchSize`。
- 默认失败即停止；`-ContinueOnError` 会继续后续 batch。

### guard 规则

workspace crate extraction runner 会检查：

- 改动文件数是否超过 `MaxFilesPerBatch`，超过记为 `WARNING`。
- rename/copy 条目是否超过 5，超过记为 `BLOCKER-RISK`，不中断运行。
- Rust 文件是否超过 `WarnRustFileLines` 或 `MaxRustFileLines`。
- 如果 Rust 文件在 HEAD 中已经超过最大行数，但本 batch 没有继续增长，只记为 `WARNING`。
- 单 batch 中超过 `MaxRustFileLines` 的 Rust 文件达到 `MaxOversizedRustFilesBeforeBlocker` 时，记为 `BLOCKER-RISK`，不中断运行。
- 单文件 diff 行数是否超过 `MaxPerFileDiffLines`，超过记为 `WARNING`。
- `task-XX.last-message.txt` 中 agent 明确报告的 `ERROR`、`BLOCKER`、`WARNING`。

`BLOCKER-RISK` 是限制风险标记，不是硬失败。runner 会记录“blocker risk starts at task: ...”，并在未启用 `-SkipBlockerReview` 时启动只读 review agent。review 返回 `BLOCKER_REVIEW: WAIVE` 时会改写为 `WAIVED-BLOCKER-RISK`；返回 `KEEP` 时保留风险标记。两种情况都不会中断后续 batch。

状态含义：

- `PASS`：runner 退出码为 0，且没有 guard finding。
- `WARNING`：runner 退出码为 0，有 warning 或 `BLOCKER-RISK`，但不阻塞完成。
- `BLOCKER`：runner 退出码为 0，但 agent 报告 blocker/error。
- `ERROR`：底层 runner 非零退出。

### 自动提交

当 batch 状态不是 `ERROR` 或 `BLOCKER`，且未启用 `-SkipCommit` 时，runner 会：

1. 根据本 batch 的改动文件集合执行 `git add -- <paths>`。
2. 如果 staged diff 非空，生成 `batch-XX.commit-message.txt`。
3. 用 Lore Commit Protocol 生成 commit message。
4. 执行 `git commit -F <message file>`。

注意：

- runner 不会主动 revert baseline dirty 文件。
- 默认 `CommitBaselineDirtyChanges=$true`，只要 baseline dirty 文件内容在 batch 中变化，就会纳入 batch changed paths。
- 如果要严格排除启动前已 dirty 文件，传 `-CommitBaselineDirtyChanges:$false` 或 Python 的 `--no-commit-baseline-dirty-changes`。

### 最终验证

未启用 `-SkipFinalValidation` 时，runner 会按顺序运行：

```powershell
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

日志写入：

```text
target/codex-runs/workspace-crate-extraction-omx/final-validation/
```

任一最终验证失败会使整体退出码变为 1，并写入 `final-report.md/json`。

### 执行报告

workspace crate extraction 有两层报告：

- `batch-XX/task-report.md/json`：底层 `codex-task-sequence.ps1` 每个 task 结束后立即刷新，适合查看当前 batch 内每个 task 的文件改动、执行结果、中间输出和中断原因。
- `execution-report.md/json`：Python runner 在每个 batch 结束、runner 内部中断、最终验证结束时刷新，聚合所有 `batch-*/task-*.summary.json`。

常用查看命令：

```powershell
Get-Content .\target\codex-runs\workspace-crate-extraction-omx\execution-report.md
Get-Content .\target\codex-runs\workspace-crate-extraction-omx\batch-01\task-report.md
```

`final-report.md` 会链接到 `execution-report.md/json`。如果运行中断，先看 `final-report.md` 的 stop reason，再看 `execution-report.md` 中最后一个非 PASS task 的 `interruption_reason` 和 last-message 路径。

## `workspace_crate_extraction_omx_supervisor.py`

路径：`scripts/workspace_crate_extraction_omx_supervisor.py`

supervisor 是 workspace crate extraction runner 的续跑控制层。它不会直接修改 Rust 功能代码；它读取已有 runner 产物，判断哪些任务已经完成，生成剩余任务文件，然后只把剩余任务交给 Python runner。

### 只生成状态和剩余任务

```powershell
python .\scripts\workspace_crate_extraction_omx_supervisor.py --plan-only
```

输出：

```text
target/codex-runs/workspace-crate-extraction-omx-supervisor/supervisor-status.md
target/codex-runs/workspace-crate-extraction-omx-supervisor/supervisor-status.json
target/codex-runs/workspace-crate-extraction-omx-supervisor/<timestamp>/remaining-attempt-01.tasks.txt
```

### 续跑未完成任务

```powershell
python .\scripts\workspace_crate_extraction_omx_supervisor.py -- --skip-final-validation
```

`--` 之后的参数会传给 `scripts/workspace_crate_extraction_omx.py`。supervisor 会自动覆盖 runner 的 `--tasks-file` 和 `--output-root`，避免误跑完整任务清单。

### 从指定任务开始

从匹配任务开始：

```powershell
python .\scripts\workspace_crate_extraction_omx_supervisor.py `
  --start-at-task "api-models-03" `
  --plan-only
```

从匹配任务之后开始：

```powershell
python .\scripts\workspace_crate_extraction_omx_supervisor.py `
  --start-after-task "api-models-02" `
  -- --skip-final-validation
```

`--start-at-task` 和 `--start-after-task` 互斥。匹配逻辑会先规范化空白和大小写，并允许传入任务文本片段。

### supervisor 参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--tasks-file` | `docs/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt` | 全量任务清单。 |
| `--runner-script` | `scripts/workspace_crate_extraction_omx.py` | 被调用的 Python runner。 |
| `--observed-output-root` | 可重复，默认观察 `target/codex-runs/workspace-crate-extraction-omx` | 读取历史 batch summary 的目录。 |
| `--supervisor-output-root` | `target/codex-runs/workspace-crate-extraction-omx-supervisor` | supervisor 自己的状态、日志和剩余任务目录。 |
| `--max-attempts` | `3` | 最多执行几轮 runner 尝试；必须大于 0。 |
| `--start-at-task` | 空 | 从匹配任务开始计算 remaining。 |
| `--start-after-task` | 空 | 从匹配任务之后计算 remaining。 |
| `--no-trust-resolution-files` | 关闭 | 不把 `batch-XX.resolution.md` 视为失败 batch 的人工解决证明。 |
| `--plan-only` | 关闭 | 只写状态，不执行 runner。 |
| `--repair` | 关闭 | 当失败看起来是 runner/脚本自身故障时，允许启动一次受限 Codex 修复会话。 |
| `--repair-model` | `gpt-5.5` | repair Codex 会话模型。 |
| `--repair-reasoning-effort` | `medium` | repair Codex 会话 reasoning effort。 |
| `--repair-timeout-seconds` | `900` | repair 会话超时时间。 |
| `--codex` | `codex` | repair 使用的 Codex 可执行文件。 |
| `runner_args` | 空 | `--` 之后传给 Python runner 的剩余参数。 |

### 完成判断

supervisor 会扫描所有观察目录中的 `batch-*.summary.json`：

- `PASS` 算完成。
- `WARNING` 算完成。
- `BLOCKER` / `ERROR` 不算完成。
- 如果存在同名 `batch-XX.resolution.md`，且未启用 `--no-trust-resolution-files`，则该 batch 视为已人工解决。

例如：

```text
target/codex-runs/workspace-crate-extraction-omx/batch-07.summary.json
target/codex-runs/workspace-crate-extraction-omx/batch-07.resolution.md
```

上面的 `resolution.md` 可以让 supervisor 把原本失败的 batch 视为已解决。

### runner 故障与任务失败的区别

supervisor 会尝试区分两类失败：

- 任务失败：例如 batch summary 已写出，状态为 `BLOCKER` 或 `ERROR`。此时应读 `supervisor-status.md` 和对应 batch summary。guard 限制类风险会写成 `BLOCKER-RISK` 并继续运行。
- runner 自身失败：例如 Python traceback、PowerShell 参数绑定错误、runner 未写出任何 summary、命令不可识别等。

只有第二类失败在启用 `--repair` 时才会触发 Codex repair 会话。repair prompt 明确限制只改 `scripts/`、`docs/scripts/`、测试和编排文档，不改 Rust 功能代码。

## `run-ratatui-ui-parity-omx.ps1`

路径：`scripts/run-ratatui-ui-parity-omx.ps1`

这是 ratatui UI parity 任务计划的批处理入口。它使用 `codex-task-sequence.ps1` 作为底层 transport，默认通过 `omx` 命令调用。

### 典型命令

预览：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1 -DryRun
```

执行：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1
```

执行但不提交：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1 -SkipCommit
```

如果当前环境没有 `omx`，可以把底层命令切到 `codex`：

```powershell
.\scripts\run-ratatui-ui-parity-omx.ps1 -Omx codex
```

### 参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `-TasksFile` | `docs/scripts/achieve/ratatui-ui-parity-omx-tasks.txt` | ratatui UI parity 已归档任务清单。 |
| `-Runner` | `scripts/codex-task-sequence.ps1` | 底层任务序列器。 |
| `-Omx` | `omx` | 实际传给 `codex-task-sequence.ps1 -Codex` 的命令。 |
| `-WorkDir` | `.` | 工作目录。 |
| `-OutputRoot` | `target/codex-runs/ratatui-ui-parity-omx` | 输出目录。 |
| `-Model` | `gpt-5.5` | 固定为 `gpt-5.5`。 |
| `-ReasoningEffort` | `medium` | 固定为 `medium`。 |
| `-Sandbox` | `danger-full-access` | 底层 Codex sandbox。 |
| `-InitialBatchSize` | `1` | 初始 batch 大小。 |
| `-MinBatchSize` | `1` | 最小 batch 大小。 |
| `-MaxBatchSize` | `1` | 最大 batch 大小；默认每个 batch 一个任务。 |
| `-ContinueOnError` | 关闭 | 失败后继续。 |
| `-SkipCommit` | 关闭 | 不自动 commit。 |
| `-DryRun` | 关闭 | 只打印 batch，不执行。 |

### guard 规则

ratatui UI parity wrapper 使用 PowerShell helper 中的默认 guard：

- 单 batch 改动超过 8 个文件：`BLOCKER`。
- 单文件 changed lines 超过 300：`BLOCKER`。
- 单文件改动比例超过当前文件 25%：`BLOCKER`。
- rename/copy 条目数量大于 0：`BLOCKER`。
- Rust 文件超过 900 行：`BLOCKER`。

这些规则比 workspace crate extraction runner 更严格，适合 UI parity 的小步迁移。

### 自动提交

通过的 batch 会调用 `scripts/ratatui-ui-parity-omx/git-commit.ps1` 中的 `Commit-Batch`：

- 只 stage runner 启动后新增的 changed paths。
- baseline dirty paths 不会被提交。
- commit message 使用 Lore Commit Protocol。
- commit message 写入 `batch-XX.commit-message.txt` 后执行 `git commit -F`。

## `export-ui-snapshots.ps1`

路径：`scripts/export-ui-snapshots.ps1`

用于生成或校验 Rust TUI snapshot。它先运行 UI 测试，再调用 `claude-code-rs` 的 `--export-ui-snapshots`。

### 更新快照

```powershell
.\scripts\export-ui-snapshots.ps1
```

默认行为：

1. 设置进程级 `INSTA_UPDATE=always`。
2. 运行 `cargo test -p claude-code-rs ui::`。
3. 运行 `cargo run -p claude-code-rs -- --export-ui-snapshots <OutputDir>`。
4. 默认输出到 `target/ui-snapshots`。

### 只校验，不更新 insta snapshots

```powershell
.\scripts\export-ui-snapshots.ps1 -CheckOnly
```

`-CheckOnly` 会把 `INSTA_UPDATE` 设置为 `no`。

### 跳过测试，只导出

```powershell
.\scripts\export-ui-snapshots.ps1 -SkipTests
```

### 参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `-OutputDir` | `target/ui-snapshots` | 导出目录；相对路径按仓库根目录解析。 |
| `-CheckOnly` | 关闭 | 测试阶段设置 `INSTA_UPDATE=no`。 |
| `-SkipTests` | 关闭 | 不运行 `cargo test -p claude-code-rs ui::`。 |

## 内部 helper 目录

以下目录主要由 wrapper 自动加载，不建议作为用户入口直接执行。

### `scripts/standard_task_list_omx_py/`

通用标准任务列表 runner 的 Python 实现模块，是新任务清单的默认模板。入口文件：

- `scripts/run-standard-task-list-omx.ps1`
- `scripts/standard_task_list_omx.py`
- `scripts/standard_task_list_omx_supervisor.py`

配套测试：

```text
scripts/tests/test_standard_task_list_omx.py
```

### `scripts/workspace_crate_extraction_omx_py/`

Python runner 的实现模块：

- `cli.py`：参数解析、batch 主循环、调用底层 PowerShell runner。
- `tasks.py`：任务文件读取、checkpoint/review/final 单 batch 规则。
- `git_state.py`：baseline dirty 签名和 batch changed paths 计算。
- `guards.py`：文件数、Rust 行数、diff 行数、agent last-message finding 检查。
- `blocker_review.py`：只读 blocker-risk review agent 调用和风险 finding 改写。
- `artifacts.py`：batch summary 和 failure log 写入。
- `commit.py`：batch 自动提交。
- `validation.py`：最终 `cargo` 验证和 run report。
- `supervisor.py`：续跑、remaining task 生成、runner failure 分类、repair。
- `common.py`：路径、时间、git 输出和文本写入工具。

配套测试在：

```text
scripts/tests/test_workspace_crate_extraction_omx.py
```

### `scripts/workspace-crate-extraction-omx/`

早期 PowerShell helper 实现。当前主入口已改为 Python runner，但这些 helper 仍保留用于兼容和参考：

- `tasks.ps1`
- `git-state.ps1`
- `runner.ps1`
- `guards.ps1`
- `artifacts.ps1`
- `git-commit.ps1`
- `validation-report.ps1`
- `include.ps1`

### `scripts/ratatui-ui-parity-omx/`

ratatui UI parity wrapper 的 PowerShell helper：

- `tasks.ps1`：路径解析、任务读取、checkpoint 判断。
- `git-guards.ps1`：差异 guard。
- `runner.ps1`：调用底层 PowerShell runner 并规范退出码。
- `batch-artifacts.ps1`：batch summary 写入。
- `git-commit.ps1`：batch commit。
- `include.ps1`：加载上述 helper。

## 历史和临时专项脚本

这些脚本都是围绕 `codex-task-sequence.ps1` 的专项会话编排器，适合复现历史计划，不建议作为新任务的默认模板。

### `scripts/achieve/run-remote-control-gateway-omx-sessions.ps1`

远程控制 gateway 计划的历史 session runner。

常用命令：

```powershell
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -DryRun
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -StartAt 10 -EndAt 12
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -OnlySession "Security"
```

特点：

- 默认要求 `omx --version` 通过。
- `-SkipOmxPreflight` 可跳过 OMX 预检。
- `-StartAt` / `-EndAt` 按 session id 选择范围。
- `-OnlySession` 可按 id 或名称片段选择。
- `-MaxSessions` 限制最多执行几个 session。
- `-DryRun` 只打印选中的 session。

### `scripts/achieve/run-p1-followup-sessions.ps1`

P1 follow-up 历史计划 runner。

```powershell
.\scripts\achieve\run-p1-followup-sessions.ps1 -StartAt 3
```

参数：

- `-Codex`：默认 `codex`。
- `-StartAt`：0 到 7。
- `-ContinueOnError`：失败后继续。

### `scripts/achieve/run-p1-review-b-residual-sessions.ps1`

P1 review B residual 历史计划 runner。

```powershell
.\scripts\achieve\run-p1-review-b-residual-sessions.ps1 -StartAt 2 -Model gpt-5.5
```

参数：

- `-Codex`：默认 `codex`。
- `-Model`：默认 `gpt-5.5`。
- `-StartAt`：0 到 4。
- `-ContinueOnError`：失败后继续。

### `scripts/tmp/run-generic-selectable-command-surface-sessions.ps1`

generic selectable command surface 临时计划 runner。

```powershell
.\scripts\tmp\run-generic-selectable-command-surface-sessions.ps1 -StartAt 1
```

参数：

- `-Codex`：默认 `codex`。
- `-Model`：默认空；非空时传给底层 runner。
- `-StartAt`：0 到 5。
- `-ContinueOnError`：失败后继续。

## 排障流程

### 底层任务失败

先看当前 batch summary：

```powershell
Get-Content .\target\codex-runs\<lane>\batch-XX.summary.md
```

再看对应 last-message：

```powershell
Get-Content .\target\codex-runs\<lane>\batch-XX\task-01.last-message.txt
```

如果是 workspace crate extraction runner，还要看：

```powershell
Get-Content .\target\codex-runs\workspace-crate-extraction-omx\failures.md
Get-Content .\target\codex-runs\workspace-crate-extraction-omx\final-report.md
```

### 只想知道还剩哪些任务

通用标准任务列表当前待执行队列：

```powershell
python .\scripts\standard_task_list_omx_supervisor.py `
  --tasks-file docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt `
  --observed-output-root target/codex-runs/non-workspace-unfinished-standard-task `
  --supervisor-output-root target/codex-runs/non-workspace-unfinished-standard-task-supervisor `
  --plan-only
```

然后读取：

```powershell
Get-Content .\target\codex-runs\non-workspace-unfinished-standard-task-supervisor\supervisor-status.md
```

workspace crate extraction 队列：

```powershell
python .\scripts\workspace_crate_extraction_omx_supervisor.py --plan-only
```

然后读取：

```powershell
Get-Content .\target\codex-runs\workspace-crate-extraction-omx-supervisor\supervisor-status.md
```

### runner 脚本自身崩溃

如果 supervisor 看到 traceback、参数绑定错误、runner 没有写出 summary 等情况，可以用：

```powershell
python .\scripts\workspace_crate_extraction_omx_supervisor.py --repair -- --skip-final-validation
```

只在确认问题属于 orchestration 层时使用。任务实现失败、agent blocker、Rust 编译失败都不应该用 `--repair` 绕过；guard 限制类风险会用 `BLOCKER-RISK` 标记并继续运行。

### PowerShell 找不到脚本或路径

确认从仓库根目录运行：

```powershell
pwd
Test-Path .\scripts\codex-task-sequence.ps1
Test-Path .\docs\scripts
```

如果 PowerShell 执行策略阻止脚本运行，可以使用脚本中同样采用的形式：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\codex-task-sequence.ps1 -TasksFile .\codex-tasks.txt
```

## 维护脚本时的检查清单

修改 `scripts/` 后建议至少执行：

```powershell
python -m unittest scripts.tests.test_workspace_crate_extraction_omx scripts.tests.test_standard_task_list_omx
git diff --check
```

如果修改影响实际 Rust 构建或最终验证流程，再按影响范围运行：

```powershell
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

如果只改文档，可以至少检查关键命令和路径是否仍存在：

```powershell
Test-Path .\scripts\codex-task-sequence.ps1
Test-Path .\scripts\run-standard-task-list-omx.ps1
Test-Path .\scripts\standard_task_list_omx.py
Test-Path .\scripts\standard_task_list_omx_supervisor.py
Test-Path .\scripts\run-workspace-crate-extraction-omx.ps1
Test-Path .\scripts\workspace_crate_extraction_omx.py
Test-Path .\scripts\workspace_crate_extraction_omx_supervisor.py
Test-Path .\scripts\run-ratatui-ui-parity-omx.ps1
Test-Path .\scripts\export-ui-snapshots.ps1
```
