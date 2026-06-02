# Team Memory Client Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add client-side sync to the Team Memory TS server: pull entries from DB to local `memory/team/` on startup, watch for local file changes, push deltas back to DB.

**Architecture:** Extend the existing `ui/team-memory-server/` Bun process with two new modules: `sync.ts` (pull/push/delta logic operating directly on DB) and `watcher.ts` (fs.watch + debounce). Rust daemon passes `--repo` and `--team-mem-path` CLI args when spawning the TS process.

**Tech Stack:** TypeScript/Bun (fs.watch, crypto), Rust (git2, std::process::Command)

**Spec:** `docs/superpowers/specs/2026-04-11-team-memory-sync-design.md`

---

## File Structure

### TS 端

| 文件 | 职责 |
|------|------|
| Create: `ui/team-memory-server/sync.ts` | pull/push/delta 核心逻辑，直接调用 db.ts |
| Create: `ui/team-memory-server/watcher.ts` | fs.watch + 2s debounce + suppressSet 管理 |
| Modify: `ui/team-memory-server/index.ts` | 解析 --repo/--team-mem-path，启动 SyncClient |

### Rust 端

| 文件 | 职责 |
|------|------|
| Modify: `src/daemon/team_memory_proxy.rs` | spawn 时传 --repo / --team-mem-path |
| Modify: `src/utils/git.rs` | 新增 `get_remote_url()` 和 `parse_github_repo()` |

### db.ts 新增导出

需要在 `db.ts` 中导出 `sha256` 函数（当前是私有的），供 `sync.ts` 使用。

---

## Task 1: db.ts — 导出 sha256 函数

**Files:**
- Modify: `ui/team-memory-server/db.ts`

- [ ] **Step 1: 将 `sha256` 从私有改为导出**

在 `ui/team-memory-server/db.ts` 中，将第 67 行的 `function sha256` 改为 `export function sha256`：

```typescript
// 改前:
function sha256(content: string): string {
// 改后:
export function sha256(content: string): string {
```

- [ ] **Step 2: 验证编译**

Run: `cd ui && bun build team-memory-server/index.ts --target bun --outdir /tmp/tm-check`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add ui/team-memory-server/db.ts
git commit -m "refactor(team-memory): export sha256 from db.ts for sync module"
```

---

## Task 2: sync.ts — Pull/Push 核心逻辑

**Files:**
- Create: `ui/team-memory-server/sync.ts`

- [ ] **Step 1: 创建 `sync.ts`**

```typescript
// ui/team-memory-server/sync.ts
import { readFileSync, writeFileSync, readdirSync, existsSync, mkdirSync, statSync } from "fs";
import { join, relative, dirname } from "path";
import * as db from "./db";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_FILE_SIZE = 250_000;

// ---------------------------------------------------------------------------
// SyncState
// ---------------------------------------------------------------------------

export interface SyncState {
  lastKnownChecksum: string | null;
  serverChecksums: Map<string, string>;
  teamMemPath: string;
  repo: string;
  suppressSet: Set<string>;
}

export function createSyncState(repo: string, teamMemPath: string): SyncState {
  return {
    lastKnownChecksum: null,
    serverChecksums: new Map(),
    teamMemPath,
    repo,
    suppressSet: new Set(),
  };
}

// ---------------------------------------------------------------------------
// Pull: DB → Local filesystem
// ---------------------------------------------------------------------------

export function pull(state: SyncState): void {
  const entries = db.getEntries(state.repo);
  const checksums = db.getEntryChecksums(state.repo);
  const meta = db.getRepoMeta(state.repo);

  // Refresh server checksums
  state.serverChecksums.clear();
  for (const [key, hash] of Object.entries(checksums)) {
    state.serverChecksums.set(key, hash);
  }
  state.lastKnownChecksum = meta?.checksum ?? null;

  // Write entries to local filesystem
  writeEntriesToLocal(entries, state);

  const count = Object.keys(entries).length;
  if (count > 0) {
    console.log(`team-memory-sync: pulled ${count} entries to ${state.teamMemPath}`);
  }
}

