# Docker 容器化测试指南

在 Docker 中构建和测试 `claude-code-rs`，消除"在我机器上能跑"问题，实现可复现的 CI 测试环境。

## 实施状态 (2026-04-09)

**已完成**，两套镜像均已验证通过。

### 镜像一览

| 镜像 | Dockerfile | 说明 |
|------|-----------|------|
| `cc-rust-test` | `Dockerfile.ci` | Rust 后端 + 118 个 e2e 测试 |
| `cc-rust-ui-test` | `ui/Dockerfile.test` | OpenTUI UI + 234 个单元测试 |

### 运行命令

```bash
# ── Rust 后端测试 ────────────────────────────────────────
# 构建（首次 ~2 分钟，后续增量 ~30 秒）
docker build --target tester -t cc-rust-test -f Dockerfile.ci .

# 运行全部离线测试
docker run --rm cc-rust-test

# 运行 live API 测试（从 .env 读取 key）
docker run --rm --env-file .env cc-rust-test \
  sh -c 'env "CARGO_BIN_EXE_claude-code-rs=/usr/local/bin/claude-code-rs" \
    /app/tests/e2e_terminal-* --ignored --test-threads=1'

# 使用 docker compose（自动从 .env 加载 API key）
docker compose -f docker-compose.test.yml run --rm test-offline
docker compose -f docker-compose.test.yml run --rm test-live

# ── OpenTUI UI 测试 ──────────────────────────────────────
# 构建（含 Rust 编译 + Bun 安装）
docker build -t cc-rust-ui-test -f ui/Dockerfile.test .

# 运行 UI 单元测试
docker run --rm cc-rust-ui-test
```

### 测试结果

**Rust 后端（`cc-rust-test`）：**

```
=== cc-rust E2E test suite (Linux container) ===
e2e_audit_export:    6 passed
e2e_cli:            20 passed
e2e_compact:        12 passed
e2e_env:            12 passed
e2e_services:        5 passed, 5 ignored (live)
e2e_session_export:  8 passed
e2e_terminal:       26 passed, 16 ignored (live)
e2e_tools:          29 passed
=== Done: 8 suites passed, 0 failed ===
```

**OpenTUI UI（`cc-rust-ui-test`）：**

```
234 pass, 0 fail, 602 expect() calls across 11 files [267ms]

覆盖范围：
  app-store reducer        — 3 tests
  ANSI parser              — 20 tests
  CSI 序列生成/解析        — 34 tests
  OSC 序列（超链接、标题） — 16 tests
  DEC 模式（交替屏幕等）   — 15 tests
  键盘输入解析             — 50 tests（含 kitty 协议、鼠标事件）
  屏幕渲染/diff/blit       — 30 tests
  文本换行/截断            — 16 tests
  帧优化器                 — 17 tests
  Tokenizer                — 17 tests
```

### 文件清单

| 文件 | 说明 |
|------|------|
| `Dockerfile.ci` | Rust 后端多阶段构建：`rust:latest` → `debian:trixie-slim` tester → release |
| `ui/Dockerfile.test` | UI 测试镜像：`rust:latest`（编译二进制）→ `oven/bun:latest`（运行测试） |
| `.dockerignore` | 排除 target/、logs/、node_modules/ |
| `docker-compose.test.yml` | test-offline（无网络）+ test-live（`env_file: .env` 自动加载 key） |
| `tests/test_workspace.rs` | 跨平台 workspace 函数（`E2E_WORKSPACE` env → 平台默认） |
| `tests/e2e_terminal/helpers.rs` | `workspace()` + `binary_path()`（`which` fallback） |

### 实施中解决的问题

| 问题 | 原因 | 解决方案 |
|------|------|---------|
| `globset 0.4.18 requires edition2024` | `rust:1.82` 太旧 | 改用 `rust:latest`（需要 1.88+） |
| `GLIBC_2.39 not found` | builder 用 trixie 但 tester 用 bookworm | tester 改为 `debian:trixie-slim` |
| `CARGO_BIN_EXE_claude-code-rs is unset` | 容器外运行测试二进制，无 cargo env | 用 `env` 命令在 run-tests.sh 中注入 |
| `CARGO_BIN_EXE_claude-code-rs: bad variable name` | 连字符在 POSIX shell 中非法 | 用 `env "NAME=VALUE"` 而非 `export` |
| `assert_cmd::cargo_bin()` panic in Docker | 编译时 env 不存在 | `which` crate fallback + `catch_unwind` |
| `cwd should contain 'temp'` | 容器中 workspace 是 `/tmp/cc-rust-test` | 断言改为检查 workspace 目录名 |
| `F:\temp` 硬编码 48 处 | 18 个测试文件 | `workspace()` 函数 + `E2E_WORKSPACE` env |

