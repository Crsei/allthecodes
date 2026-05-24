# 命令 E2E 测试计划 04：Git 与 Diff 命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_git.rs`
> 离线测试（diff、gbranch）和在线测试（commit、branch）混合。

在claude-code-rust中测试

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 模式 |
|---|---|---|---|
| `/diff` | -- | `Output` | 离线 |
| `/diff --staged` | -- | `Output` | 离线 |
| `/commit` | -- | `Query`/`Output` | 在线（无参数）/ 离线（带消息） |
| `/branch` | `/br` | `Output`/`SwitchSession` | 在线 |
| `/gbranch` | `/gitbranch` | `Output` | 离线 |

## 测试用例

### T01：`/diff` 显示变更
```rust
fn diff_command_shows_changes() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("diff")
    // 3. Wait(3s)
    // 4. AssertNoPanic
    // 5. Snapshot("diff_output")
    // 断言：屏幕显示 diff 输出或 "no changes"
}
```

### T02：`/diff --staged`
```rust
fn diff_staged_only() {
    // 步骤：
    // 1. Command("diff --staged")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：仅显示暂存的变更
}
```

### T03：`/diff --cached`（--staged 的别名）
```rust
fn diff_cached_alias() {
    // 步骤：
    // 1. Command("diff --cached")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：与 --staged 行为相同
}
```

### T04：`/commit` 带消息（离线 - 直接 git commit）
```rust
fn commit_with_message() {
    // 注意：此测试修改 git 状态。请使用临时工作区。
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("commit test: e2e commit message")
    // 3. Wait(3s)
    // 4. AssertNoPanic
    // 断言：git commit 已执行或显示错误
}
```

### T05：`/commit` 不带消息（在线 - 生成 Query）
```rust
#[ignore = "requires real API key"]
fn commit_no_message_generates_query() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("commit")
    // 3. Wait(5s)
    // 4. AssertNoPanic
    // 断言：模型接收 commit 上下文
}
```

### T06：`/gbranch` 显示 git 分支
```rust
fn gbranch_command() {
    // 步骤：
    // 1. Command("gbranch")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. AssertScreenContains("branch") 或屏幕非空
    // 断言：显示 git 分支列表
}
```

### T07：`/gbranch` 别名 `/gitbranch`
```rust
fn gbranch_alias() {
    // 步骤：
    // 1. Command("gitbranch")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T08：`/branch` 分叉对话（在线）
```rust
#[ignore = "requires real API key"]
fn branch_forks_session() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. SetPermission("full access")
    // 3. Input("Say exactly: BRANCH_MARKER_42")
    // 4. WaitForText("BRANCH_MARKER_42", API_TIMEOUT)
    // 5. Command("branch test-branch")
    // 6. Wait(3s)
    // 7. Snapshot("after_branch")
    // 8. AssertNoPanic
    // 断言：会话已分叉，无 panic
}
```

## 优先级：高
Git 命令频繁使用。`/diff` 和 `/gbranch` 是安全的离线测试。

---

## 补充功能说明

以下为各命令的功能细节，供测试断言和用例设计参考。

---

### `/diff`

#### 功能描述
显示当前工作区的 git diff。默认同时显示 staged（暂存）和 unstaged（未暂存）的变更；`--staged` 或 `--cached` 仅显示暂存变更。使用 `git2` crate 直接读取仓库状态，不调用外部 git 命令。输出为 unified diff 格式。

#### 输出示例
无变更时：
```
No changes detected.
```

有变更时（默认模式）：
```
=== Staged changes ===

diff --git a/file.rs b/file.rs
--- a/file.rs
+++ b/file.rs
@@ -1,3 +1,4 @@
 line1
+added line
 line2

=== Unstaged changes ===

diff --git a/other.rs b/other.rs
--- a/other.rs
+++ b/other.rs
@@ -5,3 +5,4 @@
 context
+another addition
 end