function writeEntriesToLocal(entries: Record<string, string>, state: SyncState): void {
  for (const [key, content] of Object.entries(entries)) {
    // Skip oversized entries
    if (new TextEncoder().encode(content).byteLength > MAX_FILE_SIZE) {
      console.warn(`team-memory-sync: skipping oversized entry: ${key}`);
      continue;
    }

    const filePath = join(state.teamMemPath, key);

    // Skip if content is identical
    if (existsSync(filePath)) {
      try {
        const local = readFileSync(filePath, "utf-8");
        if (local === content) continue;
      } catch {
        // File unreadable, overwrite it
      }
    }

    // Add to suppress set before writing (prevents watcher from triggering push)
    state.suppressSet.add(key);

    // Ensure parent directory exists
    const dir = dirname(filePath);
    mkdirSync(dir, { recursive: true });

    writeFileSync(filePath, content, "utf-8");
  }
}

// ---------------------------------------------------------------------------
// Push: Local filesystem → DB
// ---------------------------------------------------------------------------

export function push(state: SyncState): void {
  const localEntries = readLocalTeamMemory(state.teamMemPath);

  // Compute delta: only entries whose checksum differs from server
  const delta: Record<string, string> = {};
  for (const [key, { content, checksum }] of localEntries) {
    const serverHash = state.serverChecksums.get(key);
    if (serverHash !== checksum) {
      delta[key] = content;
    }
  }

  if (Object.keys(delta).length === 0) return;

  // Write directly to DB (same process, no HTTP needed)
  const newChecksum = db.upsertEntries(state.repo, delta);

  // Update sync state
  state.lastKnownChecksum = newChecksum;
  const newChecksums = db.getEntryChecksums(state.repo);
  state.serverChecksums.clear();
  for (const [key, hash] of Object.entries(newChecksums)) {
    state.serverChecksums.set(key, hash);
  }

  console.log(`team-memory-sync: pushed ${Object.keys(delta).length} entries`);
}

// ---------------------------------------------------------------------------
// Read local directory
// ---------------------------------------------------------------------------

function readLocalTeamMemory(
  teamMemPath: string
): Map<string, { content: string; checksum: string }> {
  const result = new Map<string, { content: string; checksum: string }>();

  if (!existsSync(teamMemPath)) return result;

  function walk(dir: string): void {
    let entries: string[];
    try {
      entries = readdirSync(dir);
    } catch {
      return;
    }
    for (const entry of entries) {
      const fullPath = join(dir, entry);
      let stat;
      try {
        stat = statSync(fullPath);
      } catch {
        continue;
      }
      if (stat.isDirectory()) {
        walk(fullPath);
      } else if (stat.isFile()) {
        if (stat.size > MAX_FILE_SIZE) continue;
        try {
          const content = readFileSync(fullPath, "utf-8");
          const key = relative(teamMemPath, fullPath).replace(/\\/g, "/");
          const checksum = db.sha256(content);
          result.set(key, { content, checksum });
        } catch {
          // Skip unreadable files
        }
      }
    }
  }

  walk(teamMemPath);
  return result;
}
```

- [ ] **Step 2: 验证编译**

Run: `cd ui && bun build team-memory-server/sync.ts --target bun --outdir /tmp/tm-check`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add ui/team-memory-server/sync.ts
git commit -m "feat(team-memory): add sync.ts with pull/push/delta logic"
```

---

## Task 3: watcher.ts — 文件监视 + Debounce

**Files:**
- Create: `ui/team-memory-server/watcher.ts`

- [ ] **Step 1: 创建 `watcher.ts`**