---

## 可行性评估

### 依赖分析

| 依赖 | Linux 容器兼容性 | 说明 |
|------|-----------------|------|
| `rustls-tls`（reqwest） | ✅ 零系统依赖 | 不需要 openssl-dev |
| `keyring` v3 | ✅ file-backend | Linux 无 dbus 时自动降级为文件存储 |
| `crossterm` | ✅ 纯 Rust | 无系统依赖 |
| `ratatui` | ✅ 纯 Rust | 无系统依赖 |
| `portable-pty`（dev） | ⚠️ 需要 PTY 支持 | 容器内可用，但 `docker exec` 需 `-t` 分配 TTY |

**结论：可以在 Linux 容器中交叉编译和测试，无需 Windows 容器。**

### 三种 Docker 测试模式

```
模式 A: 容器内编译 + 测试（最简单，最慢）
         Docker (Linux) → cargo build → cargo test

模式 B: 交叉编译 + 容器测试（推荐）
         Host (Windows) → cross build --target linux → Docker (Linux) → 运行测试

模式 C: 多阶段构建（CI/CD 推荐）
         Dockerfile: stage1 编译 → stage2 最小运行时 → 测试
```

---

## 模式 A：容器内完整编译 + 测试

最简单的方式。容器内安装 Rust 工具链，编译并运行所有测试。

### Dockerfile

```dockerfile
FROM rust:latest

# 系统依赖（编译期需要的最小集合）
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 先复制 Cargo 文件利用 Docker 层缓存
COPY Cargo.toml Cargo.lock ./

# 创建空 src 触发依赖预编译
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release 2>/dev/null; \
    cargo build --release --tests 2>/dev/null; \
    rm -rf src

# 复制完整源码
COPY src/ src/
COPY tests/ tests/

# 编译
RUN cargo build --release

# 创建测试工作区
RUN mkdir -p /tmp/test-workspace

# 运行离线测试（不需要 API key）
CMD ["cargo", "test", "--release", "--", "--skip", "ignored"]
```

### 使用

```bash
# 构建镜像
docker build -t cc-rust-test -f Dockerfile.test .

# 运行离线测试
docker run --rm cc-rust-test

# 运行特定测试
docker run --rm cc-rust-test cargo test --test e2e_terminal

# 运行 live 测试（传入 API key）
docker run --rm \
  -e ANTHROPIC_API_KEY="sk-ant-..." \
  cc-rust-test \
  cargo test --test e2e_terminal -- --ignored

# 交互式调试
docker run --rm -it cc-rust-test bash
```

### 优缺点

```
+ 最简单，无需交叉编译
+ 完全隔离的环境
+ Docker 层缓存加速重复构建

- 首次编译慢（5-15 分钟，依赖数量大）
- 镜像大（rust:bookworm ~1.5GB + 编译产物）
- 每次源码修改需要重新编译（增量编译需 volume）
```

---

## 模式 B：交叉编译 + 容器测试（推荐）

在 Windows 主机上用 `cross` 交叉编译 Linux 二进制，然后在轻量容器中运行测试。

### 安装 cross

```bash
cargo install cross
```

`cross` 内部使用 Docker 镜像提供交叉编译工具链，无需手动配置 linker。

### 交叉编译

```bash
# 编译 Linux 二进制
cross build --release --target x86_64-unknown-linux-gnu

# 编译测试二进制
cross test --release --target x86_64-unknown-linux-gnu --no-run
```

### 测试容器（轻量）

```dockerfile
# Dockerfile.test-runner
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
RUN mkdir -p /tmp/test-workspace

# 二进制从主机复制进来
COPY target/x86_64-unknown-linux-gnu/release/claude-code-rs /usr/local/bin/

# 测试二进制也复制进来
COPY target/x86_64-unknown-linux-gnu/release/deps/e2e_terminal-* /app/tests/

ENTRYPOINT ["/bin/bash"]
```

### 使用

```bash
# 1. 交叉编译
cross build --release --target x86_64-unknown-linux-gnu

# 2. 构建测试镜像
docker build -t cc-rust-runner -f Dockerfile.test-runner .

# 3. 运行
docker run --rm cc-rust-runner /app/tests/e2e_terminal-xxxxx
```

### 更简单：直接用 cross test

```bash
# cross 自动处理 Docker + 交叉编译 + 运行
cross test --release --target x86_64-unknown-linux-gnu --test e2e_cli
cross test --release --target x86_64-unknown-linux-gnu --test e2e_terminal
```

