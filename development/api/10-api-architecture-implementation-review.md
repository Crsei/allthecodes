# API Architecture Upgrade — Implementation Review

> 对照 `09-api-architecture-upgrade-plan.md` 审查当前代码的落地情况。
> 原始审查日期: 2026-06-07
>
> 2026-07-16 已按冻结快照 `d5e16fde..6d426dc9` 追加复核。下文原始
> 阶段记录保留历史语境；生成物状态以“2026-07-16 跟进复核”为准。

## 阶段进展总览

| 阶段 | 标题 | 完成度 | 状态 |
|------|--------|--------|--------|
| 阶段 0 | 协议定义 Crate | ~100% | ✅ 全部完成 |
| 阶段 1 | 从协议定义生成路由 | ~95% | ✅ 基本完成 |
| 阶段 2 | Processor 模式 | ~55% | ⚠️ 基础设施完备，已迁移 4/7 |
| 阶段 3 | 序列化作用域控制 | ~60% | ⚠️ `SessionOwnership` 未被替换 |
| 阶段 4 | 代码生成流水线 | ~90% | ⚠️ 生成器完成，自动写入/仓库 freshness gate 不完整 |
| 阶段 5 | 传输层抽象 | ~60% | ⚠️ Traits 与进程内 `DirectTransport` 已完成，可选 socket transport 尚未实现 |

---

## 阶段 0: Foundation — Protocol Definitions Crate ✅

**计划产出** | **实现情况**
---|---
`crates/allthecodes-protocol/` 脚手架 | ✅ Crate 已完成；原 `build.rs` 后续移除，改为显式 codegen CLI
`src/macros.rs` — `api_definitions!` 宏 | ✅ 完成，生成 `ClientRequest`/`ClientResponse`/`ApiMethod`/`ApiEndpoint`/`ApiTypeMetadata`/`SerializationPolicy`/`SerializationScope`/`ApiOperationMetadata`/`ALL_ENDPOINTS`/`API_METADATA`
`src/error.rs` — 结构化 `ApiError` | ✅ 含 `status_code()`、`into_body()`、`to_body()`、`ApiErrorBody`
`src/request.rs` — 请求类型 + 辅助函数 | ✅ `split_route`、`serialization_key`、`schema_for`
`src/response.rs` — 响应枚举 | ✅ 重导 `ClientResponse`
`src/notification.rs` — 服务器通知 | ⚠️ 已创建但为 `enum ServerNotification {}` 空枚举（二期增量正常）
`src/v1/` — 域类型模块 | ✅ 16 个域模块（agents、capabilities、chat、chat_modes、files、gateways、hooks、kanban、models、people、plugins、profiles、prompts、providers、skills、workspaces）
宏支持 `params`/`response`/`errors`/`serialization`/`#[experimental]` | ✅ 全部实现
~200+ endpoint 定义 | ✅ 覆盖协议宏中所有现有 API 路由
测试: serde 往返、状态码、作用域、去重 | ✅ 16 个测试通过

**问题**: 无。

---

## 阶段 1: Route Generation from Protocol Definitions ✅

**计划产出** | **实现情况**
---|---
`handler_registry.rs` — HandlerRegistry + 启动验证 | ✅ `HandlerRegistry` + `validate()` 检查缺失/重复/未标记 handler
`register_protocol_routes()` — 从注册表生成 Axum 路由 | ✅ 遍历注册表，按路径分组，聚合多方法路由
`all_api_handlers()` — 按域分组的工厂函数 | ✅ 返回 ~30 个域函数（chat、session、agent、file……）
向后兼容 shim | ✅ 通过 `HandlerRegistry` 扩展，无需重写旧 handler
调试端点 `ProtocolRoutes` (`GET /api/-/routes`) | ✅ 返回所有已知路由+其注册状态的 JSON

**注意**: 计划中的 `ClientRequest` 变体到 handler 映射已精简为 `ApiMethod` → `MethodRouter` 映射，因为 Axum 的 `MethodRouter`（`get()`/`post()` 等）比泛型 `Processor` 适配器更适合自由函数。

**问题**: 无。

---

## 阶段 2: Processor Pattern ⚠️

