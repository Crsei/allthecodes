#!/usr/bin/env bash
set -Eeuo pipefail

version="${1:-}"
if [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf '%s\n' latest
elif [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+-(alpha|beta|rc)\.[0-9]+$ ]]; then
  printf '%s\n' "${BASH_REMATCH[1]}"
else
  printf 'error: unsupported npm release version: %s\n' "${version}" >&2
  exit 1
fi
