Codex 审查报告目标： working tree diff（工作树差异）该补丁引入了有用的功能，但在日常使用场景中存在两个新流程被破坏：/add-dir 命令在 TUI 中实际无法持久化生效；computer-use 的截图结果在 headless 前端中不可见。此外，新增的 additional-directory 簿记机制还存在一个确定性的碰撞 bug。完整审查意见：[P1] 将 /add-dir 的变更持久化回 TUI 引擎
src/commands/add_dir.rs:82-90
该 handler 仅修改了 ctx.app_state，但 ratatui 命令路径（src/ui/tui.rs:544-579）仅将 ctx.messages 复制回 QueryEngine，并丢弃了更新后的权限上下文。因此在默认 TUI 中，/add-dir <path> 虽然报告“成功”，但后续的 Write/Edit 调用依然会根据旧的 workspace set 进行检查，并被拒绝为“越界（out-of-bounds）”。
[P2] 不要仅使用 basename 作为 additional directories 的 key
src/commands/add_dir.rs:75-86
新目录仅以 canonical.file_name() 为 key 进行存储，导致两个允许目录如果叶节点名称相同，就会发生静默碰撞。例如先后添加 /repo/frontend/src 和 /repo/backend/src，第二次 .insert() 会直接覆盖第一次，导致之前已授权的一个目录从 additional_working_directories 中消失，后续的路径检查就会出现意料之外的失败。
[P1] 在截图结果可见之前，不要提前宣传 computer-use 工具
src/engine/system_prompt.rs:459-460
在配置了 mcp__computer-use__* 服务器的 headless/ink-terminal 会话中，这段新增的 prompt 会让模型开始使用截图/点击循环，但 src/ipc/headless.rs:523-546 仍将 ToolResultContent::Blocks 转换为纯文本（仅提取嵌套的 text block）。由于截图结果是纯图像，frontend 收到的 tool_result 为空，导致界面上不显示任何输出，用户无法查看模型实际看到的画面，也无法验证操作流程。