**计划产出** | **实现情况**
---|---
`Processor` trait + `process_processor` 泛型适配器 | ✅ `crate::processors::Processor` + `processor_json_handler`/`processor_no_params_handler` + `process_processor`
错误转换: `Processor::Error: Into<ProtocolApiError>` | ✅ `protocol_error_response()` 使用正确的 `ApiError.status_code()`
序列化层集成 | ✅ `serialization_layer()`、`serialization_scope()`、`serialization_key()` 关联方法
`CapabilitiesProcessor` | ✅ 已迁移（`processor_no_params_handler::<CapabilitiesProcessor>`）
其他 Processor 组 | ⚠️ **部分迁移** — Health、Session、File JSON endpoints 已迁移；Agent、Skill/Plugin、Chat 仍为旧自由函数或专用 transport

**迁移优先级与实际进度对比**:

| 计划优先级 | Processor | 实际状态 |
|------------|-----------|----------|
| 1 | HealthProcessor | ✅ 已完成 |
| 2 | CapabilitiesProcessor | ✅ 已完成 |
| 3 | SessionProcessor | ✅ 已完成 |
| 4 | AgentProcessor / PeopleProcessor | ❌ 未开始 |
| 5 | FileProcessor | ✅ 已完成（JSON endpoints；`FilesDownload` 保持二进制专用 transport） |
| 6 | SkillPluginProcessor | ❌ 未开始 |
| 7 | ChatProcessor | ❌ 未开始 |

**问题**:
- 多数非 Session/File handlers 未从 Processor 模式受益（标准错误路径、序列化集成）
- `handlers/capabilities.rs::capabilities_handler()` 是已注册的 REST shim，并通过
  `rest_processor_response::<CapabilitiesProcessor>` 调用 Processor；它不是死代码

---

## 阶段 3: Serialization Scoping ⚠️

**计划产出** | **实现情况**
---|---
`SerializationLayer` — per-key 信号量实现 | ✅ `run_scoped()`、`queue_key()` 已实现
`SerializationScope` 枚举 | ✅ `Concurrent`、`PerProcess`、`PerConnection`、`PerKey { field, key }`
协议定义中的序列化注解 | ✅ 多个 endpoint 包含 `serialization: PerKey("session_id")`、`serialization: PerProcess` 等
替换 `SessionOwnership` | ❌ **未完成** — `SerializationLayer` 和 `SessionOwnership` 同时存在。`try_claim_owner()`/`release_owner()` 仍由 chat/TUI/IPC WebSocket handlers 直接使用
`WebState` 重构 | ⚠️ `WebState` 包含 `serialization: SerializationLayer` 字段，但未删除旧的 `SessionOwner`/`SessionOwnership` 类型

**问题**: 新旧两套机制共存，增加了理解复杂度。

---

## 阶段 4: Code Generation Pipeline ⚠️

**计划产出** | **实现情况**
---|---
`codegen.rs` — TypeScript 类型生成器 | ✅ `generate_typescript_types()` 输出 V1ApiRequest、V1ApiResponse、V1ApiRoute、各个接口
`bin/codegen.rs` — 命令行入口 | ✅ 支持 `--check` 模式
`bin/schema-export.rs` — JSON Schema 导出 | ✅ `generate_schema_json_pretty()`
`bin/route-doc.rs` — 路由 Markdown 文档 | ✅ `generate_route_markdown()`
`bin/openapi-export.rs` — OpenAPI 3.0 导出 | ✅ `generate_openapi_json_pretty()`
`build.rs` — 构建时自动生成 TS 类型 | ❌ 当前 crate 无 `build.rs`；仅有显式 `codegen` CLI
`check_frontend_api_artifacts_up_to_date` — 前端生成物检查 | ✅ 覆盖 sibling frontend 的 types/routes/schema
生成的文件:
- `allthecodes-web/src/lib/generated/api-types.ts` | ✅ 已纳入 frontend artifact check
- `allthecodes-web/src/lib/generated/api-routes.ts` | ✅ 已纳入 frontend artifact check
- `allthecodes-web/src/lib/generated/api-schema.json` | ✅ 已纳入 frontend artifact check
- `docs/api/schema.json` | ⚠️ 有导出器，但未纳入仓库 diff check
- `docs/api/routes.md` | ⚠️ 协议生成段落为 207 条，当前生成源为 253 条
- `docs/api/openapi.json` | ⚠️ 有导出器，但未纳入仓库 diff check

**问题**:
- `codegen` 默认推断 sibling frontend 路径；CI 应显式传入/固定输出目录，避免
  检查到错误 checkout。
