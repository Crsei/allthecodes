#!/usr/bin/env python3
"""Validate and run cargo-native shards for the PTY TUI integration suite."""

from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
PACKAGE = "allthecodes"
PTY_TARGET = "pty_tui_e2e"

EXPECTED_TOTAL = 248
EXPECTED_IGNORED = 34
EXPECTED_DEFAULT = 214

# Prefixes are complete Rust module paths. A test matches only when its name
# starts with ``<prefix>::``; substring matching is rejected by validation.
# commands_mcp_plugin deliberately owns a dedicated shard so hangs or cleanup
# regressions in MCP/browser subprocess tests remain immediately identifiable.
SHARDS: dict[str, tuple[str, ...]] = {
    "commands-mcp-plugin": (
        "tests::commands_mcp_plugin",
    ),
    "command-surfaces": (
        "commands",
        "harness::tests",
        "screenshot",
        "status",
        "welcome",
        "tests::commands_surface",
        "tests::running_task_slash_commands",
    ),
    "command-flows": (
        "tests::commands_agent_team",
        "tests::commands_aliases",
        "tests::commands_auth",
        "tests::commands_core_info",
        "tests::commands_git",
        "tests::commands_kairos",
    ),
    "core-flows": (
        "conversation",
        "model_flow",
        "permissions",
        "script::tests",
        "tests::commands_memory_skills_hooks",
        "tests::commands_permissions",
        "tests::commands_query",
        "tests::commands_session",
        "tests::test1_login_structure",
        "tests::test2_full_access",
        "tests::test3_plan_flow",
        "tests::test4_task_execution",
        "tests::test5_compact",
    ),
}


class ShardContractError(RuntimeError):
    """Raised when discovery no longer matches the fail-closed shard contract."""


class Coverage:
    """Validated PTY shard assignment and ignored/default partitions."""

    def __init__(
        self,
        *,
        all_tests: set[str],
        ignored_tests: set[str],
        assignments: dict[str, set[str]],
    ) -> None:
        self.all_tests = all_tests
        self.ignored_tests = ignored_tests
        self.default_tests = all_tests - ignored_tests
        self.assignments = assignments
        self.default_assignments = {
            shard: tests - ignored_tests for shard, tests in assignments.items()
        }


def parse_test_list(output: str) -> set[str]:
    """Parse libtest's terse ``--list`` output into test names."""

    return {
        line.removesuffix(": test")
        for raw_line in output.splitlines()
        if (line := raw_line.strip()).endswith(": test")
    }


def _prefix_matches(test_name: str, prefix: str) -> bool:
    return test_name.startswith(f"{prefix}::")


def validate_coverage(
    all_tests: set[str],
    ignored_tests: set[str],
    *,
    shards: Mapping[str, Sequence[str]] = SHARDS,
    expected_total: int = EXPECTED_TOTAL,
    expected_ignored: int = EXPECTED_IGNORED,
    expected_default: int = EXPECTED_DEFAULT,
) -> Coverage:
    """Require exact counts and a complete, disjoint prefix assignment."""

    if len(all_tests) != expected_total:
        raise ShardContractError(
            f"expected {expected_total} total PTY tests, discovered {len(all_tests)}"
        )
    if not ignored_tests <= all_tests:
        unknown = sorted(ignored_tests - all_tests)
        raise ShardContractError(f"ignored tests absent from total discovery: {unknown}")
    if len(ignored_tests) != expected_ignored:
        raise ShardContractError(
            f"expected {expected_ignored} ignored PTY tests, discovered {len(ignored_tests)}"
        )

    default_tests = all_tests - ignored_tests
    if len(default_tests) != expected_default:
        raise ShardContractError(
            f"expected {expected_default} default PTY tests, discovered {len(default_tests)}"
        )

    assignments = {shard: set() for shard in shards}
    used_prefixes: set[tuple[str, str]] = set()
    unassigned: list[str] = []
    multiply_assigned: dict[str, list[str]] = {}

    for test_name in sorted(all_tests):
        matches = [
            (shard, prefix)
            for shard, prefixes in shards.items()
            for prefix in prefixes
            if _prefix_matches(test_name, prefix)
        ]
        if not matches:
            unassigned.append(test_name)
            continue
        if len(matches) > 1:
            multiply_assigned[test_name] = [
                f"{shard}:{prefix}" for shard, prefix in matches
            ]
            continue

        shard, prefix = matches[0]
        assignments[shard].add(test_name)
        used_prefixes.add((shard, prefix))

    if unassigned:
        raise ShardContractError(f"unassigned PTY tests: {unassigned}")
    if multiply_assigned:
        raise ShardContractError(
            f"PTY tests matched multiple shard prefixes: {multiply_assigned}"
        )

    configured_prefixes = {
        (shard, prefix) for shard, prefixes in shards.items() for prefix in prefixes
    }
    unused_prefixes = sorted(configured_prefixes - used_prefixes)
    if unused_prefixes:
        raise ShardContractError(f"shard prefixes matched no PTY tests: {unused_prefixes}")

    union = set().union(*assignments.values())
    if union != all_tests:
        raise ShardContractError("PTY shard union is incomplete")
    shard_names = list(assignments)
    for index, left in enumerate(shard_names):
        for right in shard_names[index + 1 :]:
            overlap = assignments[left] & assignments[right]
            if overlap:
                raise ShardContractError(
                    f"PTY shard intersection is not empty for {left}/{right}: {sorted(overlap)}"
                )

    return Coverage(
        all_tests=set(all_tests),
        ignored_tests=set(ignored_tests),
        assignments=assignments,
    )


