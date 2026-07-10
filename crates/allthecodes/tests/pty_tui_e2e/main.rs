#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! PTY TUI E2E 测试套件 — 真实终端交互测试
//!
//! 完整测试 cc-rust TUI 的端到端行为：启动、权限、对话、状态。
//!
//! 参考命令操作语义化展示计划（`development/tui/command-operation-display-plan.md` Phase 7），
//! 本套件覆盖 TUI 的测试与验收要求：欢迎屏幕、权限交互、斜杠命令、状态栏、对话流、
//! CommandSurface 交互、模型切换等。所有 Phase 1-7 实现已完成，release build 已通过验收。
//!
//! ## 架构
//!
//! 通过 `portable-pty` 创建真实伪终端，启动 `allthecodes` 二进制，
//! 模拟键盘输入并用 `vt100` 解析器读取屏幕状态。
//!
//! ```text
//! ┌──────────┐    PTY    ┌──────────────┐    screen    ┌───────────┐
//! │ 测试代码  │ ────────→ │ allthecodes│ ──────────→ │ vt100 解析│
//! │ (Rust)   │ ←──────── │ (TUI 二进制)  │ ←────────── │ (断言)    │
//! └──────────┘  stdin    └──────────────┘  stdout      └───────────┘
//! ```
//!
//! ## 模块说明
//!
//! ### 根模块（集成式 PTY TUI 测试，对应 Phase 7 验收要求）
//!
//! | 模块            | 测试内容                               | 关联 Phase |
//! |-----------------|----------------------------------------|-----------|
//! | `harness`       | PTY 会话封装（非测试，工具模块）        | —         |
//! | `welcome`       | 欢迎屏幕：Logo、模型名、会话 ID        | Phase 7   |
//! | `conversation`  | 完整对话流：输入 → 响应 → 多轮 → 工具  | Phase 7   |
//! | `permissions`   | 权限交互：bypass/default/auto、对话框   | Phase 5, 7 |
//! | `commands`      | 斜杠命令：/help /version /clear        | Phase 7   |
//! | `status`        | 状态栏：消息计数、模型名、运行状况      | Phase 7   |
//! | `screenshot`    | 截图保存：HTML 渲染、mid-session 快照   | Phase 7   |
//! | `script`        | 步骤式模板引擎：数据驱动的测试脚本      | —         |
//! | `model_flow`    | 模型验证：settings.json 配置、/model 切换 | Phase 7   |
//!
//! ### `tests/` 子模块（CommandSurface 交互测试，~130 个离线测试用例）
//!
//! 覆盖 command-operation-display-plan.md Phase 3-7 中的 CommandSurface 交互：
//! 打开/关闭、Tab/箭头导航、filter 筛选、快捷键、提交、空态/禁用态保护。
//!
//! | 模块                         | 测试内容                        |
//! |------------------------------|---------------------------------|
//! | `commands_surface`           | 所有 CommandSurface 的交互行为   |
//! | `commands_core_info`         | /info、/version、/cost 等       |
//! | `commands_query`             | /query、/plan 等查询命令        |
//! | `commands_auth`              | /login status 等认证命令        |
//! | `commands_git`               | git 相关斜杠命令                |
//! | `commands_mcp_plugin`        | MCP / plugin 交互命令           |
//! | `commands_memory_skills_hooks` | /memory、/skills、/hooks     |
//! | `commands_permissions`       | /permissions 交互命令           |
//! | `commands_agent_team`        | /agent、/team 交互命令          |
//! | `commands_aliases`           | 命令别名交互测试                |
//! | `commands_session`           | /session 交互命令               |
//! | `commands_kairos`            | /kairos 交互命令                |
//! | `running_task_slash_commands`| 任务运行中的斜杠命令            |
//! | `test1_login_structure` ~ `test5_compact` | 登录/权限/计划/任务/压缩综合流程 |
//!
//! ## 运行
//!
//! ```bash
//! # 离线测试（不需要 API key，测试 UI 渲染和交互）
//! cargo test --test pty_tui_e2e -- --nocapture
//!
//! # 在线测试（需要真实 API key，测试完整对话流程）
//! cargo test --test pty_tui_e2e -- --ignored --nocapture
//!
//! # 单个模块
//! cargo test --test pty_tui_e2e welcome -- --nocapture
//! cargo test --test pty_tui_e2e conversation -- --nocapture
//! cargo test --test pty_tui_e2e permissions -- --nocapture
//! cargo test --test pty_tui_e2e commands_surface -- --nocapture
//! ```

mod harness;

mod commands;
mod conversation;
mod model_flow;
mod permissions;
mod screenshot;
mod script;
mod status;
mod tests;
mod welcome;
