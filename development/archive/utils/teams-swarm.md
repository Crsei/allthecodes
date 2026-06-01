# Team/Swarm 系统移植缺口

对应: `claude-code-bun/src/utils/swarm/`  
Rust 对应: `cc-teams/`, `cc-engine/teams/` (空目录)

> 当前阶段已进入 Full Build。本文中的 tmux/iTerm2/Windows Terminal/pane backend 缺口不再按历史 Lite 裁剪视为既定边界；若后续仍决定保留 InProcess-only，需要在对应实现和文档中显式标注为 Intentional。

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| inProcessRunner.ts | 1,650 | 进程内队友运行器 |
| teamHelpers.ts | 683 | 团队文件管理 |
| permissionSync.ts | 928 | 权限同步 |
| spawnInProcess.ts | 383 | 进程内生成 |
| spawnUtils.ts | 165 | 生成工具 |
| teammateInit.ts | 129 | 队友初始化 |
| teammateLayoutManager.ts | 107 | 布局管理 |
| teammateModel.ts | 10 | 模型类型 |
| teammatePromptAddendum.ts | 18 | Prompt 补充 |
| leaderPermissionBridge.ts | 54 | 领导权限桥接 |
| reconnection.ts | 119 | 重连处理 |
| constants.ts | 33 | 常量 |
| **Backends:** | | |
| TmuxBackend.ts | 800 | Tmux 后端 |
| ITermBackend.ts | 370 | iTerm2 集成 |
| InProcessBackend.ts | 345 | 进程内后端 |
| PaneBackendExecutor.ts | 407 | 窗格执行器 |
| WindowsTerminalBackend.ts | 237 | Windows 终端 |
| registry.ts | 585 | 后端注册表 |
| types.ts | 349 | 类型定义 |
| detection.ts | 158 | 后端检测 |
| **合计** | **~7,862** | |

## Rust 已实现

### cc-teams/ (核心 InProcess 路径已接线，终端 pane 后端仍缺失)
- **in_process.rs**: 进程内队友执行器
- **mailbox.rs**: 邮箱 IPC (文件锁)
- **runner.rs**: 队友运行循环
- **send_message.rs**: SendMessage 工具
- **team_spawn.rs**: TeamSpawn 工具
- **layout_manager.rs**: 队友颜色分配与会话内缓存
- **reconnection.rs**: CLI 启动、`--resume`、`/resume` 的团队上下文恢复
- **identity.rs**: 身份系统 (tokio::task_local!)
- **coordinator.rs**: 协调器模式
- **context.rs**: 上下文传播
- **protocol.rs**: 协议定义
- **types.rs**: 类型定义
- **helpers.rs**: 共享工具
- **constants.rs**: 常量
- **pr_activity.rs**: PR 活动工具
- **backend.rs**: 后端抽象 (TeammateExecutor trait)

### cc-engine/teams/ — 空目录
- 作为未来提取的占位，尚无代码

## Rust 剩余主要缺口

### 1. Tmux 后端 (800 行) — 未迁移
- Bun 版本的功能齐全的 tmux 后端
- 窗格创建/管理/终止
- Tmux socket 通信
- Full Build 下需要重新评估是否补齐；若继续不做，应显式标注为 Intentional

### 2. iTerm2 后端 (370 行) — 未迁移
- iTerm2 Python API 集成
- 窗格分割和管理
- Full Build 下需要重新评估是否补齐；若继续不做，应显式标注为 Intentional

### 3. Windows Terminal 后端 (237 行) — 未迁移
- Windows Terminal 集成
- Full Build 下需要重新评估是否补齐；若继续不做，应显式标注为 Intentional

### 4. 窗格执行器 (407 行) — 未迁移
- 多窗格命令执行
- 窗格布局管理

### 5. 后端注册表和检测 (585+158 行) — 未迁移
- 自动后端检测
- 平台能力检测
- 后端选择策略

### 6. 布局管理 (107 行) — ✅ 已实现 (layout_manager.rs)
- 队友颜色分配 (round-robin, 会话内缓存)
- 颜色查询与重置
- `/team spawn` 与 `TeamSpawn` 已接入 `assign_color_for_teammate()`，不再只是孤立 helper
- **注意**: 这里已实现的是颜色/视觉标识分配；窗格管理 (tmux/iTerm2 pane splitting, pane border status, 命令发送至 pane) 仍归入终端 backend 缺口
- 共 ~90 行，包含测试覆盖

### 7. 重连处理 (119 行) — ✅ 已实现 (reconnection.rs)
- `compute_team_context()` — 从团队配置文件读取并构建 TeamContext，支持 Leader/Teammate 角色判定
- `restore_team_context()` — 从团队配置文件恢复 Teammate 的 TeamContext（会话恢复场景）
- `restore_team_context_for_session()` — 通过 team file 中的 session 绑定恢复 CLI 启动和 `/resume` 的 TeamContext
- 缺失 team file 时降级为无 context；成员已移除时保留 resumed `agentName`
- 已接入 CLI `--resume`/`--continue` 启动路径和 `/resume` 的 `CommandResult::SwitchSession`
- 剩余问题: lead session 可通过 `lead_session_id` 恢复；teammate 自身 session 恢复仍依赖 `TeamMember.session_id` 持久化，见 `development/archive/KNOWN_ISSUES.md` 的 `TEAMS-001`
- 共 ~280 行（含 9 个测试用例）

## 设计决策差异

Bun 版本支持多后端 (tmux, iTerm2, InProcess, Windows Terminal)，提供灵活的团队执行模式。  
Rust 当前只实现 InProcess 后端；tmux/iTerm2/Windows Terminal/pane backend 尚未迁移。

历史代码注释曾把 tmux/iTerm2 窗格描述为故意裁剪。由于本仓库当前阶段已转为 Full Build，这些注释不能继续作为默认边界；后续处理方式应二选一:
- 对齐 Bun 的多后端和 pane backend 行为
- 明确写入 Intentional 取舍，并说明为什么 Rust 版保持 InProcess-only

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| 进程内队友 | 完整 (1,650 行) | in_process.rs | ✅ 已移植 |
| 邮箱 IPC | 完整 | mailbox.rs (文件锁) | ✅ 已移植 |
| SendMessage 工具 | 完整 | send_message.rs | ✅ 已移植 |
| TeamSpawn 工具 | 完整 | team_spawn.rs | ✅ 已移植 |
| 协调器模式 | 完整 | coordinator.rs | ✅ 已移植 |
| **Tmux 后端** | **800 行** | **未迁移** | **Full Build 待补齐或显式 Intentional** |
| **iTerm2 后端** | **370 行** | **未迁移** | **Full Build 待补齐或显式 Intentional** |
| **Window Terminal** | **237 行** | **未迁移** | **Full Build 待补齐或显式 Intentional** |
| **窗格管理** | **~1,200 行** | **未迁移** | **Full Build 待补齐或显式 Intentional** |
| **后端注册表/检测** | **~740 行** | **未迁移** | **Full Build 待补齐或显式 Intentional** |
| **布局管理** | **107 行** | **layout_manager.rs** | **✅ 颜色部分已接入；pane 部分仍缺** |
| **重连处理** | **119 行** | **reconnection.rs** | **✅ 已接入；teammate self-session 恢复仍见 TEAMS-001** |