```typescript
// ui/team-memory-server/watcher.ts
import { watch, existsSync, mkdirSync } from "fs";
import type { SyncState } from "./sync";
import { push } from "./sync";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEBOUNCE_MS = 2000;

// ---------------------------------------------------------------------------
// Watcher
// ---------------------------------------------------------------------------

let debounceTimer: Timer | null = null;

function resetDebounceTimer(fn: () => void): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(fn, DEBOUNCE_MS);
}

/**
 * Start watching the `memory/team/` directory for changes.
 * On change, debounce and push deltas to DB.
 */
export function startWatcher(state: SyncState): void {
  // Ensure the directory exists before watching
  if (!existsSync(state.teamMemPath)) {
    mkdirSync(state.teamMemPath, { recursive: true });
  }

  try {
    const watcher = watch(state.teamMemPath, { recursive: true }, (_eventType, filename) => {
      if (!filename) return;

      // Normalize path separators (Windows → forward slash)
      const key = filename.replace(/\\/g, "/");

      // Suppress changes triggered by pull writes
      if (state.suppressSet.has(key)) {
        state.suppressSet.delete(key);
        return;
      }

      // Debounce: accumulate rapid changes, push once
      resetDebounceTimer(() => {
        try {
          push(state);
        } catch (err) {
          console.error("team-memory-sync: push error:", err);
        }
      });
    });

    // Cleanup on process exit
    process.on("SIGTERM", () => watcher.close());
    process.on("SIGINT", () => watcher.close());

    console.log(`team-memory-sync: watching ${state.teamMemPath}`);
  } catch (err) {
    console.error("team-memory-sync: failed to start watcher:", err);
    // Degrade gracefully — pull worked, but no auto-push
  }
}

/**
 * Flush any pending debounced push immediately.
 * Call this before shutdown to avoid losing recent changes.
 */
export function flushPendingPush(state: SyncState): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
    try {
      push(state);
    } catch (err) {
      console.error("team-memory-sync: flush error:", err);
    }
  }
}
```

- [ ] **Step 2: 验证编译**

Run: `cd ui && bun build team-memory-server/watcher.ts --target bun --outdir /tmp/tm-check`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add ui/team-memory-server/watcher.ts
git commit -m "feat(team-memory): add watcher.ts with fs.watch + debounce"
```

---

## Task 4: index.ts — 集成 SyncClient 启动

**Files:**
- Modify: `ui/team-memory-server/index.ts`

- [ ] **Step 1: 扩展 `parseArgs` 返回类型**

修改 `index.ts` 的 `parseArgs` 函数，新增 `--repo` 和 `--team-mem-path` 解析：

```typescript
function parseArgs(): { port: number; secret: string; repo: string; teamMemPath: string } {
  const args = process.argv.slice(2);
  let port = 19837;
  let secret = "";
  let repo = "";
  let teamMemPath = "";
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--port" && args[i + 1]) {
      port = parseInt(args[i + 1], 10);
      i++;
    } else if (args[i] === "--secret" && args[i + 1]) {
      secret = args[i + 1];
      i++;
    } else if (args[i] === "--repo" && args[i + 1]) {
      repo = args[i + 1];
      i++;
    } else if (args[i] === "--team-mem-path" && args[i + 1]) {
      teamMemPath = args[i + 1];
      i++;
    }
  }
  if (!secret) {
    console.error("error: --secret is required");
    process.exit(1);
  }
  return { port, secret, repo, teamMemPath };
}
```

- [ ] **Step 2: 添加 sync 导入和启动逻辑**

在 `index.ts` 顶部添加导入：

```typescript
import { createSyncState, pull, push } from "./sync";
import { startWatcher, flushPendingPush } from "./watcher";
```

在 `const { port, secret } = parseArgs();` 改为：

```typescript
const { port, secret, repo, teamMemPath } = parseArgs();
```

在 HTTP server 启动之后（`console.log` 行之后），添加 sync 初始化：

```typescript
// --- Sync Client ---
if (repo && teamMemPath) {
  mkdirSync(teamMemPath, { recursive: true });

  const syncState = createSyncState(repo, teamMemPath);

  // Pull on startup
  try {
    pull(syncState);
  } catch (err) {
    console.error("team-memory-sync: initial pull failed:", err);
  }

  // Start file watcher
  startWatcher(syncState);

  // Flush on shutdown (override existing handlers)
  const originalSigterm = () => {
    flushPendingPush(syncState);
    db.close();
    process.exit(0);
  };
  const originalSigint = () => {
    flushPendingPush(syncState);
    db.close();
    process.exit(0);
  };
  // Remove old handlers and add new ones
  process.removeAllListeners("SIGTERM");
  process.removeAllListeners("SIGINT");
  process.on("SIGTERM", originalSigterm);
  process.on("SIGINT", originalSigint);

  console.log(`team-memory-sync: active for ${repo} at ${teamMemPath}`);
} else {
  console.log("team-memory-sync: disabled (no --repo or --team-mem-path)");
}
```

- [ ] **Step 3: 验证编译**

Run: `cd ui && bun build team-memory-server/index.ts --target bun --outdir /tmp/tm-check`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add ui/team-memory-server/index.ts
git commit -m "feat(team-memory): wire sync client into index.ts startup"
```

