# KAIROS Control Plane Plan Index

> 状态：已实施（2026-07-15；live provider smoke 与 PTY 视觉 E2E 仍为独立验证项）
> 盘点日期：2026-07-15
> 范围：KAIROS 配置、daemon 生命周期、共享接口、HTTP API、SSE、Rust TUI 控制与展示。

## 实施结论

统一 KAIROS 控制面现已落地：source-aware typed profile、共享 lifecycle controller、CLI 与 `/kairos`、protocol/Web/daemon API、SSE lifecycle accepted event，以及 Rust TUI 四页 surface 和后台 snapshot poller。普通 Rust TUI 可直接执行 `Enable & Start`，等待 controller readiness 后由 poller 展示 ready、workers、automation 与 restart-required。

本计划集补齐三个层次：

1. [Runtime 与生命周期控制](2026-07-15-kairos-runtime-control-plan.md)
2. [协议与 API 控制面](2026-07-15-kairos-api-control-plan.md)
3. [Rust TUI 控制与展示](2026-07-15-kairos-tui-control-plan.md)

实施顺序必须是 runtime -> API -> TUI。API、斜杠命令、CLI 和 TUI 不得分别实现 daemon 启停逻辑。

## 现有文档盘点

| 文档 | 当前判断 | 后续用途 |
| --- | --- | --- |
| `2026-07-04-kairos-next-parity.md` | 主体能力已落地，但 checklist 未同步，属于 implemented-but-stale | 保留为历史总计划；不再把所有未勾选项视为真实缺口 |
| `2026-07-05-bridge-session-reuse-plan.md` | workspace reuse、lease、assistant session id、CLI/API 已落地 | 作为 TUI Bridge 页的现有能力依据 |
| `2026-07-05-daemon-submit-source-mapping-plan.md` | source 到 `QuerySource` 的映射与回归测试已落地 | 不在本轮重做 |
| `2026-07-05-structured-brief-output-plan.md` | shared payload、engine、daemon SSE、IPC、Rust TUI 渲染已落地 | 仅把 Brief 作为可配置 child gate 展示 |
| `mcp-tool-safety-analysis-plan.md` | 仍是独立待实施计划 | 与本轮只共享“配置来源、重启提示、TUI 状态”基础设施 |
| `../proactive/2026-07-06-proactive-full-parity-plan.md` | 已由 current-state 文档确认落地 | KAIROS 控制面复用其 durable state 和状态展示 |
| `../proactive/current-state-after-implementation.md` | 当前状态依据 | 保持 `/proactive` 兼容，不合并其语义 |
| `../reference/DAEMON_OPERATIONS.md` | 当前 daemon 操作依据 | 控制面实施完成后更新命令和 API 文档 |
| `../reference/REMOTE_CONTROL_GATEWAY.md` | 当前 remote gateway 依据 | 保持 gateway 与本地 KAIROS control API 的边界 |

`docs/WORK_STATUS.md` 已把 resident assistant parity、workers、automation state、Brief、Dream、HTTP/SSE、bridge reuse 等列为已落地，因此旧计划中的空 checkbox 不能继续作为实施状态来源。

## 当前实现与缺口

### 已有基础

- `FeatureFlags` 支持 `FEATURE_KAIROS*` 环境变量、依赖约束和进程内 runtime override。
- `/experimental on` 能开启当前进程的全部实验 gate，但不是持久化 KAIROS 配置，也不能可靠启动后台 daemon。
- `/proactive` 已能切换 durable proactive state。
- daemon CLI 已有 start/status/stop/restart、readiness、operation lock、stale cleanup 和 worker 状态。
- daemon HTTP 已有 `/api/status`、`/api/command`、`/events`、history 和 bridge session routes。
- Rust TUI 已有 command surface、Remote surface、proactive footer 状态和 subsystem event 通路。

### 真实缺口

| 层 | 缺口 |
| --- | --- |
| 配置 | KAIROS 仍以启动环境变量为主；没有 source-aware、可持久化、可展示的 typed profile |
| Runtime | start/restart 被封装在 CLI parser 私有函数内；没有无 stdout/ExitCode 副作用的 typed controller |
| Command | `/daemon start`、`/daemon restart` 只输出 shell 提示；没有 `/kairos` 用户入口 |
| 状态 | `StatusResponse` 只有 `kairos_active`，不能表达 desired/effective/running profile、配置来源、restart-required 和生命周期错误 |
| API | protocol-backed web API 与 daemon loopback API 都没有 typed KAIROS config/control contract |
| SSE | 没有 KAIROS lifecycle transition event，前端只能轮询粗粒度 status |
| TUI | 没有 KAIROS surface、启停动作、feature gate 展示、worker/readiness/error 展示 |
| 运行语义 | 普通 TUI 的 `AppState.kairos_active` 不代表后台 daemon；把它直接设为 true 会制造双执行 owner 风险 |

