# PTY TUI E2E 日志核对记录

日期：2026-05-29

范围：`crates/allthecodes/src/tests/pty_tui_e2e/tests/` 生成的脚本日志，重点核对
`crates/allthecodes/logs/pty_tui_e2e_scripts/` 下对应目录的输出内容是否符合测试断言。

## 总结

- 日志产物整体完整，常见目录的 `.log`、`.stream.log`、`.raw`、`.html` 基本齐全。
- 大部分基础稳定性场景正常，欢迎屏、状态栏、截图和常规会话流没有明显结构性损坏。
- 主要问题集中在两类：
  - 权限流里出现了与预期不一致的交互结果，部分目录只证明“没有崩溃”，没有证明目标内容真的输出。
  - 模型切换流里有命令被拒绝或没有完成切换的痕迹，和测试预期不一致。

## 逐项结论

### `OK`

- `terminal_setup_command`
  - 日志目录完整。
  - `step_003_terminal-setup` 里能看到 `usage: /terminal-setup [env|tips|all]`。
  - 没有 panic 痕迹。
- `unknown_command_no_crash`
  - 目录完整。
  - 步骤状态正常，没有 `panicked`。
  - 这组主要证明“不崩溃”，错误提示本身不够明确，但不影响基础判定。

### `Weak OK`

- `welcome` / `status` / `screenshot`
  - 目录和截图文件完整。
  - 欢迎屏、状态栏、HTML 截图都能正常打开，内容没有明显断尾或乱码。
  - 但 `welcome_model` 里仍显示 `gpt-5.5`，没有体现出“指定模型名被显示”的目标。
  - `status_ready` 类用例更多是通过测试内断言成立，日志里没有直接出现 `ready` 文本。
- `conversation`
  - 关键 marker 基本能对上，例如 `CONV_TEST_MARKER_7749`、`ZEPHYR_42`、`RECOVERED_AFTER_ABORT`、`COCONUT`、`AFTER_CLEAR_OK`。
  - `conv_tool_use` 里能看到 Bash 调用痕迹，但最终 `PTY_TOOL_RESULT_9988` 没有以很干净的单独结果行落下。
  - `conv_five_turns` 里出现了多次 `429 rate_limit_exceeded`，说明不是一条干净的五轮成功轨迹。
- `permissions_full_access`
  - 目录文件齐全。
  - 但日志里有 `API error`、`rate_limit_exceeded`，没有看到明确的成功输出。

### `Mismatch`

- `login_structure_with_permissions`
  - 目录文件齐全，但结果没有干净落到测试期望的结构输出。
  - 日志里出现了权限对话框，例如 `Permission Required - Run command?`。
  - 说明这条更像是“权限对话框被触发”，而不是“批准后稳定输出目录结构”的完整成功轨迹。
- `full_access_structure_no_dialog`
  - 目录文件齐全。
  - 但没有看到清晰的 `Cargo.toml` / `src/` 结果输出。
  - 主要内容是命令面板和输入回显，和“免对话框直接运行”的目标不完全一致。
- `model_flow`
  - `model_flow_verify_bar`、`model_flow_identity`、`model_flow_claude` 里能看到部分正确模型信息。
  - `model_flow_switch` 和 `model_flow_full` 显示 `/model gpt-5.4` 被拒绝，实际没有完成模型切换。
  - 因此这两条日志不支持“切换成功”的预期。

## 问题点汇总

1. `welcome_model` 没有体现指定模型名。
2. `status_ready` 类用例主要靠测试内断言，日志里没有直接证明 `ready`。
3. `login_structure_with_permissions` 更像触发了权限对话框，但没有稳定输出结构结果。
4. `full_access_structure_no_dialog` 没有形成干净的目录结构输出，和“无对话框直接运行”预期不一致。
5. `permissions_full_access` 里出现 `API error` 和 `rate_limit_exceeded`。
6. `model_flow_switch` / `model_flow_full` 没有完成模型切换。
7. `conv_tool_use` 的最终工具结果没有非常干净地单独落行。
8. `conv_five_turns` 受到 `429 rate_limit_exceeded` 影响，不算干净通过。

## 结论

这批 PTY TUI E2E 日志的文件完整性是好的，但内容正确性不是全绿。
基础展示类和大多数会话流基本正常，真正需要关注的是权限流和模型切换流的几处偏差。
