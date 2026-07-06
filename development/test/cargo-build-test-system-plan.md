# Cargo Build/Test 体系 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立一套可本地复现、可在 CI 对齐、可逐步扩展到 nextest 的 Cargo build/test 入口。

**Architecture:** 保留当前 `scripts/cargo-build-test.sh` 作为 Bash 级核心执行器，新增 `justfile` 作为开发者入口，并用轻量 shell 自测锁定 dry-run 命令编排。nextest、rustfmt、clippy、CI 调整分阶段接入，避免一次性引入 Bazel 或大规模格式化 churn。

**Tech Stack:** Bash, Cargo, rustfmt, clippy, optional cargo-nextest, just, GitHub Actions.

## Global Constraints

- 当前仓库是 allthecodes full build 阶段，不再按 lite 省略验证覆盖。
- 本地 Rust 构建必须优先使用 `CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo`。
- 本地 Rustup 必须优先使用 `RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup`。
- 本地构建产物必须放到 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target`，不要落入仓库内 `target/`。
- 当前 workspace crate 使用 Rust 2021 edition；不要照搬 Codex 原版的 Rust 2024 rustfmt 配置。
- 不引入 Bazel。Codex 原版的 Bazel CI 体系不是本阶段目标。
- 不改 npm release 的 web-ui 策略；此计划只覆盖 Rust Cargo build/test 体系。
- 每个任务只暂存本任务明确涉及的路径。

---

## File Structure

- Modify: `scripts/cargo-build-test.sh`
  - 继续作为唯一 Bash 执行器。
  - 增加 CI-safe 环境探测、单项模式、可选 nextest 模式。
- Create: `scripts/test-cargo-build-test.sh`
  - Shell 自测，不运行重型 Cargo 编译。
  - 验证 `--dry-run` 输出、mode 解析、错误信息。
- Create: `justfile`
  - 开发者入口，委托给 `scripts/cargo-build-test.sh`。
- Create: `.config/nextest.toml`
  - 本地 nextest profile、slow-timeout、已知重型测试组。
- Modify: `clippy.toml`
  - 补充 async lock guard 与 ratatui 样式限制。
- Create: `rustfmt.toml`
  - 固定 `edition = "2021"`；不设置 import churn 规则。
- Modify: `.github/workflows/ci.yml`
  - 增加 `CARGO_NET_GIT_FETCH_WITH_CLI=true`。
  - 保持当前 Cargo test 作为 CI 基线，nextest 先不替换。
- Modify: `development/test/README.md`
  - 加入本计划索引和运行入口。

---

## Task 1: 脚本自测与 CI-safe 环境探测

**Files:**
- Create: `scripts/test-cargo-build-test.sh`
- Modify: `scripts/cargo-build-test.sh`

**Interfaces:**
- Consumes: existing `scripts/cargo-build-test.sh --dry-run <mode>`
- Produces: stable self-test command `scripts/test-cargo-build-test.sh`

- [ ] **Step 1: Write the failing self-test**

Create `scripts/test-cargo-build-test.sh`:

```bash
#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "${script_dir}/.." && pwd -P)"
script="${repo_root}/scripts/cargo-build-test.sh"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  [[ "${haystack}" == *"${needle}"* ]] || fail "expected output to contain: ${needle}"
}

assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  [[ "${haystack}" != *"${needle}"* ]] || fail "expected output to omit: ${needle}"
}

full_output="$("${script}" --dry-run full)"
assert_contains "${full_output}" "+ cargo fmt --all --check"
assert_contains "${full_output}" "+ cargo clippy --workspace --all-targets -- -D warnings"
assert_contains "${full_output}" "+ cargo test --workspace"
assert_contains "${full_output}" "+ cargo build --workspace --release"

ci_no_toolchain="$("${script}" --dry-run --no-toolchain ci)"
assert_not_contains "${ci_no_toolchain}" "+ cargo --version"
assert_contains "${ci_no_toolchain}" "+ cargo test -p allthecodes-tools --features full"

if "${script}" --dry-run unknown-mode >/tmp/allthecodes-script-test.out 2>&1; then
  fail "unknown mode unexpectedly succeeded"
fi
assert_contains "$(cat /tmp/allthecodes-script-test.out)" "unknown argument: unknown-mode"

printf 'PASS: cargo-build-test script self-test\n'
```

- [ ] **Step 2: Run self-test to verify the new nextest/CI-safe expectations fail**

Run:

```bash
scripts/test-cargo-build-test.sh
```

Expected: FAIL once later checks for new modes are added. At this first checkpoint, the script should pass for the current baseline before modifying behavior.

- [ ] **Step 3: Add CI-safe local env defaults**

Modify `scripts/cargo-build-test.sh` environment setup to avoid hiding CI-installed Rustup when sibling `.rust/` does not exist:

```bash
DEFAULT_CARGO_HOME="${WORKSPACE_PARENT}/.rust/cargo"
DEFAULT_RUSTUP_HOME="${WORKSPACE_PARENT}/.rust/rustup"
DEFAULT_CARGO_TARGET_DIR="${WORKSPACE_PARENT}/.tmp/allthecodes-target"