## 统一架构

```mermaid
flowchart LR
    TUI[Rust TUI /kairos surface] --> CMD[/kairos command]
    CLI[daemon CLI] --> CTRL[KairosController]
    CMD --> CTRL
    WEB[protocol-backed web API] --> CTRL
    DAPI[daemon loopback API] --> CTRL
    CTRL --> CFG[source-aware KAIROS profile]
    CTRL --> PROC[daemon process state and readiness]
    CTRL --> SNAP[KairosRuntimeSnapshot]
    SNAP --> TUI
    SNAP --> WEB
    SNAP --> DAPI
```

共享控制器是唯一写配置、获取 operation lock、清理 stale state、spawn/stop/restart 和等待 readiness 的位置。展示层只能发送 typed action 或调用 `/kairos`，不能拼接 shell 命令。

## “从 TUI 直接开启”的确定语义

TUI 的 `Enable & Start` 必须原子地完成：

1. 将 KAIROS profile 写入选定 settings scope；TUI 默认写 `.allthecodes/settings.local.json`，避免把本机 daemon 选择提交到仓库。
2. 解析 desired/effective profile 和每个字段的 source。
3. 使用共享 controller 启动独立 daemon child。
4. 等待 `/readyz` 或等价 readiness contract 成功；超时返回 typed error。
5. 刷新 TUI snapshot，展示 lifecycle、PID、URL、workers、automation 和 restart-required。
6. Bridge session 复用/attach 保持显式操作；开启 KAIROS 不静默抢占其他 lease。

普通 TUI 当前的 `QueryEngine` 继续拥有本地交互会话。它不得在按下 Enable 后静默变成 daemon assistant worker，也不得在同一进程再启动第二个自动模型执行循环。

## 兼容性边界

- 保留 `FEATURE_KAIROS*` 环境变量，作为部署/CI/临时覆盖入口。
- 保留 `/experimental`；它仍只表示当前进程实验 gate override，不替代 KAIROS profile。
- 保留 `/daemon` 和 daemon CLI；实现后将其 start/restart 重定向到共享 controller。
- `GET /api/status` 现有字段保持兼容，新增嵌套 `kairos` snapshot。
- daemon 仍只监听 loopback，mutating routes 仍要求 control token。
- remote-control gateway 不是公开 KAIROS 管理 API；不借本轮扩大公网暴露面。
- 所有持久化路径继续位于 `~/.allthecodes/`、`ALLTHECODES_HOME` 或项目 `.allthecodes/`。

## 实施批次

| 批次 | 目标 | 完成门禁 |
| --- | --- | --- |
| R1 | shared DTO + typed settings profile | 配置 merge/source/依赖测试通过 |
| R2 | typed controller + CLI/command adapter | stopped/start/ready/restart/stop 离线 E2E 通过 |
| R3 | lifecycle snapshot + SSE producer | restart-required、stale、timeout、failure 可区分 |
| A1 | protocol API + web handlers | route registry、JSON/schema/codegen 测试通过 |
| A2 | daemon loopback API +兼容 status | token、状态、控制、SSE replay 测试通过 |
| T1 | `/kairos` 命令 + command surface | fake runtime 单元测试通过 |
| T2 | TUI poller/footer/workers/bridge 展示 | render、event、PTY smoke 通过 |
| C1 | 文档、旧状态索引、发布验证 | scoped tests、workspace check、release build 通过 |

每个批次单独提交；只暂存该批次明确涉及的文件。

## 总体验收

- 全新临时 `ALLTHECODES_HOME` 下，普通 TUI 可通过 `/kairos` surface 选择 `Enable & Start`，无需退出 TUI 或手动设置环境变量。
- 启动成功后，TUI 在 readiness 完成后显示 `ready`，并展示 supervisor、assistant/proactive/scheduler/bridge workers 的实际状态。
- 修改 child gate 后能看到 desired/effective/source 与 `restart required`，重启后标记清除。
- start/restart/stop 在 CLI、slash command、web API 和 daemon API 使用同一 controller 和相同结果类型。
- 启动失败、端口占用、stale state、operation conflict、readiness timeout 均产生可操作错误，不显示假成功。
- 普通 TUI 不创建第二个 daemon assistant owner；Bridge lease 不被静默抢占。
- 无 provider 凭据和无网络时仍可完成配置、生命周期和状态 E2E；真实模型 smoke 继续作为可选验证。