```

不在 git 仓库时：
```
Not a git repository (or any parent): ...
```

#### 源码路径
`crates/cc-commands/src/diff.rs`

#### 边界情况
- 不在 git 仓库中 → 返回 `Not a git repository` 错误信息
- 无任何变更 → `No changes detected.`
- 只有 staged 变更、无 unstaged → 仅显示 `=== Staged changes ===` 部分
- 只有 unstaged 变更、无 staged → 仅显示 `=== Unstaged changes ===` 部分
- 初始提交（无 HEAD）→ staged diff 以空 tree 为基准
- `--staged` 和 `--cached` 行为完全一致
- diff 输出中文本行以 `+`、`-`、` ` 开头标记新增/删除/上下文

---

### `/commit`

#### 功能描述
创建 git 提交。无参数时不直接提交，而是构造一条包含当前变更摘要的 `Query` 消息（返回 `CommandResult::Query`），让模型审查变更并帮用户生成 commit message。带参数时直接执行 `git commit -m "<message>"`。当前实现不会自动 `git add`（暂存），只在有已暂存变更时执行提交。

#### 输出示例
带消息且成功：
```
Committed successfully.
[main abc1234] test: e2e commit message
 1 file changed, 5 insertions(+)
```

带消息但提交失败：
```
Commit failed:
nothing to commit
```

无参数（生成 Query）：
```
**Current changes:**
  Staged: 2 file(s)
    Modified src/main.rs
    Added src/lib.rs
  Unstaged: 1 file(s)
    Modified README.md
  Untracked: 3 file(s)

Please review the changes and create a git commit. Stage the relevant files and write a clear commit message summarizing the changes.
```

无变更时：
```
Nothing to commit — working tree clean.
```

不在 git 仓库时：
```
Error: not in a git repository.
```

#### 源码路径
`crates/cc-commands/src/commit.rs`

#### 边界情况
- 不在 git 仓库中 → 错误提示（有消息和无消息参数均如此）
- 工作树完全干净（无 staged、unstaged、untracked）→ `Nothing to commit — working tree clean`
- 无参数时返回 `Query` 类型而非 `Output`（测试需区分返回变体）
- 无参数不会自动 stage 文件，只汇总状态信息让模型处理
- 带消息时直接调用 `git commit -m`，不检查是否有 staged 文件
- git 命令执行失败时输出 stdout + stderr

---

### `/branch`

#### 功能描述
（与 plan-02 相同）将当前对话 transcript fork 到新 session。这是会话级别的分叉命令，**不是** git 分支操作。传参数时提示使用 `/gbranch`。返回 `SwitchSession` 切换到新会话。

#### 输出示例
```
Branched conversation. You are now in the new branch (session abc12345-...).
Use /resume def67890-... to return to the original conversation.
From a terminal, run: cc-rust -r def67890-...
```

传参提示：
```
/branch takes no arguments and forks the current conversation.
Did you mean `/gbranch feature/foo` (git branch wrapper)?
```

#### 源码路径
`crates/cc-commands/src/branch.rs`

#### 边界情况
- 与 plan-02 中 `/branch` 的边界情况一致
- 别名 `/br` 可用，但注意 `/br` 不是 git branch 的别名
- 明确区分 `/branch`（会话 fork）和 `/gbranch`（git 操作）

---

### `/gbranch`

#### 功能描述
显示或管理 **git** 分支。无参数时列出所有本地分支，当前分支前标记 `*`；传参数时先尝试 `git checkout <branch>` 切换分支，若分支不存在则执行 `git checkout -b <branch>` 创建并切换。使用 `git2` crate 的 `list_branches` 辅助函数获取分支列表，切换/创建则调用外部 `git` 命令。

#### 输出示例
列分支：
```
* main
  feature/auth
  fix/bug-123
```

切换成功：
```
Switched to branch 'feature/auth'.
Already on 'feature/auth'
```

创建并切换：
```
Created and switched to new branch 'new-feature'.
```

不在 git 仓库时：
```
Error: not in a git repository.
```

无分支时：
```
No branches found.
```

切换/创建失败：
```
Failed to switch/create branch 'bad...name':
error: ...
```

#### 源码路径
`crates/cc-commands/src/gbranch.rs`

#### 边界情况
- 不在 git 仓库中 → `not in a git repository` 错误提示（无参数和有参数均如此）
- 无分支 → `No branches found.`
- 切换已存在的分支 → `git checkout`
- 分支不存在 → 自动尝试 `git checkout -b` 创建
- git 命令输出通常在 stderr（切换成功信息由 git 写到 stderr）
- 别名 `/gitbranch` 可用
- 注意：`/br` 是 `/branch` 的别名，不是 `/gbranch` 的别名
