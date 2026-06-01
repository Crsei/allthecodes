# IPC 重构执行计划

> 目标：在不改现有 JSONL 协议的前提下，把 Rust 的 IPC 从“单文件协调器”重构成“协议稳定、运行时可测、职责清晰”的结构。

## 范围

- 主要改动目录：`rust/src/ipc`
- 重点文件：
  - `rust/src/ipc/headless.rs`
  - `rust/src/ipc/protocol.rs`
  - `rust/src/ipc/subsystem_handlers.rs`
  - `rust/src/ipc/agent_handlers.rs`
- 参考文档：
  - `rust/architecture/ink-terminal-frontend.md`
  - `rust/architecture/ARCHITECTURE.md`

## 非目标

- 第一阶段不改 `FrontendMessage` / `BackendMessage` 的 JSON 结构
- 不同时改 TS 前端协议
- 不引入新依赖
- 不顺手重写 subsystem / team 业务逻辑

## 总体原则

1. 先锁行为，再拆结构
2. 先拆实现，不改协议
3. 每一阶段都保持可回退
4. 优先减少 `headless.rs` 的职责密度

## 阶段 0：锁行为

先补测试，覆盖现有关键行为。

至少锁住以下 6 类：

1. 协议序列化/反序列化：`FrontendMessage`、`BackendMessage`
2. `SdkMessage -> BackendMessage` 映射
3. permission request/response 往返
4. AskUserQuestion 当前行为
5. slash command 触发 query 的路径
6. agent / subsystem event 转发

### 阶段 0 输出

- 新增或整理测试，建议放在：
  - `rust/src/ipc/protocol.rs`
  - `rust/src/ipc/headless.rs`
  - 或拆到 `rust/src/ipc/tests/` 子模块

## 阶段 1：提取纯映射层

从 `headless.rs` 提取 `SdkMessage -> BackendMessage` 逻辑到新文件：

- `rust/src/ipc/sdk_mapper.rs`

建议搬出的内容：

- `handle_sdk_message`
- `handle_stream_event`
- `extract_tool_result_output`
- `generate_and_send_suggestions`

目标：

- `headless.rs` 不再包含大段 `SdkMessage` 匹配分发逻辑
- 映射逻辑可独立测试

### 阶段 1 完成标准

- `headless.rs` 中不再承担主要消息映射职责
- 所有映射测试继续通过

## 阶段 2：提取 query turn 启动逻辑

当前两条路径都在做重复的 query 启动：

- `FrontendMessage::SubmitPrompt`
- slash command 的 `CommandResult::Query`

建议新增：

- `rust/src/ipc/query_runner.rs`

提取统一入口：

- `spawn_query_turn(engine, prompt_text, message_id, source, suggestion_svc, sink)`

目标：

- 普通 prompt 与 slash command query 复用同一条逻辑
- 避免两条路径逐渐漂移

### 阶段 2 完成标准

- query 启动逻辑只有一份
- 行为保持一致
- 不改协议

## 阶段 3：引入统一输出口 `FrontendSink`

当前多个文件都直接调用 `send_to_frontend()`：

- `headless.rs`
- `subsystem_handlers.rs`
- `agent_handlers.rs`

建议新增：

- `rust/src/ipc/sink.rs`

第一版只做薄包装：

- `FrontendSink::send(BackendMessage)`
- `FrontendSink::send_many(iter)`

后续再演进成单写者 `mpsc` 模型。

### 阶段 3 完成标准

- stdout 写出逻辑统一收口
- handler 不再直接依赖底层输出函数

## 阶段 4：把 `run_headless()` 拆成运行时对象

建议新增文件：

- `rust/src/ipc/runtime.rs`
- `rust/src/ipc/callbacks.rs`
- `rust/src/ipc/ingress.rs`

目标分工：

- `runtime.rs`
  - `tokio::select!` 主循环
  - runtime 状态持有
- `callbacks.rs`
  - permission callback 安装
  - ask-user callback 安装
- `ingress.rs`
  - `FrontendMessage` 分发

此后：

