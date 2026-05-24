## P0

先把现有 headless 基线修绿。`cargo test --test e2e_terminal` 现在有 11 个失败。根本原因是 `src/ipc/headless.rs` (line 334) 的 `handle_slash_command` 已从 stub 升级为真正执行命令，所有 `CommandResult::Output` / `Clear` / `Exit` 都返回 `level: "info"`（line 384, 393, 399），而测试还在断言旧的 `"warning"` 级别和 `"not yet supported"` 文本。

需要修复的测试文件：

- `tests/e2e_terminal/offline.rs` (line 144) — `slash_command_returns_warning` 断言 `level == "warning"` 和 `text.contains("not yet supported")`，应改为 `level == "info"` 并去掉 "not yet supported" 断言
- `tests/e2e_terminal/commands.rs` (line 124-135) — `all_slash_commands_return_warning_level` 断言 6 个命令都返回 `"warning"` level，应改为 `"info"`
- `tests/e2e_terminal/commands.rs` (line 137-163) — `multiple_slash_commands_in_session` 现在会失败因为部分命令（如 `/clear`）返回的 type 可能不是 `"system_info"` 而是触发 `conversation_replaced` 等消息序列，需要更新收集逻辑
- `tests/e2e_terminal/commands.rs` (line 35-45) — `slash_help` 断言 `text.contains("/help")`，现在 /help 返回完整命令列表，断言仍通过但注释描述已过时（不再是 "warning"）
- `tests/e2e_terminal/commands.rs` (line 71-77) — `slash_clear` 触发 `CommandResult::Clear`，headless 端会先发 `conversation_replaced` 再发 `system_info`，测试只读一条消息可能读到错误的那条
- `tests/e2e_terminal/commands.rs` (line 104-107) — `slash_empty` 发送 `/`，`parse_command_input` 返回 None，走 `BackendMessage::Error` 路径而非 `system_info`
- `tests/e2e_terminal/commands.rs` (line 110-114) — `slash_unknown` 同上，未知命令走 Error 路径

**修复方向**：更新测试断言以匹配 `handle_slash_command` 的真实行为。具体来说：
1. 所有成功命令的 level 从 `"warning"` 改为 `"info"`
2. 去掉 `"not yet supported"` 文本断言
3. `/clear` 测试需要先消费 `conversation_replaced` 消息再断言 `system_info`
4. `/` 和 `/nonexistent_command_xyz` 应断言 `type == "error"` 而非 `type == "system_info"`
5. 更新 `commands.rs` 模块顶部注释（line 1-6），不再说 "all slash commands return a not yet supported warning"

---

停止把 tracing 文本日志当 dashboard 事件源。当前只有 `src/main.rs` (line 195-206) 的 `.logs/cc-rust.log.YYYY-MM-DD`。按 spec 需要新增专用结构化事件文件 `.logs/subagent-events.ndjson`。

---

落地 dashboard 后端入口。补 `mod dashboard;` 到 `src/main.rs` (line 49 区域，`mod services;` 之后)，新增 `src/dashboard.rs`，负责：

- `FEATURE_SUBAGENT_DASHBOARD=1` 开关（需在 `src/config/features.rs` 新增此 feature gate）
- 仅交互式 TUI 启动
- 非 `--headless` / `--print` / `--daemon`
- 进程退出时清理 companion

---

补 `.logs/` 到 `.gitignore`。当前 `.gitignore` 只有 `logs/**/*.log` 等规则（line 27-31），但运行时日志写入的是 `.logs/`（带点前缀），两者不匹配。应添加 `.logs/` 条目。

## P1

4. 在 Agent 生命周期里补结构化事件写入。优先覆盖：
   - spawn
   - complete
   - error
   - background_complete
   - worktree_kept / worktree_cleaned
   - background+worktree fallback warning

   关键落点：
   - `src/tools/agent/tool_impl.rs` (line 91-104) — subagent 参数解析和模型解析处，在 spawn 子 QueryEngine 之前写入 `spawn` 事件
   - `src/tools/agent/dispatch.rs` (line 1) — 正常和 worktree 执行模式分发入口，在执行结束后写入 `complete` / `error` 事件
   - `src/tools/agent/worktree.rs` (line 1) — worktree 隔离执行，在 worktree 保留或清理后写入 `worktree_kept` / `worktree_cleaned` 事件

5. 新增 companion UI 目录。当前缺：
   - `ui/subagent-dashboard/server.ts`
   - `ui/subagent-dashboard/event-watcher.ts`
   - `ui/subagent-dashboard/dashboard.html`
   - `tests/dashboard_verify.ts`

   这些文件当前都不存在。

6. 给 `ui/package.json` 增加 dashboard 脚本。现在 `ui/package.json` (line 6-9) 只有 `dev` / `build`，需新增 companion 启动脚本。

## P2

7. 把 subagent e2e 真正接入现有 headless 测试集。按 spec 在 `tests/e2e_terminal/` 下新增 `subagent.rs`，并在 `tests/e2e_terminal/main.rs` (line 27，`mod usage;` 之后) 加 `mod subagent;`。

8. 补最小测试矩阵：
   - 同步 subagent 成功
   - background complete
   - recursion depth limit
   - abort 后可继续下一轮
   - worktree isolation
   - background + worktree 降级行为

9. 保持 spec 和实现一致。当前 spec 已经写成"结构化事件 + companion"，但代码只做了日志增强、UI early-message buffer 和 copy-on-select。实现没跟上 spec，需要避免继续漂移。

## P3

10. 清理无关改动或隔离提交。当前和本任务直接相关的只有：
    - `src/ipc/headless.rs` (line 334-440) — slash command 实际执行逻辑
    - `src/tools/agent/tool_impl.rs` (line 91) — subagent 参数 & 模型解析
    - `ui/src/ipc/client.ts` (line 6)
    - `ui/src/main.tsx` (line 31)

    其它大量变更先别混进 dashboard 实现里。

11. 把 `.logs/` 加入 `.gitignore`。该目录已出现在工作区（`ui/.logs/`），但 `.gitignore` 没有覆盖 `.logs/` 路径。

## 建议执行顺序

1. 修 `e2e_terminal` 基线失败（更新 11 个测试的断言以匹配新的 slash command 行为）
2. 做 Rust 侧结构化事件写入
3. 做 `src/dashboard.rs` 生命周期管理 + `src/config/features.rs` 新增 feature gate
4. 做 `ui/subagent-dashboard/*`
5. 补 `subagent.rs` 和 `dashboard_verify.ts`
6. 跑：
   ```
   cargo test --test e2e_terminal
   cargo check --bin claude-code-rs
   ```
