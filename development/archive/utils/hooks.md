# Hook 系统移植缺口

对应: `claude-code-bun/src/utils/hooks/`  
Rust 对应: `cc-tools/hooks/`, `cc-types/hooks.rs`, `cc-engine/worktree_hooks.rs`

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| hooksConfigManager.ts | 400 | Hook 配置管理 |
| AsyncHookRegistry.ts | 309 | 异步 Hook 生命周期 |
| sessionHooks.ts | 447 | 会话级函数 Hook |
| hookEvents.ts | 192 | 通用 Hook 事件系统 |
| hooksSettings.ts | 271 | Hook 设置 |
| execAgentHook.ts | 341 | Agent Hook 执行 |
| execHttpHook.ts | 243 | HTTP Hook 执行 |
| execPromptHook.ts | 214 | Prompt Hook 执行 |
| fileChangedWatcher.ts | 191 | 文件变更检测 |
| skillImprovement.ts | 299 | 技能改进 Hook |
| ssrfGuard.ts | 294 | SSRF 防护 |
| postSamplingHooks.ts | 70 | 采样后 Hook |
| registerFrontmatterHooks.ts | 67 | Frontmatter Hook 注册 |
| registerSkillHooks.ts | 64 | Skill Hook 注册 |
| hooksConfigSnapshot.ts | 133 | 配置快照 |
| hookHelpers.ts | 83 | 共享工具 |
| apiQueryHookHelper.ts | 144 | API 查询 Hook |
| **合计** | **~4,051** | |

## Rust 已实现

### cc-tools/hooks/
- `pre_tool.rs` / `post_tool.rs`: 工具执行前后的 Hook 调用
- `execution.rs`: Hook 执行基础框架
- `http_hook.rs`: HTTP Hook POST 执行、header env allowlist、SSRF guard 接入
- `async_registry.rs`: 异步 Hook pending/response registry 基线
- `hook_events.rs`: Hook started/progress/response event emitter 基线
- `ssrf_guard.rs`: HTTP Hook SSRF 地址过滤
- `mod.rs`: Hook 模块组织

### cc-types/hooks.rs
- Hook 相关类型定义

### cc-engine/hooks/ + cc-engine/worktree_hooks.rs
- Hook 配置元数据、session hook store、frontmatter/skill hook 注册基线
- post-sampling hook registry、api query helper、skill improvement/file watcher 结构
- WorktreeCreate/Remove 钩子集成

## Rust 缺失的主要功能

### 1. Hook 配置管理 — 部分缺失
- 已有配置加载和事件元数据基线
- 仍缺完整加载/保存/更新 UX、配置 snapshot/比较、malformed config 的结构化诊断

### 2. 异步 Hook / 事件系统 — 基线完成，runtime 集成待补
- 已有 pending registry、response collection、hook started/progress/response emitter
- 仍缺与真实 long-running hook process 生命周期、UI 附件和进度轮询的完整 runtime parity

### 3. 会话级函数 Hook — 基线完成，回调执行待补
- 已有 `add_function_hook()` / `remove_function_hook()` 和 session store
- 仍缺真实 in-memory callback 执行和按 Agent 生命周期的完整临时 Hook 接入

### 4. Hook 执行后端 — 部分缺失
- command hook 已接
- HTTP hook 已接 Pre/Post/Stop/event 路径并包含 SSRF guard
- **execAgentHook.ts** (341 行): Agent 事件 Hook 执行 — 缺失
- **execPromptHook.ts** (214 行): Prompt Hook 执行 — 缺失
- prompt/agent hook 当前显式报错，避免静默跳过

### 5. 文件变更检测和 skill improvement — 缺失
- `fileChangedWatcher.ts`: 当前 Rust 只有 watch path/notifier 结构，未接 notify watcher
- `skillImprovement.ts`: 当前 Rust 只有开关/类型结构，未接 LLM 分析与文件写回

### 6. Hook 快照和比较 (133 行) — 缺失
- `hooksConfigSnapshot.ts`
- 配置变更检测

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| Pre/Post Hook 执行 | command/http hooks | pre_tool.rs / post_tool.rs / http_hook.rs | ✅ command/http 基线已接 |
| Hook 类型定义 | hooksSettings.ts + types | hooks.rs | ✅ 已移植 |
| **Hook 配置管理** | hooksConfigManager.ts (400 行) | config_manager.rs + settings load | 部分缺口 |
| **异步注册表** | AsyncHookRegistry.ts (309 行) | async_registry.rs | runtime 集成缺口 |
| **会话 Hook** | sessionHooks.ts (447 行) | session_hooks.rs | callback 执行缺口 |
| **HTTP WebHook** | execHttpHook.ts (243 行) | http_hook.rs | ✅ 基线已接 |
| **SSRF 防护** | ssrfGuard.ts (294 行) | ssrf_guard.rs | ✅ 基线已接 |
| **事件系统** | hookEvents.ts (192 行) | hook_events.rs | runtime/UI 集成缺口 |
| **Frontmatter/Skill** | register*.ts | registration.rs | 基线已接 |
| 快照管理 | hooksConfigSnapshot.ts | 无 | 功能缺口 |

## 当前剩余缺口（2026-05-18 review 后）

- Prompt hook 和 Agent hook 尚未接真实 LLM/runtime 执行；配置此类 hook 会显式失败而非静默跳过。
- FileChanged watcher 未使用 `notify` 等后端监听真实文件事件。
- Skill improvement 未接 post-sampling LLM 分析和 `.cc-rust/skills/{name}/SKILL.md` 写回。
- Hook config snapshot/比较、配置保存/更新 UX、malformed config 诊断仍缺。
- Async registry / hook events 已有基线，但还需要贯通真实进程生命周期、UI 附件和进度轮询。
