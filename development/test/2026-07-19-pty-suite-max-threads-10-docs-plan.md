# PTY 套件 10 路并行说明同步计划

生效日期：2026-07-19
任务分支：`worktree/pty-suite-max-threads-10-docs`

## 目标

同步 `AGENTS.md` 与 `CLAUDE.md` 的 PTY 分层验证命令，使说明与已经完成全套回归验证的 `.config/nextest.toml` 保持一致：完整离线 PTY 套件使用 nextest，`tui_pty_e2e` 测试组最多并行 10 个 test process。

## 安全边界

- 权威完整离线回归命令为 `cargo nextest run -p allthecodes --test pty_tui_e2e --no-fail-fast`。
- 10 路并发由 `.config/nextest.toml` 的 `test-groups.tui_pty_e2e.max-threads = 10` 控制。
- 普通 `cargo test` / libtest 的 case 共享同一个 runner PID，仍只允许 `--test-threads=1`；不得把该参数直接改为 10。
- 本任务只修改说明文档与工作流 artifact，不修改测试代码或 nextest 配置。

## 实施步骤

1. 在主分支单独提交本计划。
2. 创建 `.worktrees/pty-suite-max-threads-10-docs`。
3. 在 worktree 中同步修改 `AGENTS.md` 与 `CLAUDE.md` 的 PTY 验证条目。
4. 创建 `development/worktree-workflow-artifacts/2026-07-19-pty-suite-max-threads-10-docs.html`。
5. 用文本一致性检查、`git diff --check` 和 nextest 配置检查验证说明准确性。
6. 提交 worktree 改动，fast-forward 合并回 `allthecodes`，推送并清理 worktree/分支。

## 验收标准

- 两份说明都明确 PTY 完整离线套件可最多 10 路并行。
- 两份说明都给出相同的 nextest 命令和普通 libtest 串行边界。
- `.config/nextest.toml` 仍为 `max-threads = 10`。
- 只提交本计划、`AGENTS.md`、`CLAUDE.md` 和对应 HTML artifact；不包含主工作区既有的无关修改。