### 优缺点

```
+ 开发迭代快：主机编辑 → cross test → 即时反馈
+ 测试镜像小（debian-slim ~80MB）
+ cross 处理所有交叉编译细节
+ 可以在 Windows 上开发，Linux 上测试

- 需要安装 cross 和 Docker
- 交叉编译可能遇到 C 依赖问题（本项目用 rustls 所以不会）
- e2e_terminal 测试中的 F:\temp 路径需要改为 /tmp
```

---

## 模式 C：多阶段构建（CI/CD 推荐）

### Dockerfile.ci

```dockerfile
# ============================================================
# Stage 1: 编译
# ============================================================
FROM rust:latest AS builder

WORKDIR /build

# 依赖缓存层
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release 2>/dev/null; \
    rm -rf src

# 完整编译
COPY src/ src/
COPY tests/ tests/
RUN cargo build --release && \
    cargo test --release --no-run 2>&1 | tee /build/test-binaries.txt

# ============================================================
# Stage 2: 测试运行时
# ============================================================
FROM debian:trixie-slim AS tester

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
RUN mkdir -p /tmp/test-workspace

# 只复制需要的产物
COPY --from=builder /build/target/release/claude-code-rs /usr/local/bin/
COPY --from=builder /build/target/release/deps/e2e_* /app/tests/

# 离线测试入口
CMD ["sh", "-c", "for t in /app/tests/e2e_*; do echo \"=== $t ===\"; $t --test-threads=1 2>&1 || true; done"]

# ============================================================
# Stage 3: 最小发布镜像（可选）
# ============================================================
FROM debian:trixie-slim AS release

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/claude-code-rs /usr/local/bin/
ENTRYPOINT ["claude-code-rs"]
```

### GitHub Actions 集成

```yaml
# .github/workflows/test.yml
name: E2E Tests

on: [push, pull_request]

jobs:
  test-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build test image
        run: docker build --target tester -t cc-rust-test -f Dockerfile.ci .

      - name: Run offline tests
        run: docker run --rm cc-rust-test

      - name: Run live tests
        if: github.event_name == 'push' && github.ref == 'refs/heads/rust-lite'
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          docker run --rm \
            -e ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
            cc-rust-test \
            sh -c 'for t in /app/tests/e2e_*; do $t --ignored --test-threads=1 2>&1 || true; done'

  test-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --release -- --skip ignored
```

---

## 容器内 PTY 测试

### 问题：Docker 容器中有 PTY 吗？

Docker 容器默认没有分配 TTY。但：

```bash
# 无 TTY（默认）
docker run --rm cc-rust-test python -c "import os; print(os.isatty(0))"
# → False

# 分配 TTY
docker run --rm -t cc-rust-test python -c "import os; print(os.isatty(0))"
# → True
```

### e2e_pty 测试在容器中的可行性

| 场景 | 可行 | 说明 |
|------|------|------|
| `--headless` JSONL 测试（e2e_terminal） | ✅ | 不需要 PTY，管道 I/O |
| `--init-only` / `--version`（e2e_cli） | ✅ | 不需要 PTY |
| `portable-pty` PTY 测试（e2e_pty） | ✅ | Linux 容器有 `/dev/ptmx`，`portable-pty` 用 `openpty()` |
| TUI 交互测试 | ✅ | `portable-pty` 自行创建 PTY，不依赖 `docker -t` |

**关键**：`portable-pty` 和 `pexpect` 自己调用 `openpty()` 创建 PTY 对，不需要 Docker 分配 TTY。容器中 `/dev/ptmx` 默认可用。

### 验证

```bash
# 检查容器是否支持 PTY
docker run --rm debian:bookworm-slim ls -la /dev/ptmx
# crw-rw-rw- 1 root root 5, 2 ... /dev/ptmx  ← 存在即可
```

---

## 路径适配（已完成）

测试中原有 48 处硬编码 `F:\temp`，已全部替换为 `workspace()` 函数。

### 采用方案：环境变量 + 平台默认值

```rust
// tests/e2e_terminal/helpers.rs
pub fn workspace() -> &'static str {
    static WS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    WS.get_or_init(|| {
        std::env::var("E2E_WORKSPACE")
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    r"F:\temp".to_string()
                } else {
                    "/tmp/cc-rust-test".to_string()
                }
            })
    })
}
```

```bash
# Docker 中
docker run --rm -e E2E_WORKSPACE=/tmp/test cc-rust-test cargo test
```

### 方案 2：条件编译

