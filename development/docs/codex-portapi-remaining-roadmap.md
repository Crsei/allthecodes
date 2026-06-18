# Codex Port API 剩余目标总路线图

> 计划日期：2026-06-17
> 上游参考：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/`
> 入口文档：`development/docs/codex-portapi-phase-execution-plan.md`

## 背景

`codex-portapi-phase-execution-plan.md` 已记录 Phase 0-8 的主要落地状态。当前剩余目标不再适合继续塞进单个执行计划中推进，因为它们同时涉及外部 wire shape、跨平台本地传输、provider 流式事件类型和文档/schema 同步。

本路线图把剩余目标拆成可独立验收的阶段。每个阶段必须先补测试，再实现；每个阶段结束后更新对应执行记录。

## 已确认决策

- 允许 breaking migration，不继续保持 gateway `/remote-control/v1/runs/{run_id}/events` 的旧 `RunEvent` response shape。
- gateway 输出与 timeline 拆分：`/events` 返回 `OutputReadBatch`；非输出状态、审批、诊断等走新的 timeline 查询。
- 本地 socket transport 要跨平台：Unix 使用 Unix domain socket，Windows 使用 named pipe 等价传输。
- Provider streaming 目标是全链路迁移到 `ResponseEvent`，不是只在 provider crate 内部兼容。
- 历史持久化数据做 best-effort lazy import，但不再输出旧外部 JSON shape。

## 阶段顺序

1. **Phase A：文档落地**
   - 新增本路线图和三个执行计划文档。
   - 更新原 phase execution plan 的“当前优先级建议”。
   - 不改代码。

2. **Phase B：Phase 4b 输出恢复**
   - 计划文档：`development/docs/phase4b-output-recovery-plan.md`
   - 目标：统一 gateway run、daemon worker、agent supervisor 输出合约到 `OutputEvent` / `OutputReadBatch`。
   - 主要外部变化：`/remote-control/v1/runs/{run_id}/events` breaking 改为 output batch；新增 timeline 端点。

3. **Phase C：Local Socket Transport**
   - 计划文档：`development/docs/local-socket-transport-plan.md`
   - 目标：新增跨平台 local transport，复用现有 transport event、outbound router、replay/lag 能力。
   - Unix 使用 WebSocket-over-UDS；Windows 使用 named pipe 传输同等 JSON-RPC frames。

4. **Phase D：ResponseEvent 全链路迁移**
   - 计划文档：`development/docs/phase6-response-event-migration-plan.md`
   - 目标：用 `ResponseEvent` 替换 `StreamEvent` public contract，并补齐 Bedrock/Vertex provider runtime。

5. **Phase E：最终文档、schema 与复验**
   - 计划文档：`development/docs/codex-portapi-final-verification-plan.md`
   - 目标：同步 API 文档/OpenAPI/schema，复验 `web-ui` feature static assets，完成 workspace 级验证。

## 跨阶段规则

- 每个代码阶段必须保持 workspace 可构建。
- 涉及公共协议时，同阶段更新 docs/api 与 development/docs。
- 所有新增持久化路径必须保持 allthecodes/Codex 路径隔离：
  - `~/.allthecodes/`
  - `.allthecodes/`
  - keychain service `allthecodes`
- 不以历史 Lite 范围为理由省略分支；不能实现的内容写入 gaps/known issues。

## 最小验收命令

各阶段可追加 crate-specific 测试，但完成代码阶段时至少运行：

```bash
cargo check -p allthecodes --bin allthecodes
cargo build --workspace
cargo clippy --workspace --lib --bins
```