- `route-doc`、`schema-export`、`openapi-export` 只向 stdout 输出；CI 没有校验
  `docs/api/*` 重新生成后是否产生 diff。

---

## 阶段 5: Transport Abstraction ⚠️

**计划产出** | **实现情况**
---|---
`Transport` trait | ✅ `send_request()`、`send_notification()`、`supports_streaming()`
`MessageProcessor` trait | ✅ `process_request()`、`process_notification()`
`JsonRpcFrame` — JSON-RPC 2.0 格式 | ✅ Request、Response、Error、Notification 变体
`DirectTransport<P>` — 进程内传输 | ✅ 带可选 streaming 支持
`TransportError` | ✅ ConnectionClosed、Unsupported、Protocol、Serialization
WebSocket 传输 | ❌ 未实现（计划标记为可选）
Unix 套接字传输 | ❌ 未实现（计划标记为可选）

**问题**: 与计划一致（Phase 5 标记为可选，2–3 个 sprint 即可）。

---

## 跨阶段问题

### ✅ 1. `providers.rs` 编译错误已修复

**文件**: `crates/allthecodes-web/src/handlers/providers.rs`
**提交**: `58cfceb56`（2026-06-07）
**历史症状**:
```
error[E0631]: type mismatch in function arguments
    fn normalized_json_object(value: Option<Value>) -> Option<Value>
```
**原因**: `normalized_json_object` 的签名曾为 `fn(Option<Value>) -> Option<Value>`，但多处调用 `.and_then(normalized_json_object)` 需要 `fn(Value) -> Option<Value>`。

**当前状态**: 已修复。`normalized_json_object` 当前签名为 `fn normalized_json_object(value: Value) -> Option<Value>`，不再阻止 `allthecodes-web` crate 编译。

---

### 🟡 2. 双重 ApiError 类型

- `allthecodes_protocol::error::ApiError` — 结构化错误，含 `status_code()`、`into_body()`、结构化细节（用于处理器路径）
- `handlers::ApiError` — 扁平遗留错误 `{ error: String, code: String }`（用于遗留 handler 路径）

`api_fallback_handler()` 返回的是遗留格式，与处理器路径的格式不同：

```json
// 遗留格式 (api_fallback_handler)
{ "error": "API endpoint not implemented", "code": "capability_not_implemented" }

// 协议格式 (processors.rs)
{ "error": "...", "code": "not_found", "details": { "entity": "session", "id": "..." } }
```

**修复**: 迁移 `api_fallback_handler` 和所有遗留 handlers 使用 `allthecodes_protocol::ApiError`。

---

### 🟡 3. Experimental 门控未执行

协议定义中已有 `experimental_reason()` 方法和 `#[experimental("...")]` 语法支持，但没有 Axum middleware 或 Processor 层检查该标记并返回 `403 Experimental`。当前没有任何 endpoint 标记为 experimental。

---
### 🟡 4. 无 API 版本前缀

成功标准中写明 **"All new endpoints use `/api/v2/` prefix"**，但所有路由仍在 `/api/` 下。

---

### 🟢 5. 生成文件所有权

`allthecodes-web/src/lib/generated/` 三件套由 codegen 写入。如果多个
agent 并行工作，可能导致竞态条件。当前通过 `write_if_changed()` 已有缓解
（仅当内容不同时才写入）。

---

## 测试状态（2026-06-07 原始快照）

```
cargo test -p allthecodes-protocol
├── 16 单元测试 (main crate)        — 全部通过
└── 1 集成测试 (check_ts_types_up_to_date) — 通过

cargo check -p allthecodes-web       — providers.rs 阻断项已修复；需按当前工作区改动重新验证
cargo check -p allthecodes-protocol  — 通过
```

---

## 行动建议

优先级排序：

1. **🔴 高 — 让已提交 API 生成物可验证** — 同一提交生成并检查
   `docs/api/{routes.md,schema.json,openapi.json}` 与 frontend 三件套。