if [[ -z "${CARGO_HOME:-}" && -d "${DEFAULT_CARGO_HOME}" ]]; then
  export CARGO_HOME="${DEFAULT_CARGO_HOME}"
fi

if [[ -z "${RUSTUP_HOME:-}" && -d "${DEFAULT_RUSTUP_HOME}" ]]; then
  export RUSTUP_HOME="${DEFAULT_RUSTUP_HOME}"
fi

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="${DEFAULT_CARGO_TARGET_DIR}"
fi

if [[ -n "${CARGO_HOME:-}" ]]; then
  case ":${PATH}:" in
    *":${CARGO_HOME}/bin:"*) ;;
    *) export PATH="${CARGO_HOME}/bin:${PATH}" ;;
  esac
fi
```

- [ ] **Step 4: Re-run self-test**

Run:

```bash
scripts/test-cargo-build-test.sh
```

Expected: PASS.

- [ ] **Step 5: Syntax check**

Run:

```bash
bash -n scripts/cargo-build-test.sh
bash -n scripts/test-cargo-build-test.sh
```

Expected: both commands exit 0.

- [ ] **Step 6: Commit**

```bash
git add -A -- scripts/cargo-build-test.sh scripts/test-cargo-build-test.sh
git commit -m "test: add cargo build script self-test"
```

---

## Task 2: justfile 开发者入口

**Files:**
- Create: `justfile`
- Modify: `development/test/README.md`

**Interfaces:**
- Consumes: `scripts/cargo-build-test.sh`
- Produces: `just quick`, `just ci`, `just full`, `just fmt`, `just test`

- [ ] **Step 1: Write the failing command check**

Run before creating `justfile`:

```bash
just --list
```

Expected: FAIL with no justfile found, or command not found if `just` is not installed. If `just` is not installed, install it outside this task before continuing:

```bash
cargo install --locked just
```

- [ ] **Step 2: Create `justfile`**

Create repository-root `justfile`:

```make
set positional-arguments

script := "scripts/cargo-build-test.sh"

help:
    just --list

toolchain:
    {{script}} toolchain

quick:
    {{script}} quick

fmt:
    cargo fmt --all

fmt-check:
    {{script}} --no-toolchain --dry-run ci
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test *args:
    cargo test --workspace {{args}}

ci:
    {{script}} ci

full:
    {{script}} full

dry-run mode="full":
    {{script}} --dry-run {{mode}}
```

- [ ] **Step 3: Verify just command discovery**

Run:

```bash
just --list
```

Expected: output includes `quick`, `ci`, `full`, `dry-run`.

- [ ] **Step 4: Verify dry-run delegation**

Run:

```bash
just dry-run full
```

Expected: output includes:

```text
+ cargo fmt --all --check
+ cargo clippy --workspace --all-targets -- -D warnings
+ cargo build --workspace --release
```

- [ ] **Step 5: Update test README run section**

Modify `development/test/README.md` under `## 运行` to include:

```markdown
```bash
# Cargo build/test wrapper
scripts/cargo-build-test.sh --dry-run full
scripts/cargo-build-test.sh ci
scripts/cargo-build-test.sh full

# Optional just facade
just dry-run full
just ci
just full
```
```

- [ ] **Step 6: Commit**

```bash
git add -A -- justfile development/test/README.md
git commit -m "chore: add cargo verification just facade"
```

---

## Task 3: Optional nextest profile and mode

**Files:**
- Create: `.config/nextest.toml`
- Modify: `scripts/cargo-build-test.sh`
- Modify: `scripts/test-cargo-build-test.sh`
- Modify: `justfile`

**Interfaces:**
- Consumes: Cargo workspace tests
- Produces: `scripts/cargo-build-test.sh nextest` and `just nextest`

- [ ] **Step 1: Extend self-test with failing nextest expectation**

Append to `scripts/test-cargo-build-test.sh`:

```bash
nextest_output="$("${script}" --dry-run nextest)"
assert_contains "${nextest_output}" "+ cargo nextest run --workspace --no-fail-fast"
```

- [ ] **Step 2: Run self-test and confirm failure**

Run:

```bash
scripts/test-cargo-build-test.sh
```

Expected: FAIL with `unknown argument: nextest`.

- [ ] **Step 3: Add nextest config**

