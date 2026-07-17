# Provider Profile CRUD 与运行时支持计划

日期：2026-07-18

## 目标

- 新增 Rust TUI `/providers` 管理入口，支持查看、创建、完整替换、删除和激活用户 provider profile。
- 将 TUI、Web provider API 和既有 profile API 统一到无损配置存储层，修复更新、重命名、删除和导入时的数据丢失或 active profile 悬空问题。
- 所有 profile 类型均支持持久化 CRUD；17 个静态 provider、Bedrock 和 Vertex 可激活运行；Foundry、未知/custom/ACP profile 保持可管理但不可激活。
- 内置 provider preset 与用户 profile 分离并保持只读，所有读取和输出均隐藏秘密值。

## 实现范围

1. 在共享底层 crate 维护 provider 标识、别名与运行时支持状态，供配置校验、API registry、客户端构造和 TUI 共用。
2. 在 `allthecodes-config` 新增统一 profile store，提供 list/get/create/patch/replace/rename/delete/activate；保留无关 RawSettings，拒绝删除 active profile，重命名 active profile 时同步 active ID。
3. PATCH 支持“缺失即保留、null 即清除”；PUT 精确替换全部非秘密字段。秘密字段使用显式 keep/set/clear，读取、日志、错误和快照只返回脱敏状态。
4. 修复 profile 切换时旧 provider/model/base URL/env 泄漏；运行时直接按 active profile 构建相应客户端。静态 registry 的全部 provider 以及 Bedrock、Vertex 使用 profile-aware 配置，Foundry 和非 LLM profile 返回明确的不可激活错误。
5. 新增 `/providers` 列表、详情、创建/替换向导、激活和删除确认；preset 只读。秘密输入通过 typed UI action 提交，不拼接到 slash-command 文本。
6. 新增 `GET /api/providers/{id}` 和 `PUT /api/providers/{id}`，保留现有 list/create/patch/delete 兼容行为；既有 `/api/profiles` handlers 改用同一存储层并保持数据无损。
7. 更新 protocol schema、`docs/api/*`、`development/archive/KNOWN_ISSUES.md` 和本任务 worktree HTML artifact。

## 验证

- 覆盖 profile store CRUD、精确替换、秘密 keep/set/clear、active 删除冲突、active 重命名、无关设置保留和失败回滚。
- 覆盖全部 registry provider 的持久化 CRUD；验证 17 个静态 provider、Bedrock、Vertex 的离线客户端构造，以及 Foundry、未知/custom/ACP 的激活拒绝。
- 覆盖 Web provider/profile handler 无数据丢失、协议路由与 schema/codegen。
- 覆盖 TUI 向导、秘密遮罩、带空格 ID、激活、替换、删除确认与 `/providers` PTY 快照。
- 按顺序执行 fmt、clippy、非 PTY workspace lib tests、相关定向测试、PTY 单线程测试、workspace release build。
- 在独立 `allthecodes-web` worktree 执行 API contract 生成与检查，不修改现有 dirty checkout，不提交 `src/lib/generated/*`。

## 工作流

- 本计划先在主分支单独提交。
- 从该提交创建 `.worktrees/provider-profile-crud` / `worktree/provider-profile-crud`，代码、测试、文档和 artifact 均在该 worktree 修改和提交。
- 最终仅 fast-forward 合并回 `allthecodes`，推送后删除 worktree 与临时分支；所有提交只显式暂存本任务路径。
