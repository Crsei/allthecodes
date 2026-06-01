# TEAMMEM 团队共享记忆 — 实现分析

> 来源: claude-code-bun (TypeScript 原版) 实现分析，供 cc-rust 移植参考。

## 一、功能概述

TEAMMEM 是基于 GitHub 仓库的**团队共享记忆系统**。`memory/team/` 目录中的文件双向同步到 Anthropic 服务器，团队所有认证成员可共享项目知识。

**Feature Flag**: `FEATURE_TEAMMEM=1` (GrowthBook: `tengu_herring_clock`)

**前置条件**:
- Anthropic OAuth 认证 (first-party)
- 项目有 GitHub remote (`owner/repo` 作为同步 scope)
- Auto-memory 已启用

## 二、架构总览

```
┌─────────────────────────────────────────────────────────┐
│                    用户交互层                            │
│  teamMemPrompts.ts   teamMemCollapsed.tsx  teamMemSaved │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────┐
│                    工具集成层                            │
│  FileWriteTool → teamMemSecretGuard (写入前密钥检查)     │
│  extractMemories → 保存时区分 team/private              │
│  sessionFileAccessHooks → 读写计数 + 遥测              │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────┐
│                    核心同步层                            │
│  index.ts (pull/push/delta)                             │
│  watcher.ts (fs.watch + 2s debounce → auto push)        │
│  secretScanner.ts (gitleaks 30+ 规则)                   │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────┐
│                    路径安全层                            │
│  teamMemPaths.ts (穿越防护 + symlink 验证 + NFKC)       │
└────────────────────────┬────────────────────────────────┘
                         │
                    REST API
          /api/claude_code/team_memory
```

## 三、文件索引

| 文件 | 行数 | 职责 |
|------|------|------|
| `src/services/teamMemorySync/index.ts` | 1257 | 核心同步逻辑 (pull/push/delta/batch) |
| `src/services/teamMemorySync/watcher.ts` | 388 | 文件监视 + 自动同步触发 |
| `src/services/teamMemorySync/secretScanner.ts` | — | gitleaks 密钥扫描 (30+ 规则) |
| `src/services/teamMemorySync/types.ts` | — | Zod schema + 类型定义 |
| `src/services/teamMemorySync/teamMemSecretGuard.ts` | — | FileWrite/FileEdit 写入前密钥防护 |
| `src/memdir/teamMemPaths.ts` | 293 | 路径验证 + 目录管理 |
| `src/memdir/teamMemPrompts.ts` | — | 组合 team + private memory 的系统提示词 |
| `src/utils/teamMemoryOps.ts` | 89 | 工具函数 + UI 摘要生成 |
| `src/components/messages/teamMemCollapsed.tsx` | — | 读取/搜索 UI 折叠显示 |
| `src/components/messages/teamMemSaved.ts` | — | 保存记忆 UI 消息 |

## 四、数据结构

### 4.1 同步状态

```typescript
type SyncState = {
  lastKnownChecksum: string | null      // ETag (用于 If-Match 条件请求)
  serverChecksums: Map<string, string>  // 逐文件 sha256:<hex> 哈希
  serverMaxEntries: number | null       // 从 413 响应学习的服务端容量上限
}
```

### 4.2 服务端数据

```typescript
type TeamMemoryData = {
  organizationId: string
  repo: string                // "owner/repo"
  version: number
  lastModified: string
  checksum: string            // 全局 SHA-256 ("sha256:..." 前缀)
  content: {
    entries: Record<string, string>       // relPath → UTF-8 内容
    entryChecksums?: Record<string, string>  // relPath → sha256 哈希
  }
}
```

### 4.3 API 端点

```
GET  /api/claude_code/team_memory?repo={owner/repo}             → 完整数据 + entryChecksums
GET  /api/claude_code/team_memory?repo={owner/repo}&view=hashes → 仅 checksums (轻量冲突探测)
PUT  /api/claude_code/team_memory?repo={owner/repo}             → upsert entries (增量)
```

**Headers**: `Authorization: Bearer <oauth_token>`, `If-Match: <etag>` (push 时), `anthropic-beta`

## 五、同步流程

### 5.1 Pull (Server → Local)

```
pullTeamMemory(state)
    │
    ├── 检查 OAuth + GitHub remote
    │
    ├── fetchTeamMemory(state, repo, etag)
    │     ├── 304 Not Modified → skip
    │     ├── 404 → 服务端无数据
    │     └── 200 → 解析 TeamMemoryData
    │
    ├── 刷新 serverChecksums (per-key hashes)
    │
    └── writeRemoteEntriesToLocal(entries)
          ├── validateTeamMemKey() — 路径穿越验证
          ├── 文件大小检查 (> 250KB skip)
          ├── 内容比较 (相同则 skip)
          └── Promise.all 并行写入
```

### 5.2 Push (Local → Server)

```
pushTeamMemory(state)
    │
    ├── readLocalTeamMemory(maxEntries)
    │     ├── 递归扫描 memory/team/
    │     ├── 跳过 > 250KB 文件
    │     ├── scanForSecrets() — 密钥文件跳过
    │     └── 按 serverMaxEntries 截断
    │
    ├── 计算 delta = localHashes - serverChecksums
    │
    ├── batchDeltaByBytes(delta) → ≤200KB/批
    │
    └── 逐批 uploadTeamMemory()
          ├── 200 → 更新 serverChecksums + lastKnownChecksum
          ├── 412 (ETag 冲突) → fetchHashes() → 刷新 → 重试 (max 2)
          └── 413 (超容量) → 学习 serverMaxEntries
```

### 5.3 Watcher (文件监视)