Create `.config/nextest.toml`:

```toml
[profile.default]
slow-timeout = { period = "30s", terminate-after = 2 }

[profile.default.junit]
path = "junit.xml"

[profile.local]
inherits = "default"

[test-groups.tui_pty_e2e]
max-threads = 1

[test-groups.integration_process_heavy]
max-threads = 2

[[profile.default.overrides]]
filter = 'package(allthecodes) & test(pty_tui_e2e)'
test-group = 'tui_pty_e2e'
slow-timeout = { period = "45s", terminate-after = 2 }

[[profile.default.overrides]]
filter = 'test(headless) | test(ipc) | test(daemon)'
test-group = 'integration_process_heavy'
slow-timeout = { period = "45s", terminate-after = 2 }
```

- [ ] **Step 4: Add nextest mode**

Modify `scripts/cargo-build-test.sh`:

```bash
# usage modes
#   nextest    Run workspace tests with cargo-nextest

# argument case
quick|ci|release|full|toolchain|nextest)
  MODE="$1"
  shift
  ;;

run_nextest() {
  if [[ "${DRY_RUN}" == "0" ]]; then
    cargo nextest --version >/dev/null 2>&1 || die "cargo-nextest is required for nextest mode. Install with: cargo install --locked cargo-nextest"
  fi
  run cargo nextest run --workspace --no-fail-fast
}

# mode case
nextest)
  run_nextest
  ;;
```

- [ ] **Step 5: Add just nextest target**

Modify `justfile`:

```make
nextest:
    {{script}} nextest
```

- [ ] **Step 6: Verify dry-run and self-test**

Run:

```bash
scripts/cargo-build-test.sh --dry-run nextest
scripts/test-cargo-build-test.sh
```

Expected: both pass; dry-run prints `cargo nextest run --workspace --no-fail-fast`.

- [ ] **Step 7: Do not make CI depend on nextest yet**

No CI workflow changes in this task. CI continues to run `cargo test --workspace`. This keeps nextest optional until local runs prove stable.

- [ ] **Step 8: Commit**

```bash
git add -A -- .config/nextest.toml scripts/cargo-build-test.sh scripts/test-cargo-build-test.sh justfile
git commit -m "test: add optional nextest verification mode"
```

---

## Task 4: Rustfmt and Clippy policy hardening

**Files:**
- Create: `rustfmt.toml`
- Modify: `clippy.toml`

**Interfaces:**
- Consumes: current workspace lints from `Cargo.toml`
- Produces: stable formatting edition and stricter TUI/async lint config

- [ ] **Step 1: Create rustfmt config**

Create `rustfmt.toml`:

```toml
edition = "2021"
```

Do not add `imports_granularity = "Item"` in this task. That setting can reflow many imports and should be a separate style migration if needed.

- [ ] **Step 2: Extend clippy config**

Modify `clippy.toml`:

```toml
# Allow unwrap/expect/panic in test code -- tests should fail-fast on error.
# See also: [workspace.lints] in root Cargo.toml
allow-expect-in-tests = true
allow-unwrap-in-tests = true
allow-panic-in-tests = true

await-holding-invalid-types = [
    "tokio::sync::MutexGuard",
    "tokio::sync::RwLockReadGuard",
    "tokio::sync::RwLockWriteGuard",
]

disallowed-methods = [
    { path = "ratatui::style::Color::Rgb", reason = "Use ANSI/default terminal colors unless a hardcoded color is explicitly required." },
    { path = "ratatui::style::Color::Indexed", reason = "Use ANSI/default terminal colors unless a hardcoded color is explicitly required." },
    { path = "ratatui::style::Stylize::white", reason = "Avoid hardcoded white; prefer default foreground or dim/bold styling." },
    { path = "ratatui::style::Stylize::black", reason = "Avoid hardcoded black; prefer default foreground or dim/bold styling." },
]

large-error-threshold = 256
```

- [ ] **Step 3: Verify formatting config**

Run:

```bash
cargo fmt --all --check
```

Expected: exits 0, or prints only existing formatting drift. If it reports drift, run:

```bash
cargo fmt --all
```

Then inspect `git diff` and keep only intended formatting changes if they are small. If formatting churn is broad, revert formatting output and split it into a separate task.

- [ ] **Step 4: Verify clippy config**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: exits 0. If new ratatui/async violations appear, fix them in the same task only when fixes are local and obvious; otherwise document the violating paths and split a follow-up cleanup plan.

- [ ] **Step 5: Commit**

```bash
git add -A -- rustfmt.toml clippy.toml
git commit -m "chore: harden rustfmt and clippy policy"
```

---

## Task 5: CI hardening without changing test runner

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `scripts/cargo-build-test.sh`