---

## Task 5: Rust — 添加 git remote URL 解析

**Files:**
- Modify: `src/utils/git.rs`

- [ ] **Step 1: 添加 `get_remote_url` 和 `parse_github_repo` 函数**

在 `src/utils/git.rs` 的末尾（`#[cfg(test)]` 之前）添加：

```rust
/// Get the URL of the `origin` remote for the repository at `path`.
pub fn get_remote_url(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;
    let remote = repo
        .find_remote("origin")
        .map_err(|e| anyhow::anyhow!("no origin remote: {}", e))?;
    remote
        .url()
        .map(|u| u.to_string())
        .ok_or_else(|| anyhow::anyhow!("origin remote has no URL"))
}

/// Parse a GitHub remote URL into `owner/repo` format.
///
/// Supports:
/// - HTTPS: `https://github.com/owner/repo.git`
/// - SSH: `git@github.com:owner/repo.git`
pub fn parse_github_repo(url: &str) -> Option<String> {
    let url = url.trim();

    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let repo = rest.strip_suffix(".git").unwrap_or(rest);
        if repo.contains('/') {
            return Some(repo.to_string());
        }
    }

    // HTTPS: https://github.com/owner/repo.git
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let repo = rest.strip_suffix(".git").unwrap_or(rest);
        if repo.contains('/') {
            // Take only owner/repo (ignore extra path segments)
            let parts: Vec<&str> = repo.splitn(3, '/').collect();
            if parts.len() >= 2 {
                return Some(format!("{}/{}", parts[0], parts[1]));
            }
        }
    }

    None
}
```

- [ ] **Step 2: 添加测试**

在 `src/utils/git.rs` 的 `#[cfg(test)] mod tests` 中添加：

```rust
#[test]
fn parse_github_repo_https() {
    assert_eq!(
        parse_github_repo("https://github.com/owner/repo.git"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn parse_github_repo_https_no_git_suffix() {
    assert_eq!(
        parse_github_repo("https://github.com/owner/repo"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn parse_github_repo_ssh() {
    assert_eq!(
        parse_github_repo("git@github.com:owner/repo.git"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn parse_github_repo_ssh_no_suffix() {
    assert_eq!(
        parse_github_repo("git@github.com:owner/repo"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn parse_github_repo_non_github() {
    assert_eq!(parse_github_repo("https://gitlab.com/owner/repo.git"), None);
}

#[test]
fn parse_github_repo_invalid() {
    assert_eq!(parse_github_repo("not-a-url"), None);
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test utils::git::tests::parse_github_repo`
Expected: 所有 6 个测试 PASS

- [ ] **Step 4: Commit**

```bash
git add src/utils/git.rs
git commit -m "feat(team-memory): add get_remote_url and parse_github_repo to git utils"
```

---

## Task 6: Rust — spawn 时传 --repo / --team-mem-path

**Files:**
- Modify: `src/daemon/team_memory_proxy.rs`

- [ ] **Step 1: 修改 `spawn_team_memory_server` 签名和逻辑**

修改函数签名，接收 `cwd`:

```rust
pub async fn spawn_team_memory_server(
    base_port: u16,
    cwd: &std::path::Path,
) -> anyhow::Result<(Child, u16, String)> {
```

在 `let secret = ...` 之后、构建 `Command` 之前，添加 repo 和 team-mem-path 解析：

```rust
    // Resolve GitHub repo from git remote.
    let repo = crate::utils::git::get_remote_url(cwd)
        .ok()
        .and_then(|url| crate::utils::git::parse_github_repo(&url));

    // Compute team memory path: ~/.cc-rust/projects/<sanitized>/memory/team/
    let team_mem_path = {
        let sanitized = cwd
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>();
        dirs::home_dir()
            .unwrap_or_default()
            .join(".cc-rust")
            .join("projects")
            .join(&sanitized)
            .join("memory")
            .join("team")
    };
```

在 `Command::new("bun")` 的 `.arg(&secret)` 之后、`.stdout(Stdio::piped())` 之前，添加可选参数：

