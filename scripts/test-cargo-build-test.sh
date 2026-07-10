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
assert_contains "${full_output}" "+ cargo clippy --locked --workspace --all-targets -- -D warnings"
assert_contains "${full_output}" "+ cargo test --locked --workspace"
assert_contains "${full_output}" "+ cargo build --locked --workspace --release"

ci_no_toolchain="$("${script}" --dry-run --no-toolchain ci)"
assert_not_contains "${ci_no_toolchain}" "+ cargo --version"
assert_contains "${ci_no_toolchain}" "+ cargo test --locked -p allthecodes-tools --features full"

nextest_output="$("${script}" --dry-run nextest)"
assert_contains "${nextest_output}" "+ cargo nextest run --locked --workspace --no-fail-fast"

unknown_output="$(mktemp "${TMPDIR:-/tmp}/allthecodes-script-test.XXXXXX")"
trap 'rm -f "${unknown_output}"' EXIT
if "${script}" --dry-run unknown-mode >"${unknown_output}" 2>&1; then
  fail "unknown mode unexpectedly succeeded"
fi
assert_contains "$(cat "${unknown_output}")" "unknown argument: unknown-mode"

printf 'PASS: cargo-build-test script self-test\n'
