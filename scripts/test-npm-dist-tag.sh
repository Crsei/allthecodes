#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
resolver="${script_dir}/npm-dist-tag.sh"

[[ "$("${resolver}" 1.2.3)" == "latest" ]]
[[ "$("${resolver}" 1.2.3-alpha.1)" == "alpha" ]]
[[ "$("${resolver}" 1.2.3-beta.2)" == "beta" ]]
[[ "$("${resolver}" 1.2.3-rc.4)" == "rc" ]]
if "${resolver}" 1.2.3-preview.1 >/dev/null 2>&1; then
  printf 'FAIL: unsupported prerelease unexpectedly succeeded\n' >&2
  exit 1
fi

printf 'PASS: npm dist-tag resolver\n'
