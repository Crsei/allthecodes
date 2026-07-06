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
