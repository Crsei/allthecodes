# SQLite 集成计划

本目录包含将 SQLite（通过 sqlx）引入 allthecodes 的设计文档。

## 文档索引

| 文档 | 描述 | 状态 |
|------|------|------|
| [PLAN.md](PLAN.md) | 总体架构与阶段性实施计划 | ✅ 草稿 |
| [SCHEMA.md](SCHEMA.md) | 分库设计与表结构定义 | ✅ 草稿 |
| [MIGRATION_GUIDE.md](MIGRATION_GUIDE.md) | 逐 crate 迁移方案 (JSON→SQLite) | ✅ 草稿 |

## 核心决策

- **库**: sqlx 0.8（已存在于 workspace Cargo.toml）
- **无 ORM**：全部使用 raw SQL + `QueryBuilder`
- **多库分离**：按领域拆分为 3–4 个 `.sqlite` 文件
- **嵌入式迁移**：编译时嵌入 `.sql` 文件
- **WAL 模式**：并发读写性能
- **参考实现**: `allthecodes-web-state`（已有）与 codex `codex-state`（外部参考）

## 已确认的迁移决策

- **迁移顺序**：先迁移 `allthecodes-tasks`，再迁移 `allthecodes-session`
- **Task output**：大文本 output 继续存文件，SQLite 只存路径、摘要、大小、截断状态等元数据
- **兼容策略**：保留 feature flag 与 JSON fallback，便于回滚和读取旧数据
- **旧数据导入**：初期不包含旧 JSON 数据 backfill；稳定后再单独设计导入命令
