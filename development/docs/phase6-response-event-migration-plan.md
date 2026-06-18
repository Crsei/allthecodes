# Phase 6：ResponseEvent 全链路迁移计划

> 计划日期：2026-06-17
> 依赖：已落地 provider runtime primitives

## 目标

将 provider streaming public contract 从 `StreamEvent` 全链路迁移到 `ResponseEvent`，并补齐 Bedrock/Vertex 对 `ProviderEndpoint`、`ProviderErrorKind`、metadata capture 的接入。

## 类型边界

- 新增或替换共享类型 `ResponseEvent`。
- `ResponseEvent` 覆盖现有 `StreamEvent` 语义：
  - message start
  - content block start/delta/stop
  - message delta
  - message stop
- request correlation / provider metadata 挂到 response start 或独立 metadata event。
- `SdkStreamEvent` 改为 `SdkResponseEvent`，SDK output 序列化字段同步改名。
- `QueryYield::Stream` 改为 `QueryYield::Response`。

## Provider Runtime

- `StreamProvider` trait 改为返回 `ResponseEvent` stream。
- Anthropic/OpenAI-compatible/Google 保持现有 wire parser 行为，只改内部输出类型。
- Bedrock:
  - endpoint 构建纳入 `ProviderEndpoint`。
  - EventStream frame parser 输出 `ResponseEvent`。
  - HTTP error 走 `ProviderErrorKind` classification。
- Vertex:
  - endpoint 构建纳入 `ProviderEndpoint`。
  - auth/header/base URL 仍由 Vertex 模块负责。
  - streaming parser 输出 `ResponseEvent`。

## Downstream Migration

- API client `messages_stream` 返回 `ResponseEvent` stream。
- query deps、query loop、loop helpers、recovery tests 改用 `ResponseEvent`。
- lifecycle stream handler、TUI engine event mapper、agent sdk-to-agent mapper 改用新名称。
- 测试 mocks 全量替换，避免保留 `StreamEvent` public alias。

## Compatibility

- 这是 public Rust API breaking migration。
- 不保留 `pub type StreamEvent = ResponseEvent`，防止新代码继续依赖旧名称。
- JSON 序列化的 event variant shape 尽量保持原内容字段，减少前端渲染改动；只调整外层类型命名相关字段。

## Tests

- `cargo test -p allthecodes-api`
  - provider runtime fixture classification。
  - Bedrock/Vertex parser fixture 输出 `ResponseEvent`。
  - metadata/request id 捕获。
- `cargo test -p allthecodes-engine`
  - query loop streaming、tool use、recovery、tombstone 场景。
- `cargo test -p allthecodes-query`
  - query crate mirror tests。
- `cargo test -p allthecodes`
  - TUI/SDK mapper tests。
- `cargo check --workspace`
  - 确认没有残留 `StreamEvent` public references。