```rust
#[cfg(windows)]
const WORKSPACE: &str = r"F:\temp";
#[cfg(not(windows))]
const WORKSPACE: &str = "/tmp/cc-rust-test";
```

### 方案 3：tempdir（最干净）

```rust
fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("create temp workspace")
}
```

---

## Docker Compose 测试矩阵

```yaml
# docker-compose.test.yml
version: "3.8"

services:
  # 离线测试（不需要网络）
  test-offline:
    build:
      context: .
      dockerfile: Dockerfile.ci
      target: tester
    network_mode: none
    environment:
      - E2E_WORKSPACE=/tmp/test
      - ANTHROPIC_API_KEY=
    command: >
      sh -c '
        mkdir -p /tmp/test &&
        for t in /app/tests/e2e_*; do
          echo "=== $(basename $t) ===" &&
          timeout 120 $t --test-threads=1 2>&1 || true
        done
      '

  # Live API 测试
  test-live:
    build:
      context: .
      dockerfile: Dockerfile.ci
      target: tester
    environment:
      - E2E_WORKSPACE=/tmp/test
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
    command: >
      sh -c '
        mkdir -p /tmp/test &&
        for t in /app/tests/e2e_*; do
          echo "=== $(basename $t) ===" &&
          timeout 300 $t --ignored --test-threads=1 2>&1 || true
        done
      '

  # PTY 专项测试
  test-pty:
    build:
      context: .
      dockerfile: Dockerfile.ci
      target: tester
    environment:
      - E2E_WORKSPACE=/tmp/test
    command: >
      sh -c '
        mkdir -p /tmp/test &&
        /app/tests/e2e_pty* --test-threads=1 2>&1
      '
```

```bash
# 运行离线测试
docker compose -f docker-compose.test.yml run --rm test-offline

# 运行 live 测试
ANTHROPIC_API_KEY=sk-ant-... docker compose -f docker-compose.test.yml run --rm test-live

# 全部运行
docker compose -f docker-compose.test.yml up --abort-on-container-exit
```

---

## 性能优化

### 1. 依赖缓存（BuildKit cache mount）

```dockerfile
FROM rust:latest AS builder

# 利用 BuildKit 缓存 cargo registry 和编译产物
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release
```

```bash
DOCKER_BUILDKIT=1 docker build -t cc-rust-test .
```

### 2. sccache 分布式编译缓存

```dockerfile
FROM rust:latest AS builder

RUN cargo install sccache
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_DIR=/sccache

RUN --mount=type=cache,target=/sccache \
    cargo build --release
```

### 3. 增量开发（bind mount）

```bash
# 将源码 mount 进容器，利用增量编译
docker run --rm -it \
  -v "$(pwd):/app" \
  -v cc-rust-cargo-cache:/usr/local/cargo/registry \
  -v cc-rust-target-cache:/app/target \
  rust:1.82-bookworm \
  bash -c "cd /app && cargo test --test e2e_terminal"
```

---

## 安全注意事项

### API Key 管理

```bash
# 不要在 Dockerfile 中硬编码 API key

# 运行时注入
docker run --rm -e ANTHROPIC_API_KEY="$KEY" cc-rust-test

# 或使用 Docker secret（Compose/Swarm）
echo "sk-ant-..." | docker secret create anthropic_key -

# 或使用 .env 文件（不要提交到 git）
docker run --rm --env-file .env.test cc-rust-test
```

### 容器隔离

```bash
# 限制资源防止失控测试
docker run --rm \
  --memory=2g \
  --cpus=2 \
  --network=none \        # 离线测试不需要网络
  --read-only \           # 只读根文件系统
  --tmpfs /tmp:size=100m  # 工作区用 tmpfs
  cc-rust-test
```

---

## 实施完成清单

| 步骤 | 状态 | 说明 |
|------|------|------|
| 步骤 | 状态 | 说明 |
|------|------|------|
| 路径适配 | ✅ 完成 | 18 文件 48 处 → `workspace()` 函数 + `E2E_WORKSPACE` env |
| Dockerfile.ci（Rust 后端） | ✅ 完成 | 三阶段：`rust:latest` → `debian:trixie-slim` tester → release |
| ui/Dockerfile.test（UI） | ✅ 完成 | 两阶段：`rust:latest`（编译二进制）→ `oven/bun:latest`（测试） |
| .dockerignore | ✅ 完成 | 排除 target/、logs/、node_modules/ |
| docker-compose.test.yml | ✅ 完成 | test-offline（无网络）+ test-live（`env_file: .env`） |
| Rust 后端验证 | ✅ 完成 | 8/8 测试套件，118 offline tests green |
| OpenTUI UI 验证 | ✅ 完成 | 234 tests green（parser、keypress、screen、reducer 等） |
| GitHub Actions CI | 📋 待做 | `.github/workflows/test.yml` |
| cross 交叉编译 | 📋 待做 | `cross test --target x86_64-unknown-linux-gnu` |
| BuildKit 缓存 | 📋 待做 | `--mount=type=cache` 优化编译时间 |

