# Team Memory 客户端同步设计

> 日期: 2026-04-11
> 状态: 已批准
> 前置: [`../../archive/superpowers/specs/2026-04-11-team-memory-design.md`](../../archive/superpowers/specs/2026-04-11-team-memory-design.md) (服务端已实现)

## 一、概述

在已有的 Team Memory TS 服务进程中，新增客户端同步逻辑：启动时从 server 拉取团队记忆到本地 `memory/team/` 目录，监视本地文件变更并自动推送增量到 server。

## 二、架构

```
TS Server 进程 (ui/team-memory-server/)
    │
    ├── HTTP Server (index.ts)     — 处理 API 请求 (已实现)
    │
    └── SyncClient (新增)
          ├── 启动时 pull (server → local)
          ├── fs.watch + 2s debounce → push (local → server)
          └── 抑制 pull 写入触发的假变更
```

同步目标:
- Pull: `GET /api/claude_code/team_memory` → 写入 `memory/team/`
- Push: 本地文件变更 → `PUT /api/claude_code/team_memory` (增量)

注意: SyncClient 直接调用 `db.ts` 中的函数读写数据库，不经过 HTTP — 它运行在同一进程内。

## 三、同步状态

```typescript
interface SyncState {
  lastKnownChecksum: string | null;      // 全局 ETag (用于 If-None-Match / If-Match)
  serverChecksums: Map<string, string>;  // per-file "sha256:<hex>" 哈希
  teamMemPath: string;                   // memory/team/ 绝对路径
  repo: string;                          // "owner/repo"
  suppressSet: Set<string>;              // pull 写入的路径，抑制 watcher 触发
}
```

`SyncState` 在进程内存中维护，不持久化。进程重启时重新 pull 构建。

## 四、Pull 流程 (Server → Local)

启动时执行一次。

```
pull(state)
    │
    ├── 从数据库读取 repo 的所有 entries + checksums
    │   (直接调用 db.getEntries / db.getEntryChecksums / db.getRepoMeta)
    │
    ├── 刷新 state.serverChecksums
    │
    └── writeEntriesToLocal(entries, state)
          ├── 遍历 entries
          ├── 路径: path.join(state.teamMemPath, key)
          │
          ├── 跳过 > 250KB 的 entry
          │
          ├── 读取本地文件内容 (如果存在)
          │   └── 内容相同 → 跳过写入
          │
          ├── 将 key 加入 state.suppressSet (抑制 watcher)
          │
          ├── 确保父目录存在 (mkdirSync recursive)
          │
          └── writeFileSync(filePath, content, 'utf-8')
```

## 五、Push 流程 (Local → Server)

由 watcher debounce 触发。

```
push(state)
    │
    ├── readLocalTeamMemory(state.teamMemPath)
    │   ├── 递归扫描 memory/team/ 目录
    │   ├── 跳过 > 250KB 文件
    │   ├── 读取内容，计算 sha256
    │   └── 返回 Map<relPath, { content, checksum }>
    │
    ├── 计算 delta = localChecksums - state.serverChecksums
    │   (只包含 checksum 不同或 server 不存在的 key)
    │
    ├── delta 为空 → 返回 (无需上传)
    │
    └── upsert delta 到数据库 (直接调用 db.upsertEntries)
          │
          ├── 成功 → 更新 state.serverChecksums + state.lastKnownChecksum
          │
          └── (数据库写入不会有 ETag 冲突，因为同进程内)
```

注意: 因为 SyncClient 和 HTTP Server 在同一进程中，push 直接写数据库而非通过 HTTP API。这避免了自己给自己发 HTTP 请求的尴尬，也避免了共享密钥自引用问题。

## 六、Watcher

### 6.1 文件监视

```typescript
fs.watch(state.teamMemPath, { recursive: true }, (eventType, filename) => {
  if (!filename) return;

  const fullPath = path.join(state.teamMemPath, filename);

  // 抑制由 pull 写入触发的假变更
  if (state.suppressSet.has(filename)) {
    state.suppressSet.delete(filename);
    return;
  }

  // Debounce: 重置 2s 定时器
  resetDebounceTimer(() => push(state));
});
```

### 6.2 Debounce

```typescript
let debounceTimer: Timer | null = null;

function resetDebounceTimer(fn: () => void): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(fn, DEBOUNCE_MS);
}
```

`DEBOUNCE_MS = 2000`

### 6.3 假变更抑制

当 pull 写入文件时，将 relative path 加入 `suppressSet`。watcher 收到该路径的事件时，从 set 中删除并跳过 push。