def _list_command(*, ignored_only: bool) -> list[str]:
    command = [
        "cargo",
        "test",
        "--locked",
        "-p",
        PACKAGE,
        "--test",
        PTY_TARGET,
        "--",
        "--list",
        "--format",
        "terse",
    ]
    if ignored_only:
        command.append("--ignored")
    return command


def discover_test_list(*, ignored_only: bool) -> set[str]:
    command = _list_command(ignored_only=ignored_only)
    print(f"+ {shlex.join(command)}", flush=True)
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return parse_test_list(result.stdout)


def discover_coverage() -> Coverage:
    all_tests = discover_test_list(ignored_only=False)
    ignored_tests = discover_test_list(ignored_only=True)
    return validate_coverage(all_tests, ignored_tests)


def print_coverage(coverage: Coverage) -> None:
    print(
        "PTY coverage: "
        f"total={len(coverage.all_tests)} "
        f"ignored={len(coverage.ignored_tests)} "
        f"default={len(coverage.default_tests)}"
    )
    for shard in SHARDS:
        all_count = len(coverage.assignments[shard])
        default_count = len(coverage.default_assignments[shard])
        ignored_count = all_count - default_count
        print(
            f"  {shard}: total={all_count} "
            f"ignored={ignored_count} default={default_count}"
        )
    print("PTY shard union complete; pairwise intersections empty")


def pty_test_command(prefix: str) -> list[str]:
    """Build one cargo-native module-filter command for a shard prefix."""

    return [
        "cargo",
        "test",
        "--locked",
        "-p",
        PACKAGE,
        "--test",
        PTY_TARGET,
        f"{prefix}::",
        "--",
        "--test-threads=1",
    ]


def _exact_pty_test_command(test_name: str) -> list[str]:
    return [
        "cargo",
        "test",
        "--locked",
        "-p",
        PACKAGE,
        "--test",
        PTY_TARGET,
        test_name,
        "--",
        "--exact",
        "--test-threads=1",
    ]


def pty_test_commands(
    prefix: str,
    *,
    runnable_tests: set[str],
    all_tests: set[str],
    ignored_tests: set[str],
) -> list[list[str]]:
    """Use one module filter when safe, otherwise exact-test commands."""

    filter_text = f"{prefix}::"
    cargo_runnable = {
        test_name
        for test_name in all_tests - ignored_tests
        if filter_text in test_name
    }
    if cargo_runnable == runnable_tests:
        return [pty_test_command(prefix)] if runnable_tests else []
    return [_exact_pty_test_command(test_name) for test_name in sorted(runnable_tests)]


def require_isolated_environment(environment: Mapping[str, str]) -> None:
    workspace = environment.get("E2E_WORKSPACE", "").strip()
    home = environment.get("ALLTHECODES_HOME", "").strip()
    if not workspace:
        raise ShardContractError("E2E_WORKSPACE must be set for a PTY shard")
    if not home:
        raise ShardContractError("ALLTHECODES_HOME must be set for a PTY shard")
    if Path(workspace).resolve() == Path(home).resolve():
        raise ShardContractError("E2E_WORKSPACE and ALLTHECODES_HOME must differ")


