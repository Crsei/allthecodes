#!/usr/bin/env python3
"""Static regression tests for CI and release quality gates."""

from __future__ import annotations

import re
import shlex
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import pty_tui_shards  # noqa: E402


CI = ROOT / ".github" / "workflows" / "ci.yml"
RELEASE = ROOT / ".github" / "workflows" / "release.yml"
DENY = ROOT / "deny.toml"
PTY_SHARD_RUNNER = ROOT / "scripts" / "pty_tui_shards.py"
PTY_SAFE_RUST_JOB_TEST_PACKAGES = frozenset({"allthecodes-tools"})


def parse_workflow(path: Path) -> None:
    subprocess.run(
        ["ruby", "-e", "require 'yaml'; YAML.load_file(ARGV.fetch(0))", str(path)],
        check=True,
        capture_output=True,
        text=True,
    )


def cargo_lines(text: str) -> list[str]:
    text = re.sub(r"\\[ \t]*\n[ \t]*", " ", text)
    return [
        line.strip()
        for line in text.splitlines()
        if re.search(
            r"^\s*(?:run:\s*)?(?:run\s+)?cargo\s+(?:\+\S+\s+)?"
            r"(build|check|clippy|test|nextest)\b",
            line,
        )
    ]


def cargo_test_is_pty_safe(command: str) -> bool:
    tokens = shlex.split(command)
    try:
        cargo_index = tokens.index("cargo")
    except ValueError:
        return False

    subcommand_index = cargo_index + 1
    if subcommand_index < len(tokens) and tokens[subcommand_index].startswith("+"):
        subcommand_index += 1
    if subcommand_index >= len(tokens) or tokens[subcommand_index] != "test":
        return False

    arguments = tokens[subcommand_index + 1 :]
    if "--" in arguments:
        arguments = arguments[: arguments.index("--")]
    if {"--workspace", "--all"} & set(arguments):
        return False

    packages = []
    index = 0
    while index < len(arguments):
        argument = arguments[index]
        if argument in {"-p", "--package"}:
            if index + 1 >= len(arguments) or arguments[index + 1].startswith("-"):
                return False
            packages.append(arguments[index + 1])
            index += 2
            continue
        if argument.startswith("--package="):
            package = argument.removeprefix("--package=")
            if not package:
                return False
            packages.append(package)
        index += 1

    return bool(packages) and set(packages) <= PTY_SAFE_RUST_JOB_TEST_PACKAGES


