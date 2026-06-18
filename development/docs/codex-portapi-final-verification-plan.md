# Codex Port API 最终复验计划

> 计划日期：2026-06-17
> 范围：剩余目标全部代码阶段完成后的文档、schema、构建和手动复验

## 文档与 Schema

- 更新 `development/docs/api-interface-analysis.md`：
  - gateway `/events` 新 response shape。
  - 新 `/timeline` endpoint。
  - local socket / named pipe transport。
  - ResponseEvent provider streaming contract。
- 更新 `docs/api/routes.md`。
- 更新 `docs/api/openapi.json` 和 `docs/api/schema.json`。
- 更新 `development/docs/codex-portapi-phase-execution-plan.md`：
  - Phase 4b、local transport、Phase 6 migration 的执行记录。
  - 移除过期“当前优先级建议”。

## Static Assets 复验

- backend-only 构建：
  - `/` 返回 unbundled/static fallback。
- `web-ui` feature 构建：
  - `cargo build -p allthecodes --bin allthecodes --features web-ui`
  - 启动 feature-built binary。
  - 验证 `/` 返回 embedded HTML。
  - 验证 `/api/web/health` 或 `/healthz` 返回 200。
- 如果 binary 15s 内未 bind 端口：
  - 记录 stdout/stderr。
  - 检查 readiness path 是否被 server lifecycle 阻塞。
  - 修复后重新记录命令。

## Workspace 验证

```bash
cargo test -p allthecodes-gateway
cargo test -p allthecodes-daemon
cargo test -p allthecodes-server
cargo test -p allthecodes-ipc
cargo test -p allthecodes-api
cargo test -p allthecodes-engine
cargo test -p allthecodes-web
cargo check -p allthecodes --bin allthecodes
cargo build --workspace
cargo clippy --workspace --lib --bins
```

## 手动验收

- `--listen local://auto`
- `--listen all://web=127.0.0.1:17322,daemon=127.0.0.1:19836,local=auto`
- Gateway run:
  - create run
  - stream output
  - read `/events?after_seq=0`
  - read `/timeline`
  - stop run
- Agent output:
  - spawn background agent
  - query output with `after_seq=0`
  - query again with returned `next_seq`
- Provider:
  - probe Anthropic/OpenAI-compatible/Google without model request.
  - fixture-test Bedrock/Vertex error classification.