def run_shard(shard: str) -> None:
    require_isolated_environment(os.environ)
    coverage = discover_coverage()
    print_coverage(coverage)

    for prefix in SHARDS[shard]:
        runnable_tests = {
            test_name
            for test_name in coverage.default_assignments[shard]
            if _prefix_matches(test_name, prefix)
        }
        commands = pty_test_commands(
            prefix,
            runnable_tests=runnable_tests,
            all_tests=coverage.all_tests,
            ignored_tests=coverage.ignored_tests,
        )
        print(f"::group::PTY prefix {prefix} ({len(runnable_tests)} default tests)")
        try:
            for command in commands:
                print(f"+ {shlex.join(command)}", flush=True)
                subprocess.run(command, cwd=ROOT, check=True)
        finally:
            print("::endgroup::")


_LIB_KINDS = {"lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"}
_DIRECT_TARGET_KINDS = {"bin", "test", "example", "bench"}


def _allthecodes_package(metadata: Mapping[str, Any]) -> Mapping[str, Any]:
    packages = [
        package for package in metadata.get("packages", []) if package.get("name") == PACKAGE
    ]
    if len(packages) != 1:
        raise ShardContractError(
            f"expected exactly one {PACKAGE} package in cargo metadata, found {len(packages)}"
        )
    return packages[0]


def non_pty_commands(metadata: Mapping[str, Any]) -> list[list[str]]:
    """Build commands covering all workspace tests except the PTY binary."""

    package = _allthecodes_package(metadata)
    targets = package.get("targets", [])
    pty_targets = [
        target
        for target in targets
        if target.get("name") == PTY_TARGET and "test" in target.get("kind", [])
    ]
    if len(pty_targets) != 1:
        raise ShardContractError(
            f"expected exactly one {PTY_TARGET} integration target, found {len(pty_targets)}"
        )

    commands = [
        ["cargo", "test", "--locked", "--workspace", "--exclude", PACKAGE]
    ]
    package_command = ["cargo", "test", "--locked", "-p", PACKAGE]
    has_library = False
    selected_targets = 0

    for target in sorted(targets, key=lambda item: (item.get("name", ""), item.get("kind", []))):
        name = target.get("name")
        kinds = set(target.get("kind", []))
        if "custom-build" in kinds:
            continue
        if name == PTY_TARGET and "test" in kinds:
            continue
        if kinds & _LIB_KINDS:
            if not has_library:
                package_command.append("--lib")
                selected_targets += 1
                has_library = True
            continue

        direct_kinds = kinds & _DIRECT_TARGET_KINDS
        if len(direct_kinds) != 1 or not isinstance(name, str) or not name:
            raise ShardContractError(
                f"unsupported {PACKAGE} cargo target {name!r} with kinds {sorted(kinds)}"
            )
        kind = direct_kinds.pop()
        package_command.extend([f"--{kind}", name])
        selected_targets += 1

    if selected_targets == 0:
        raise ShardContractError(f"no non-PTY {PACKAGE} cargo targets discovered")
    commands.append(package_command)
    if has_library:
        commands.append(["cargo", "test", "--locked", "-p", PACKAGE, "--doc"])
    return commands


def discover_metadata() -> Mapping[str, Any]:
    command = ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]
    print(f"+ {shlex.join(command)}", flush=True)
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return json.loads(result.stdout)


def run_non_pty() -> None:
    commands = non_pty_commands(discover_metadata())
    for command in commands:
        print(f"+ {shlex.join(command)}", flush=True)
        subprocess.run(command, cwd=ROOT, check=True)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("verify", help="discover and validate PTY shard coverage")
    run_parser = subparsers.add_parser("run", help="validate and run one PTY shard")
    run_parser.add_argument("--shard", choices=sorted(SHARDS), required=True)
    subparsers.add_parser(
        "run-non-pty",
        help="run every non-PTY workspace target without executing the PTY binary",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "verify":
            print_coverage(discover_coverage())
        elif args.command == "run":
            run_shard(args.shard)
        else:
            run_non_pty()
    except ShardContractError as error:
        print(f"PTY shard contract failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