**Interfaces:**
- Consumes: existing GitHub Actions CI job
- Produces: CI env hardening and script parity check

- [ ] **Step 1: Add Git fetch hardening**

Modify `.github/workflows/ci.yml` top-level env:

```yaml
env:
  CARGO_TERM_COLOR: always
  CARGO_NET_GIT_FETCH_WITH_CLI: "true"
```

- [ ] **Step 2: Add script dry-run parity check before heavy Cargo commands**

Add a CI step after Linux build dependency installation:

```yaml
      - name: Check local Cargo verification script
        run: |
          bash -n scripts/cargo-build-test.sh
          scripts/cargo-build-test.sh --dry-run --no-toolchain ci
```

- [ ] **Step 3: Keep existing CI commands unchanged**

Leave these existing steps in place:

```yaml
      - name: Check formatting
        run: cargo fmt --all --check

      - name: Run Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Run workspace tests
        run: cargo test --workspace

      - name: Run tools contract tests
        run: cargo test -p allthecodes-tools --no-default-features --features contract

      - name: Run tools full feature tests
        run: cargo test -p allthecodes-tools --features full
```

- [ ] **Step 4: Verify workflow syntax locally by grep-level check**

Run:

```bash
rg -n 'CARGO_NET_GIT_FETCH_WITH_CLI|Check local Cargo verification script|cargo test --workspace' .github/workflows/ci.yml
```

Expected: all three strings are present.

- [ ] **Step 5: Verify script dry-run**

Run:

```bash
scripts/cargo-build-test.sh --dry-run --no-toolchain ci
```

Expected: output contains the same Cargo command sequence as `.github/workflows/ci.yml`.

- [ ] **Step 6: Commit**

```bash
git add -A -- .github/workflows/ci.yml scripts/cargo-build-test.sh
git commit -m "ci: harden cargo verification workflow"
```

---

## Task 6: Full local verification and documentation update

**Files:**
- Modify: `development/test/README.md`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: all previous tasks
- Produces: documented local build/test workflow

- [ ] **Step 1: Update `development/test/README.md` index**

Add this row to the plan table:

```markdown
| [cargo-build-test-system-plan.md](cargo-build-test-system-plan.md) | Codex 原版 build/test 体系对照 | 本计划 | ⭐ P0 |
```

- [ ] **Step 2: Update `CLAUDE.md` Cargo / Build section**

Add after the existing daily/full verification commands:

```markdown
Unified local verification entry:

```bash
scripts/cargo-build-test.sh quick
scripts/cargo-build-test.sh ci
scripts/cargo-build-test.sh full
```

If `just` is installed, the same commands are available as:

```bash
just quick
just ci
just full
```
```

- [ ] **Step 3: Run lightweight verification**

Run:

```bash
bash -n scripts/cargo-build-test.sh
scripts/test-cargo-build-test.sh
scripts/cargo-build-test.sh --dry-run full
```

Expected: all commands exit 0.

- [ ] **Step 4: Run full verification**

Run:

```bash
scripts/cargo-build-test.sh full
```

Expected: exits 0 with no warnings promoted to errors. If failures come from unrelated existing worktree changes, capture exact failing crate/test and do not mark the plan complete.

- [ ] **Step 5: Commit**

```bash
git add -A -- development/test/README.md CLAUDE.md
git commit -m "docs: document cargo verification workflow"
```

---

## Acceptance Checklist

- [ ] `scripts/test-cargo-build-test.sh` exists and passes.
- [ ] `scripts/cargo-build-test.sh --dry-run full` prints toolchain, fmt, clippy, workspace tests, tools feature tests, and release build.
- [ ] `just dry-run full` delegates to the same script.
- [ ] `.config/nextest.toml` exists, but GitHub CI still uses `cargo test --workspace`.
- [ ] `clippy.toml` includes async lock guard and ratatui style restrictions.
- [ ] `rustfmt.toml` pins `edition = "2021"` and does not introduce import churn.
- [ ] `.github/workflows/ci.yml` includes `CARGO_NET_GIT_FETCH_WITH_CLI: "true"`.
- [ ] Full local verification command is documented in `CLAUDE.md`.

## Non-Goals

- No Bazel adoption.
- No Rust 2024 edition migration.
- No npm release packaging changes.
- No `--all-features` workspace test expansion unless a task explicitly requires it.
- No broad formatting-only churn bundled with functional build/test changes.

## Execution Notes

- Prefer one commit per task.
- Run `git status --short` before every commit and stage only paths listed in that task.
- If `cargo nextest` is not installed, keep nextest task optional until `cargo install --locked cargo-nextest` is approved and completed.
- If `just` is not installed, the repository remains usable through `scripts/cargo-build-test.sh`.