class WorkflowQualityGates(unittest.TestCase):
    def test_workflows_parse_as_yaml(self) -> None:
        parse_workflow(CI)
        parse_workflow(RELEASE)

    def test_build_and_verification_cargo_commands_are_locked(self) -> None:
        files = [CI, RELEASE, ROOT / "scripts" / "cargo-build-test.sh", ROOT / "scripts" / "stage_npm_packages.py"]
        offenders = [
            f"{path.relative_to(ROOT)}: {line}"
            for path in files
            for line in cargo_lines(path.read_text(encoding="utf-8"))
            if "--locked" not in line
        ]
        self.assertEqual([], offenders)

    def test_ci_has_representative_feature_and_codegen_checks(self) -> None:
        text = CI.read_text(encoding="utf-8")
        self.assertIn("--no-default-features", text)
        self.assertRegex(text, r"allthecodes-protocol.+--features codegen.+--bins")
        self.assertRegex(text, r"-p allthecodes\b.+--all-features")
        self.assertRegex(
            text,
            r"-p allthecodes-services.+--no-default-features.+--features json-storage",
        )
        self.assertRegex(
            text,
            r"-p allthecodes-tasks.+--no-default-features.+--features json-storage",
        )

    def test_all_workspace_members_inherit_lints(self) -> None:
        manifests = list((ROOT / "crates").glob("*/Cargo.toml"))
        offenders = []
        for manifest in manifests:
            text = manifest.read_text(encoding="utf-8")
            lint_section = text.split("[lints]", 1)[1].split("\n[", 1)[0] if "[lints]" in text else ""
            if not re.search(r"^\s*workspace\s*=\s*true\s*$", lint_section, re.MULTILINE):
                offenders.append(str(manifest.relative_to(ROOT)))
        self.assertEqual([], offenders)

    def test_ci_has_npm_and_supply_chain_gates(self) -> None:
        text = CI.read_text(encoding="utf-8")
        self.assertTrue(DENY.is_file())
        self.assertIn("test_npm_packaging.py", text)
        self.assertRegex(text, r"cargo audit(?:\s|$)")
        self.assertRegex(text, r"cargo deny\s+--locked\s+check\s+advisories\s+bans\s+licenses\s+sources")

    def test_ci_checks_linux_macos_and_windows(self) -> None:
        text = CI.read_text(encoding="utf-8")
        platform_job = text.split("  platform-check:", 1)[1]
        runners = set(re.findall(r"^\s+os:\s+([^\s]+)$", platform_job, re.MULTILINE))
        self.assertTrue(any(name.startswith("ubuntu-") for name in runners))
        self.assertTrue(any(name.startswith("macos-") for name in runners))
        self.assertTrue(any(name.startswith("windows-") for name in runners))

    def test_cache_keys_hash_cargo_lock(self) -> None:
        for path in (CI, RELEASE):
            text = path.read_text(encoding="utf-8")
            keys = re.findall(r"^\s*key:\s*(.+)$", text, re.MULTILINE)
            self.assertTrue(keys, f"no explicit cache key in {path}")
            self.assertTrue(all("Cargo.lock" in key and "hashFiles" in key for key in keys))

    def test_workspace_clippy_remains_blocking(self) -> None:
        text = CI.read_text(encoding="utf-8")
        self.assertIn("Workspace Clippy", text)
        self.assertRegex(text, r"cargo clippy --locked --workspace --all-targets -- -D warnings")
        clippy_step = text.split("      - name: Workspace Clippy", 1)[1].split("      - name:", 1)[0]
        self.assertNotIn("continue-on-error", clippy_step)

    def test_ci_has_fail_closed_isolated_pty_shards(self) -> None:
        text = CI.read_text(encoding="utf-8")
        self.assertIn("  pty-tests:", text)
        pty_job = text.split("  pty-tests:", 1)[1].split("\n  npm-smoke:", 1)[0]

        self.assertIn("runs-on: ubuntu-22.04", pty_job)
        self.assertIn("fail-fast: false", pty_job)
        self.assertNotIn("continue-on-error", pty_job)
        shard_matrix = pty_job.split("        shard:", 1)[1].split("    steps:", 1)[0]
        shards = set(re.findall(r"^          - ([a-z0-9-]+)$", shard_matrix, re.MULTILINE))
        self.assertEqual(set(pty_tui_shards.SHARDS), shards)

        self.assertIn(
            "E2E_WORKSPACE: ${{ runner.temp }}/pty-workspace-${{ matrix.shard }}",
            pty_job,
        )
        self.assertIn(
            "ALLTHECODES_HOME: ${{ runner.temp }}/pty-home-${{ matrix.shard }}",
            pty_job,
        )
        self.assertIn("python3 scripts/pty_tui_shards.py verify", pty_job)
        self.assertIn("python3 scripts/pty_tui_shards.py run", pty_job)
        self.assertIn('"${{ matrix.shard }}"', pty_job)
        run_step = pty_job.split("      - name: Run PTY shard", 1)[1].split(
            "      - name:", 1
        )[0]
        job_timeout_match = re.search(r"^    timeout-minutes:\s*(\d+)$", pty_job, re.MULTILINE)
        run_timeout_match = re.search(r"^        timeout-minutes:\s*(\d+)$", run_step, re.MULTILINE)
        self.assertIsNotNone(job_timeout_match)
        self.assertIsNotNone(run_timeout_match)
        job_timeout = int(job_timeout_match.group(1))
        run_timeout = int(run_timeout_match.group(1))
        self.assertEqual(60, job_timeout)
        self.assertEqual(40, run_timeout)
        self.assertGreaterEqual(job_timeout - run_timeout, 10)
        self.assertIn("actions/upload-artifact@v4", pty_job)
        self.assertIn("if: failure()", pty_job)
        self.assertIn("name: pty-logs-${{ matrix.shard }}", pty_job)
        self.assertIn("path: crates/allthecodes/logs/pty_tui_e2e_*", pty_job)

    def test_pty_shard_runner_is_cargo_native_and_serial(self) -> None:
        self.assertTrue(PTY_SHARD_RUNNER.is_file(), "missing PTY shard runner")
        text = PTY_SHARD_RUNNER.read_text(encoding="utf-8")
        self.assertIn("cargo", text)
        self.assertNotIn("nextest", text)
        self.assertIn("--test-threads=1", text)
        self.assertIn("EXPECTED_TOTAL = 248", text)
        self.assertIn("EXPECTED_IGNORED = 34", text)
        self.assertIn("EXPECTED_DEFAULT = 214", text)

    def test_rust_job_delegates_non_pty_workspace_coverage(self) -> None:
        text = CI.read_text(encoding="utf-8")
        rust_job = text.split("  rust:", 1)[1].split("\n  pty-tests:", 1)[0]
        for command in (
            "cargo test",
            "cargo test --all",
            "cargo test --workspace --all-targets",
            "cargo test --workspace -p allthecodes-tools",
            "cargo test --all -p allthecodes-tools",
            "cargo test -p allthecodes",
        ):
            self.assertFalse(cargo_test_is_pty_safe(command))
        for command in (
            "cargo test -p allthecodes-tools",
            "cargo test --package allthecodes-tools",
            "cargo +stable test --package=allthecodes-tools",
        ):
            self.assertTrue(cargo_test_is_pty_safe(command))

        direct_tests = [
            line
            for line in cargo_lines(rust_job)
            if re.search(r"\bcargo\s+(?:\+\S+\s+)?test\b", line)
        ]
        unsafe_tests = [
            line for line in direct_tests if not cargo_test_is_pty_safe(line)
        ]
        self.assertEqual([], unsafe_tests)
        self.assertIn("python3 scripts/pty_tui_shards.py run-non-pty", rust_job)

    def test_prerelease_root_wrapper_gets_non_latest_tag(self) -> None:
        text = RELEASE.read_text(encoding="utf-8")
        self.assertIn('root_tag="$(scripts/npm-dist-tag.sh "${VERSION}")"', text)
        self.assertRegex(
            text,
            r'publish_if_missing\s+"allthecodes"\s+"\$\{VERSION\}"\s+"\$\{root_tarball\}"\s+"\$\{root_tag\}"',
        )

    def test_platform_publish_checks_the_actual_package_and_version(self) -> None:
        text = RELEASE.read_text(encoding="utf-8")
        self.assertIn('npm view "${package_name}@${version}" version', text)
        self.assertIn('package_name="allthecodes-${platform}"', text)
        self.assertRegex(
            text,
            r'publish_if_missing\s+"\$\{package_name\}"\s+"\$\{VERSION\}"\s+"\$\{tarball\}"\s+"\$\{platform\}"',
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
