#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/cargo-build-test.sh [OPTIONS] [MODE]

Modes:
  quick      Show toolchain, cargo check --workspace, build allthecodes release binary
  ci         Mirror the Rust CI job: fmt, clippy, workspace tests, tools feature tests
  nextest    Run workspace tests with cargo-nextest
  release    Build the full workspace in release mode
  full       Run ci mode, then release mode
  toolchain  Only show the selected Cargo/Rust toolchain

Options:
  -n, --dry-run       Print commands without running them
      --no-toolchain  Skip the initial cargo/rustc/rustup version commands
  -h, --help          Show this help

Environment defaults:
  CARGO_HOME       <workspace-parent>/.rust/cargo
  RUSTUP_HOME      <workspace-parent>/.rust/rustup
  CARGO_TARGET_DIR <workspace-parent>/.tmp/allthecodes-target

These defaults match AGENTS.md and keep build artifacts out of the repository.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

info() {
  printf '==> %s\n' "$*"
}

print_command() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
}

run() {
  print_command "$@"
  if [[ "${DRY_RUN}" == "0" ]]; then
    "$@"
  fi
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
WORKSPACE_PARENT="$(cd -- "${REPO_ROOT}/.." && pwd -P)"

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

export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"

if [[ -n "${CARGO_HOME:-}" ]]; then
  case ":${PATH}:" in
    *":${CARGO_HOME}/bin:"*) ;;
    *) export PATH="${CARGO_HOME}/bin:${PATH}" ;;
  esac
fi

MODE="ci"
DRY_RUN="0"
SHOW_TOOLCHAIN="1"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -n|--dry-run)
      DRY_RUN="1"
      shift
      ;;
    --no-toolchain)
      SHOW_TOOLCHAIN="0"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    quick|ci|nextest|release|full|toolchain)
      MODE="$1"
      shift
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

cd "${REPO_ROOT}"

if [[ "${DRY_RUN}" == "0" ]]; then
  mkdir -p "${CARGO_TARGET_DIR}"
  require_command cargo
  require_command rustc
  require_command rustup
fi

info "repo: ${REPO_ROOT}"
info "mode: ${MODE}"
info "target dir: ${CARGO_TARGET_DIR}"

show_toolchain() {
  run cargo --version
  run rustc --version
  run rustup show active-toolchain
}

run_quick() {
  run cargo check --locked --workspace
  run cargo build --locked -p allthecodes --release
}

run_ci() {
  run cargo fmt --all --check
  run cargo clippy --locked --workspace --all-targets -- -D warnings
  run cargo test --locked --workspace
  run cargo test --locked -p allthecodes-tools --no-default-features --features contract
  run cargo test --locked -p allthecodes-tools --features full
}

run_nextest() {
  if [[ "${DRY_RUN}" == "0" ]]; then
    cargo nextest --version >/dev/null 2>&1 || die "cargo-nextest is required for nextest mode. Install with: cargo install --locked cargo-nextest"
  fi
  run cargo nextest run --locked --workspace --no-fail-fast
}

run_release() {
  run cargo build --locked --workspace --release
}

if [[ "${SHOW_TOOLCHAIN}" == "1" || "${MODE}" == "toolchain" ]]; then
  show_toolchain
fi

case "${MODE}" in
  quick)
    run_quick
    ;;
  ci)
    run_ci
    ;;
  nextest)
    run_nextest
    ;;
  release)
    run_release
    ;;
  full)
    run_ci
    run_release
    ;;
  toolchain)
    ;;
esac