```rust
    let mut cmd = Command::new("bun");
    cmd.arg("run")
        .arg(&script_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--secret")
        .arg(&secret);

    if let Some(ref repo_str) = repo {
        cmd.arg("--repo").arg(repo_str);
    }
    cmd.arg("--team-mem-path")
        .arg(team_mem_path.to_string_lossy().as_ref());

    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
```

同时记录日志：

```rust
    info!(
        port,
        script = %script_path.display(),
        repo = repo.as_deref().unwrap_or("none"),
        team_mem_path = %team_mem_path.display(),
        "spawning team-memory-server"
    );
```

- [ ] **Step 2: 更新 main.rs 中的调用**

在 `src/main.rs` 中，将 `spawn_team_memory_server(cli.port)` 改为传入 cwd:

找到:
```rust
daemon::team_memory_proxy::spawn_team_memory_server(cli.port).await
```

改为:
```rust
daemon::team_memory_proxy::spawn_team_memory_server(cli.port, std::path::Path::new(&cwd)).await
```

(其中 `cwd` 是 main.rs 中已有的当前工作目录变量。检查 main.rs 中 cwd 的获取方式并使用相同的变量。)

- [ ] **Step 3: 编译验证**

Run: `cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: 解决 warnings**

检查并修复任何未使用的 import 或变量警告。

- [ ] **Step 5: Commit**

```bash
git add src/daemon/team_memory_proxy.rs src/main.rs
git commit -m "feat(team-memory): pass --repo and --team-mem-path to TS server subprocess"
```

---

## Task 7: 端到端验证

**Files:**
- 无新文件

- [ ] **Step 1: 手动测试 — TS server 独立运行**

```bash
# 创建测试目录
mkdir -p /tmp/test-team-mem

# 启动 TS server with sync
cd ui && bun run team-memory-server/index.ts \
  --port 19837 --secret test123 \
  --repo test/repo --team-mem-path /tmp/test-team-mem
```

Expected: 输出包含:
- `team-memory-server listening on http://127.0.0.1:19837`
- `team-memory-sync: active for test/repo at /tmp/test-team-mem`

- [ ] **Step 2: 通过 API 写入数据，验证 pull 到本地**

在另一个终端：

```bash
# PUT some entries
curl -s -X PUT "http://127.0.0.1:19837/api/claude_code/team_memory?repo=test/repo" \
  -H "x-team-memory-secret: test123" \
  -H "If-Match: *" \
  -H "Content-Type: application/json" \
  -d '{"entries":{"project.md":"# Project","rules.md":"## Rules"}}'

# Restart the TS server (Ctrl+C then re-run)
# On restart, pull should write project.md and rules.md to /tmp/test-team-mem/

# Verify files exist
cat /tmp/test-team-mem/project.md  # Should show "# Project"
cat /tmp/test-team-mem/rules.md    # Should show "## Rules"
```

- [ ] **Step 3: 验证本地变更自动 push**

```bash
# While TS server is running, create a new file
echo "# New file" > /tmp/test-team-mem/new.md

# Wait 3 seconds (2s debounce + margin)
sleep 3

# Verify it was synced to DB via API
curl -s "http://127.0.0.1:19837/api/claude_code/team_memory?repo=test/repo" \
  -H "x-team-memory-secret: test123" | python3 -m json.tool
```

Expected: 响应中 `content.entries` 包含 `"new.md": "# New file\n"`

- [ ] **Step 4: 清理**

```bash
rm -rf /tmp/test-team-mem
rm -f ~/.cc-rust/team-memory.db
```

---

## Task 8: Warning 清理 + 最终验证

**Files:**
- 可能涉及所有新增/修改的文件

- [ ] **Step 1: 编译检查 warnings**

Run: `cargo build 2>&1`
检查所有 warning，逐一修复

- [ ] **Step 2: 运行 Rust 测试**

Run: `cargo test`
Expected: 所有测试 PASS（除已知的 flaky test_from_env 外）

- [ ] **Step 3: 验证 TS 编译**

Run: `cd ui && bun build team-memory-server/index.ts --target bun --outdir /tmp/tm-check`
Expected: 编译成功

- [ ] **Step 4: 如有修复，commit**

```bash
git add -A
git commit -m "chore(team-memory): resolve warnings and cleanup for sync client"
```