```
startTeamMemoryWatcher()
    │
    ├── 初始 pull (启动时)
    │
    ├── fs.watch(memory/team/, {recursive: true})
    │     └── onChange → 2s debounce → pushTeamMemory()
    │
    ├── 抑制 pull 写入引起的假变更
    │
    ├── 永久失败抑制 (403, 404, no_oauth, no_repo)
    │     └── unlink 事件清除抑制 (恢复路径)
    │
    └── shutdown 时 best-effort flush
```

## 六、安全机制

### 6.1 路径穿越防护 (PSR M22186)

`validateTeamMemKey(relPath)` 检查:
- Null byte (`\0`) 检测
- URL 编码穿越 (`%2e%2e%2f`) 检测
- Unicode 正规化攻击 (NFKC) 防护
- 反斜杠 + 绝对路径拒绝
- Symlink 解析: `realpathDeepestExisting()` + realpath 比较

### 6.2 密钥扫描 (PSR M22174)

`secretScanner.ts` — 基于 gitleaks 规则:

| 类别 | 模式数 |
|------|--------|
| AWS (Access Key, Secret, Session Token) | 3 |
| GCP (Service Account JSON) | 1 |
| Azure (Storage, AD, Connection String) | 3 |
| Anthropic API Key | 1 |
| OpenAI API Key | 1 |
| GitHub (PAT, OAuth, App Key) | 3 |
| GitLab (PAT, Pipeline Token) | 2 |
| Slack (Bot/User/Webhook/Config Token) | 4 |
| 通用 (Private Key, JWT, Bearer Token) | 5+ |
| 其他 (Stripe, Twilio, SendGrid...) | 7+ |

**行为**: 密钥文件跳过上传，记录事件 (仅规则 ID，不记录密钥值)，不阻止其他文件同步。

### 6.3 FileWrite/FileEdit 拦截

`teamMemSecretGuard.ts`:
- 在 FileWriteTool/FileEditTool 的 `call()` 中调用
- 写入 team memory 路径的内容经过密钥扫描
- 检测到密钥时返回错误消息，阻止写入

## 七、集成点

### 7.1 启动初始化

```typescript
// src/setup.ts:365-368
if (feature('TEAMMEM')) {
  void import('./services/teamMemorySync/watcher.js')
    .then(m => m.startTeamMemoryWatcher())  // fire-and-forget
}
```

### 7.2 系统提示词

`teamMemPrompts.ts` 构建组合提示词:
- 四类记忆分类: user / feedback / project / reference
- 每类区分 scope: private (个人) vs team (团队共享)
- 指导模型将通用知识写入 `memory/team/`，个人偏好写入 `memory/` (private)

### 7.3 记忆提取

`extractMemories.ts`:
- 对话结束后提取持久记忆
- 区分 team memories 和 private memories 计数
- 记录 `team_memories_saved` 遥测事件

### 7.4 文件操作钩子

`sessionFileAccessHooks.ts`:
- 跟踪 team memory 的 read/write/search 操作
- Write/Edit 后调用 `notifyTeamMemoryWrite()` 触发 watcher push
- 统计计入遥测

## 八、关键设计决策

| 决策 | 理由 |
|------|------|
| **Server-wins on pull** | 保证团队成员总是拿到最新共识 |
| **Local-wins on push** | 用户正在编辑的内容不应被静默丢弃 |
| **Delta upload** | 只传哈希变化的条目，节省带宽 |
| **分批 PUT (≤200KB)** | 避免 API 网关 (~256-512KB) 拒绝 |
| **ETag 乐观锁** | 轻量级并发控制，412 时 probe `?view=hashes` 避免重下全量 |
| **服务端容量动态学习** | 不硬编码上限，从 413 的 `max_entries` 学习 |
| **2s debounce** | 防止快速编辑导致 push 风暴 |
| **永久失败抑制** | 403/404/no_oauth 不重试，避免 watcher 循环轰炸 API |

## 九、常量

```
MAX_FILE_SIZE_BYTES  = 250,000   // 单文件大小上限
MAX_PUT_BODY_BYTES   = 200,000   // 单批上传大小上限
MAX_RETRIES          = 3         // 暂时性失败重试
MAX_CONFLICT_RETRIES = 2         // 412 冲突重试
DEBOUNCE_MS          = 2,000     // watcher debounce
```

## 十、遥测事件

| 事件名 | 时机 |
|--------|------|
| `tengu_team_mem_sync_started` | 初始 pull 完成 |
| `tengu_team_mem_sync_pull` | 每次 pull |
| `tengu_team_mem_sync_push` | 每次 push |
| `tengu_team_mem_entries_capped` | 因 serverMaxEntries 截断 |
| `tengu_team_mem_secret_skipped` | 检测到密钥跳过 |
| `tengu_team_mem_push_suppressed` | 永久失败抑制 |
| `tengu_team_memdir_disabled` | feature 关闭 |

## 十一、cc-rust 移植要点

若要在 cc-rust 中实现 TEAMMEM，需关注:

1. **同步服务**: 核心 pull/push/delta 逻辑 → Rust async (reqwest + tokio)
2. **文件监视**: `fs.watch` → `notify` crate (跨平台文件监视)
3. **密钥扫描**: gitleaks 正则 → `regex` crate
4. **路径安全**: `validateTeamMemKey` → 路径穿越防护 (需处理 Windows 反斜杠)
5. **ETag 冲突**: reqwest 的 `If-Match` header + 412 重试逻辑
6. **Feature Gate**: 已有 `src/config/features.rs`，添加 `FEATURE_TEAMMEM`
7. **存储路径**: `~/.cc-rust/projects/<sanitized>/memory/team/`
8. **OAuth 依赖**: 已有 OAuth 实现，需添加 team memory API 端点