2. **🔴 高 — 补齐 ApiDispatcher 覆盖与迁移 tracker** — 当前 JSON-RPC dispatch 已覆盖 Health、Capabilities、Session、File JSON endpoints；其余 REST 路由仍未统一进入 dispatcher
3. **🟡 中 — 继续 Processor 迁移** — HealthProcessor、SessionProcessor、FileProcessor 已完成；下一步扩展到 Agent/Skill/Chat
4. **🟡 中 — 统一 ApiError** — 消除双重 ApiError 类型，迁移 `api_fallback_handler`
5. **🟡 中 — 替换 SessionOwnership** — 完全移除旧的 `try_claim_owner()` / `release_owner()`，仅保留 `SerializationLayer`
6. **🟢 低 — 添加 Experimental gating middleware**
7. **🟡 中 — 让 streaming event 进入 codegen metadata** — 当前生成器只遍历
   `API_METADATA` 引用的请求/响应类型；`Chat` 的 `Value` 响应不会自动带出
   permission SSE event schema

---

## 2026-07-16 跟进复核

### 范围与计数

- `development/api/` 上次提交更新为 `d5e16fde`；当前复核 HEAD 为
  `6d426dc9`（冻结快照）。
- 直接运行当前 `route-doc` 生成器得到 253 条 protocol operation。
- 已提交 `docs/api/routes.md` 的 protocol-generated 主表只有 207 条；文件
  末尾另有 2 条手写 daemon remote-control route，不参与 protocol 计数。
- 因此当前确认的生成物漂移是 46 条 operation。它表示文档/CI 不新鲜，
  不等于 46 个运行时 handler 都缺失。

### 当前结论

| 项目 | 当前状态 | 必要动作 |
|---|---|---|
| Protocol metadata | 253 条 operation 是当前 contract source | 继续以 `API_METADATA`/registry 为单一来源 |
| Web handler registry | 可以按 operation 验证 registered/unregistered | 保留启动校验与 `/api/-/routes` 诊断 |
| Frontend 生成物 | `codegen --check` 可检查 types/routes/schema | CI 必须在固定 sibling/output 目录运行，缺目录不可静默算通过 |
| Backend `docs/api/*` | routes/schema/OpenAPI 导出器存在，但 committed files 无 freshness gate | 生成到临时文件、比较三件套并在 diff 时失败 |

### 与产品 API 缺口的边界

生成物新鲜度是跨域 P0 基础设施问题。实际产品缺口仍需逐项验证 handler
是否连接真实 owner；本轮确认的 Security Web transport、Memory、Skills
proposal、Jobs、Workflow、Web IPC Agent/Team、Group Chat、Backend Services 与
Discovery 缺口见
[最近功能 API 缺口审计](13-recent-feature-api-gap-audit.md)。具体生成与 CI
验收见 [Generated API Artifact Freshness Plan](17-api-generated-artifact-freshness-plan.md)。

---

*附录: 原始架构实现受影响的文件清单*

```
# 已创建的新文件
crates/allthecodes-protocol/Cargo.toml
crates/allthecodes-protocol/build.rs                (历史文件；当前已移除)
crates/allthecodes-protocol/src/lib.rs
crates/allthecodes-protocol/src/macros.rs
crates/allthecodes-protocol/src/request.rs
crates/allthecodes-protocol/src/response.rs
crates/allthecodes-protocol/src/notification.rs
crates/allthecodes-protocol/src/error.rs
crates/allthecodes-protocol/src/transport.rs
crates/allthecodes-protocol/src/codegen.rs
crates/allthecodes-protocol/src/v1/mod.rs
crates/allthecodes-protocol/src/v1/*.rs           (16 域模块)
crates/allthecodes-protocol/src/bin/codegen.rs
crates/allthecodes-protocol/src/bin/schema-export.rs
crates/allthecodes-protocol/src/bin/route-doc.rs
crates/allthecodes-protocol/src/bin/openapi-export.rs

# 已修改的现有文件
crates/allthecodes-web/src/mod.rs                  (build_router 重构)
crates/allthecodes-web/src/state.rs                (添加 serialization 字段)
crates/allthecodes-web/src/handler_registry.rs     (新增)
crates/allthecodes-web/src/serialization.rs        (新增)
crates/allthecodes-web/src/processors.rs           (新增)
crates/allthecodes-web/src/handlers/mod.rs         (添加重导、ApiError)

# 已生成的文件
allthecodes-web/src/lib/generated/api-types.ts     (TS 类型)
allthecodes-web/src/lib/generated/api-routes.ts    (TS 路由元数据)
allthecodes-web/src/lib/generated/api-schema.json  (前端 schema)
docs/api/schema.json                               (JSON Schema，360KB)
docs/api/routes.md                                 (路由文档，~200 行)
docs/api/openapi.json                              (OpenAPI 3.0，237KB)
```