---

## 后续工作计划

### 短期（可立即执行）

1. **GitHub Actions CI**
   - 添加 `.github/workflows/test.yml`
   - push 时自动构建两个镜像并运行测试：
     ```yaml
     - docker build --target tester -t cc-rust-test -f Dockerfile.ci .
     - docker run --rm cc-rust-test
     - docker build -t cc-rust-ui-test -f ui/Dockerfile.test .
     - docker run --rm cc-rust-ui-test
     ```
   - live 测试仅在 main/rust-lite 分支 push 时运行（使用 secrets）

2. **e2e_pty / pty_ui 路径适配**
   - 这两个文件仍有 `F:\temp` 硬编码
   - 使用相同的 `test_workspace.rs` 模式替换

3. **UI 集成测试**
   - 在 `ui/src/` 中添加 `ipc/__tests__/client.test.ts`
   - 测试 `RustBackend` spawn `--headless` → 发送 JSONL → 接收响应
   - Rust 二进制已在 UI 测试镜像中（`CC_RUST_BINARY=/usr/local/bin/claude-code-rs`）
   - 不需要 API key：测试 ready 消息、quit、错误处理

### 中期（功能增强）

4. **BuildKit 缓存加速**
   ```bash
   DOCKER_BUILDKIT=1 docker build \
     --build-arg BUILDKIT_INLINE_CACHE=1 \
     --target tester -t cc-rust-test -f Dockerfile.ci .
   ```
   在 Dockerfile.ci 中添加 `--mount=type=cache` 减少重复编译

5. **`cross` 集成**
   ```bash
   cargo install cross
   cross test --release --target x86_64-unknown-linux-gnu --test e2e_terminal
   ```
   开发循环更快：无需手动管理 Docker

6. **镜像体积优化**
   - release stage 当前 ~150MB
   - 可用 `musl` target 静态编译，release 镜像降到 ~30MB
   - `FROM scratch` 或 `FROM alpine:3.20` 做最终镜像

7. **UI 渲染测试（node-pty + xterm-headless）**
   - 在 `ui/Dockerfile.test` 中添加 node-pty
   - 用 `xterm-headless` 做渲染级验证（像素级准确）
   - 测试交替屏幕、resize、颜色等 TUI 行为
   - 参见 `docs/pty-test-language-comparison.md`

### 长期（架构改进）

8. **多平台 CI 矩阵**
   ```yaml
   strategy:
     matrix:
       os: [ubuntu-latest, windows-latest, macos-latest]
       rust: [stable]
   ```
   Windows: `cargo test`，Linux: Docker，macOS: `cargo test`

9. **测试覆盖率报告**
    - `cargo tarpaulin --out Html` 生成覆盖率
    - Docker 中运行 tarpaulin，产出 artifact
    - CI 中自动发布到 PR comment

10. **统一测试入口**
    - 添加 `Makefile` 或 `just` 配置
    - `make test-all` = Rust 后端 + UI 单元 + UI 集成
    - `make test-docker` = 两个 Docker 镜像构建 + 运行
    - `make test-live` = API 测试（需要 `.env`）

---

## 关键结论

| 问题 | 答案 |
|------|------|
| Linux 容器能编译 cc-rust 吗？ | **能**。rustls-tls 无系统依赖，keyring 自动降级 |
| 容器里能跑 PTY 测试吗？ | **能**。`/dev/ptmx` 默认存在，`portable-pty` 自行创建 PTY |
| OpenTUI UI 能在 Docker 中测试吗？ | **能**。`oven/bun:latest` + Rust 二进制，234 测试已验证 |
| 需要 Windows 容器吗？ | **不需要**。Linux 容器覆盖核心逻辑，Windows 测试在本机跑 |
| 需要 rust:latest 而非固定版本？ | **是**。依赖需 1.88+（globset edition2024, time-core） |
| tester 必须用 trixie 不能用 bookworm？ | **是**。`rust:latest` 编译产物依赖 glibc 2.39+ |
| 如何传 API key 给 Docker？ | `--env-file .env` 或 compose 的 `env_file: .env` |
| `CARGO_BIN_EXE_*` 含连字符怎么办？ | 用 `env "NAME=VALUE" cmd` 而非 `export` |
