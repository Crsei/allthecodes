# Per-Session Worktree 工作流规范（计划文件）

> 本文件是工作流规范的「源头」。按本规范执行任务前，先把本文件提交到主分支 `allthecodes`，作为每次 session 在 worktree 内可以拉取引用的来源。本文件本身不进 worktree。

生效日期：2026-07-16
作用范围：`allthecodes` 仓库所有修改 `CLAUDE.md` / `AGENTS.md` 以及按本规范执行的任务。

## 1. 目标

让每次任务在独立的 git worktree 中完成，避免与主分支上的并行任务相互污染；除「对应任务的计划文件」必须先同步到主分支外，任务期间的代码、文档、产物都在 worktree 内改、在 worktree 内 commit；完成后 fast-forward 合并回主分支 `allthecodes`，并删除该 worktree 与其分支。

## 2. 适用对象

- 任何在 `allthecodes` 仓库中需要同时改动 `CLAUDE.md` / `AGENTS.md` / 代码 / 文档 / 产物的任务。
- 计划文件本身（即本文件及后续同类计划）按 §3 的「主分支前置」流程处理。

## 3. 任务流程

每个 session 按下列步骤执行，缺一不可：

1. **主分支前置（只针对计划文件）**
   - 在主分支 `allthecodes`（不进 worktree）新建/修改本次任务的**计划文件**（位于 `development/workflow/<task>-plan.md` 或既有 `development/<域>/<...-plan.md>`）。
   - 只对计划文件做 `git add` + `commit`，绝不碰 working tree 中其它未提交改动。
   - 计划文件是后续 worktree 拉取的引用源头；它在主分支落地后才能进入下一步。

2. **创建独立 worktree**
   - 从主分支 `allthecodes` 当前 HEAD 起，`git worktree add` 一条独立 worktree 与分支：
     ```bash
     cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes
     git worktree add -b worktree/<task-slug> \
       .worktrees/<task-slug> allthecodes
     ```
   - worktree 目录统一放在 `.worktrees/<task-slug>`，分支统一以 `worktree/<task-slug>` 命名。
   - 进入 worktree 后再做任何改动。

3. **在 worktree 内完成改动**
   - 所有代码 / 文档 / 产物的修改、`cargo` 编译、测试、`git add`、`git commit` 都在 worktree 内进行。
   - 严禁回到主分支 working tree 改动会污染并行任务的内容；若必须临时回到主分支核对，只读，不写、不 commit。

4. **构建 HTML artifact**
   - 在 worktree 内创建本次任务的 HTML artifact，路径固定为：
     `development/worktree-workflow-artifacts/<YYYY-MM-DD>-<task-slug>.html`
   - artifact 必须能独立打开阅读，包含：任务目标、流程步骤、本次改动列表、对应的计划文件路径、本次 commit 列表、验证依据（编译/测试结果或截图描述）。
   - artifact 跟随 worktree 的提交一起合并到主分支。

5. **fast-forward 合并回主分支**
   - 在 worktree 内做完所有 commit（含 artifact）后，切回主分支：
     ```bash
     cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes
     git merge --ff-only worktree/<task-slug>
     ```
   - 只允许 fast-forward；如果无法 fast-forward，说明主分支在 task 进行期间被别人推进了，需要先解决再说，绝不产生不必要的 merge commit。
   - 合并后推送主分支：
     ```bash
     git push origin allthecodes
     ```

6. **删除 worktree 与分支**
   - 合并成功后清理：
     ```bash
     git worktree remove .worktrees/<task-slug>
     git branch -d worktree/<task-slug>
     ```
   - 如果 worktree 内有未提交改动，`remove` 会拒绝；此时回到 §3-3 收尾，不要用 `--force`。

## 4. 计划文件 vs worktree 改动的边界

| 类型 | 主分支 commit（前置） | worktree commit |
|------|------------------------|------------------|
| 计划文件本身（`development/workflow/<task>-plan.md`、`development/<域>/<...-plan.md>`） | ✅ 必须 | ❌ 不在 worktree 内改 |
| `CLAUDE.md` / `AGENTS.md` 内容 | ❌ | ✅ 在 worktree 内改 |
| 代码、其它文档、HTML artifact | ❌ | ✅ 在 worktree 内改 |

> 反例：不要把 `CLAUDE.md` 改动直接 commit 到主分支 working tree，否则违背 §1「worktree 隔离」目标。

## 5. 命名与产物目录约定

- `<task-slug>`：小写、连字符分隔、与计划文件同前缀，例如 `worktree-workflow-spec`。
- artifact 目录：`development/worktree-workflow-artifacts/`，首次使用即创建。
- artifact 文件名：`<YYYY-MM-DD>-<task-slug>.html`，日期使用任务完成日。

## 6. 与既有文档的关系

- 本规范是对 `CLAUDE.md` / `AGENTS.md` 中「工作树」「提交」「文档更新按任务拆分」相关条目的细化，不取代它们的提交路径、env、镜像等约定。
- 本规范与 [`docs/KNOWN_ISSUES.md`](../archive/KNOWN_ISSUES.md) 解耦：worktree 流程不改变已知问题的记录方式。
- `using-git-worktrees` superpowers 与本规范可以并存；本规范规定的是仓库级「主分支前置 + ff 合并 + 删树」的固定动作。

## 7. 失败回退

- §3-1 主分支前置提交失败：放弃本次任务，不创建 worktree。
- §3-2 worktree 创建失败：清理未完成的 worktree 目录与分支后，回到主分支重试。
- §3-5 fast-forward 失败：不要改用 `--no-ff`；先 `git fetch` + 评估冲突，必要时 `git rebase worktree/<task-slug>` 将 worktree 分支重放到最新主分支后再 ff 合并。
- 任何回退都要在 artifact 的「验证依据」里写清发生了什么。