- `headless.rs` 只保留兼容入口 `run_headless()`
- 真正的运行时逻辑移入 `HeadlessRuntime`

建议 runtime 收口的状态：

- `pending_permissions`
- `pending_questions`
- `agent_rx`
- `event_rx`
- `suggestion_svc`
- `sink`

### 阶段 4 完成标准

- `headless.rs` 变薄
- `runtime.rs` 承担主循环
- ingress、callback、egress 各自可测

## 阶段 5：整理 handler 边界

现在 subsystem / agent handler 都是“收到命令就直接发消息”。

建议改成返回值模式，而不是副作用模式：

- `async fn handle_xxx(...) -> Vec<BackendMessage>`

或者：

```rust
enum IpcEffect {
    Outbound(BackendMessage),
    ReplaceConversation(...),
    SpawnQuery(...),
    None,
}
```

由主循环统一执行 effect。

### 阶段 5 完成标准

- handler 不再直接依赖输出层
- 单元测试可直接断言返回的消息 / effect

## 阶段 6：协议修正 AskUserQuestion

这是唯一建议在后半程改协议的点。

当前行为：

- 后端通过 `SystemInfo("Question: ...")` 把问题发给前端
- 前端下一条 `submit_prompt` 被复用为回答

问题：

- 语义不明确
- 无法支持并发问题
- UI 草稿/连续输入会让行为变脆

建议新增正式协议：

- `BackendMessage::QuestionRequest { id, text }`
- `FrontendMessage::QuestionResponse { id, text }`

等 UI 适配完成后，再删除旧路径。

### 阶段 6 完成标准

- 不再依赖“下一条 submit_prompt 就是回答”
- 问答关联显式按 `id`

## 阶段 7：协议文件整理

当前 `protocol.rs` 已经同时容纳：

- 基础对话协议
- subsystem 扩展
- agent 扩展
- team 扩展

建议在 runtime 稳定后再拆：

- `protocol/base.rs`
- `protocol/subsystem.rs`
- `protocol/agent.rs`
- `protocol/team.rs`
- `protocol/mod.rs`

这一阶段只做文件布局，不改字段语义。

## 提交切片建议

1. `test(ipc): lock protocol and headless behavior`
2. `refactor(ipc): extract sdk message mapper`
3. `refactor(ipc): unify query turn spawning`
4. `refactor(ipc): add frontend sink abstraction`
5. `refactor(ipc): introduce headless runtime and ingress modules`
6. `refactor(ipc): make agent and subsystem handlers return effects`
7. `feat(ipc): add explicit question request/response protocol`
8. `refactor(ipc): split protocol module by concern`

## 每阶段必跑验证

- `cargo test`
- `cargo build`

重点关注：

- `protocol.rs` 的序列化测试
- `headless` 相关测试
- 如有 headless 端到端测试，也一并运行

## 风险排序

### 高风险

- AskUserQuestion 协议修正

### 中风险

- `run_headless()` 拆成运行时对象
- 统一 sink 后的消息顺序

### 低风险

- `SdkMessage` mapper 提取
- query turn 启动逻辑去重
- protocol 文件拆分

## 首批建议落地文件

### 新增

- `rust/src/ipc/sdk_mapper.rs`
- `rust/src/ipc/query_runner.rs`
- `rust/src/ipc/sink.rs`
- `rust/src/ipc/runtime.rs`
- `rust/src/ipc/ingress.rs`
- `rust/src/ipc/callbacks.rs`

### 修改

- `rust/src/ipc/headless.rs`
- `rust/src/ipc/mod.rs`
- `rust/src/ipc/subsystem_handlers.rs`
- `rust/src/ipc/agent_handlers.rs`

## 建议执行顺序

1. 先做阶段 0-2
2. 再做阶段 3-5
3. 最后决定是否推进阶段 6 的协议升级
4. 阶段 7 只在前面稳定后再做

## 一句话总结

这次 IPC 重构不应该从“改协议”开始，而应该从：

- 锁行为
- 拆 `headless.rs`
- 收口输出
- 清理职责边界

开始。

真正值得最后再动的是协议本身，尤其是 AskUserQuestion 这一块。