## 七、repo 参数传递

### 7.1 Rust 端

Rust daemon spawn TS 进程时，新增 `--repo` 和 `--team-mem-path` 参数：

```rust
// team_memory_proxy.rs spawn_team_memory_server()
let repo = get_github_repo();  // 从 git remote 解析 owner/repo
let team_mem_path = get_team_mem_path(&cwd);  // ~/.cc-rust/projects/<sanitized>/memory/team/

Command::new("bun")
    .arg("run").arg(&script_path)
    .arg("--port").arg(port.to_string())
    .arg("--secret").arg(&secret)
    .arg("--repo").arg(&repo)
    .arg("--team-mem-path").arg(&team_mem_path)
    // ...
```

### 7.2 Git Remote 解析

```rust
/// 从 git remote -v 解析 owner/repo。
/// 支持 HTTPS (github.com/owner/repo.git) 和 SSH (git@github.com:owner/repo.git)
fn get_github_repo() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_github_repo(&url)
}

fn parse_github_repo(url: &str) -> Option<String> {
    // HTTPS: https://github.com/owner/repo.git
    // SSH: git@github.com:owner/repo.git
    // 提取 owner/repo，去掉 .git 后缀
}
```

### 7.3 Team Memory 路径计算

```rust
fn get_team_mem_path(cwd: &Path) -> PathBuf {
    let sanitized = sanitize_path(cwd);  // 将 cwd 转为安全的目录名
    dirs::home_dir()
        .unwrap_or_default()
        .join(".cc-rust")
        .join("projects")
        .join(sanitized)
        .join("memory")
        .join("team")
}
```

### 7.4 TS 端接收

`index.ts` 的 `parseArgs()` 扩展：

```typescript
function parseArgs(): { port: number; secret: string; repo: string; teamMemPath: string } {
  // 新增 --repo 和 --team-mem-path 解析
}
```

## 八、index.ts 启动流程变更

```typescript
// 现有: DB init → HTTP Server 启动
// 新增: HTTP Server 启动后，初始化 SyncClient

const { port, secret, repo, teamMemPath } = parseArgs();

// ... 现有 DB init + HTTP Server ...

// 初始化同步 (仅当 repo 和 teamMemPath 都提供时)
if (repo && teamMemPath) {
  const state: SyncState = {
    lastKnownChecksum: null,
    serverChecksums: new Map(),
    teamMemPath,
    repo,
    suppressSet: new Set(),
  };

  // 确保 memory/team/ 目录存在
  mkdirSync(teamMemPath, { recursive: true });

  // 启动时 pull
  pull(state);

  // 启动 watcher
  startWatcher(state);

  // 优雅关闭时 flush
  process.on("SIGTERM", () => {
    push(state);  // best-effort flush
    db.close();
    process.exit(0);
  });
}
```

## 九、文件布局

```
ui/team-memory-server/
├── index.ts        修改: 解析 --repo/--team-mem-path, 启动 SyncClient
├── db.ts           不变
├── routes.ts       不变
├── sync.ts         新增: pull() / push() / readLocalTeamMemory() / writeEntriesToLocal()
└── watcher.ts      新增: startWatcher() / debounce / suppressSet 管理
```

Rust 端:
```
src/daemon/
├── team_memory_proxy.rs    修改: spawn 时传 --repo / --team-mem-path
└── (新增辅助函数)          get_github_repo() / get_team_mem_path()
```

## 十、常量

```
DEBOUNCE_MS          = 2000      // watcher debounce
MAX_FILE_SIZE        = 250_000   // 跳过超大文件 (pull + push)
MAX_CONFLICT_RETRIES = 2         // 412 冲突重试 (预留，当前同进程不会冲突)
```

## 十一、错误处理

| 场景 | 行为 |
|------|------|
| `--repo` 或 `--team-mem-path` 缺失 | 同步不启动，仅 HTTP server 运行 |
| pull 时数据库为空 | 正常 — 无 entries，跳过写入 |
| push 扫描目录不存在 | 创建目录，返回空 map |
| 文件读取失败 (权限) | 跳过该文件，log 警告 |
| watcher 创建失败 | log 错误，同步降级为仅 pull (无自动 push) |

## 十二、不在本次实现范围

- 密钥扫描 (push 前检测凭证)
- 路径穿越防护 (validateTeamMemKey)
- 遥测事件
- 前端 UI 组件
- 定时 pull (仅启动时一次)
- 删除条目同步 (本地删除文件不同步到 server)
