#!/usr/bin/env python3
"""Unit contracts for the cargo-native PTY shard runner."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


RUNNER_PATH = Path(__file__).with_name("pty_tui_shards.py")


def load_runner(testcase: unittest.TestCase):
    testcase.assertTrue(RUNNER_PATH.is_file(), "missing scripts/pty_tui_shards.py")
    spec = importlib.util.spec_from_file_location("pty_tui_shards", RUNNER_PATH)
    testcase.assertIsNotNone(spec)
    testcase.assertIsNotNone(spec.loader)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PtyShardRunnerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner(self)

    def test_parse_test_list_ignores_summary_and_benchmarks(self) -> None:
        output = """\
tests::alpha::first: test
tests::alpha::second: test
tests::bench::speed: benchmark

2 tests, 1 benchmark
"""
        self.assertEqual(
            {"tests::alpha::first", "tests::alpha::second"},
            self.runner.parse_test_list(output),
        )

    def test_validate_coverage_is_complete_disjoint_and_tracks_ignored(self) -> None:
        shards = {
            "one": ("tests::alpha",),
            "two": ("tests::beta",),
        }
        all_tests = {
            "tests::alpha::first",
            "tests::alpha::real_api",
            "tests::beta::second",
        }
        coverage = self.runner.validate_coverage(
            all_tests,
            {"tests::alpha::real_api"},
            shards=shards,
            expected_total=3,
            expected_ignored=1,
            expected_default=2,
        )

        self.assertEqual(all_tests, set().union(*coverage.assignments.values()))
        self.assertEqual(2, len(coverage.default_tests))
        self.assertEqual(
            {"tests::alpha::first"},
            coverage.default_assignments["one"],
        )

    def test_validate_coverage_rejects_prefix_gaps(self) -> None:
        with self.assertRaisesRegex(self.runner.ShardContractError, "unassigned"):
            self.runner.validate_coverage(
                {"tests::alpha::first", "tests::missing::test"},
                set(),
                shards={"one": ("tests::alpha",)},
                expected_total=2,
                expected_ignored=0,
                expected_default=2,
            )

    def test_validate_coverage_rejects_prefix_overlaps(self) -> None:
        with self.assertRaisesRegex(self.runner.ShardContractError, "multiple"):
            self.runner.validate_coverage(
                {"tests::alpha::nested::test"},
                set(),
                shards={
                    "one": ("tests::alpha",),
                    "two": ("tests::alpha::nested",),
                },
                expected_total=1,
                expected_ignored=0,
                expected_default=1,
            )

    def test_validate_coverage_uses_complete_prefix_boundaries(self) -> None:
        with self.assertRaisesRegex(self.runner.ShardContractError, "unassigned"):
            self.runner.validate_coverage(
                {"tests::alphabet::test"},
                set(),
                shards={"one": ("tests::alpha",)},
                expected_total=1,
                expected_ignored=0,
                expected_default=1,
            )

    def test_unsafe_cargo_filter_falls_back_to_exact_test_commands(self) -> None:
        commands = self.runner.pty_test_commands(
            "tests::alpha",
            runnable_tests={"tests::alpha::direct"},
            all_tests={
                "tests::alpha::direct",
                "outer::tests::alpha::nested",
            },
            ignored_tests=set(),
        )

        self.assertEqual(1, len(commands))
        self.assertIn("tests::alpha::direct", commands[0])
        self.assertIn("--exact", commands[0])
        self.assertIn("--test-threads=1", commands[0])

    def test_pty_command_is_locked_cargo_and_single_threaded(self) -> None:
        command = self.runner.pty_test_command("tests::alpha")
        self.assertEqual("cargo", command[0])
        self.assertIn("--locked", command)
        self.assertNotIn("nextest", command)
        self.assertIn("tests::alpha::", command)
        self.assertIn("--test-threads=1", command)

    def test_commands_mcp_plugin_has_a_dedicated_shard(self) -> None:
        self.assertEqual(
            ("tests::commands_mcp_plugin",),
            self.runner.SHARDS["commands-mcp-plugin"],
        )

    def test_isolated_environment_is_required(self) -> None:
        with self.assertRaisesRegex(self.runner.ShardContractError, "E2E_WORKSPACE"):
            self.runner.require_isolated_environment({})
        with self.assertRaisesRegex(self.runner.ShardContractError, "must differ"):
            self.runner.require_isolated_environment(
                {"E2E_WORKSPACE": "/tmp/shared", "ALLTHECODES_HOME": "/tmp/shared"}
            )
        self.runner.require_isolated_environment(
            {"E2E_WORKSPACE": "/tmp/workspace", "ALLTHECODES_HOME": "/tmp/home"}
        )

    def test_non_pty_commands_discover_every_integration_target(self) -> None:
        metadata = {
            "packages": [
                {
                    "name": "allthecodes",
                    "targets": [
                        {"name": "allthecodes", "kind": ["lib"]},
                        {"name": "allthecodes", "kind": ["bin"]},
                        {"name": "pty_tui_e2e", "kind": ["test"]},
                        {"name": "e2e_cli", "kind": ["test"]},
                        {"name": "future_regression", "kind": ["test"]},
                        {"name": "demo", "kind": ["example"]},
                    ],
                }
            ]
        }

        commands = self.runner.non_pty_commands(metadata)
        flattened = [argument for command in commands for argument in command]
        self.assertIn("--workspace", commands[0])
        self.assertIn("--exclude", commands[0])
        self.assertIn("e2e_cli", flattened)
        self.assertIn("future_regression", flattened)
        self.assertIn("demo", flattened)
        self.assertNotIn("pty_tui_e2e", flattened)
        self.assertTrue(any("--doc" in command for command in commands))

    def test_non_pty_commands_fail_if_pty_target_is_missing(self) -> None:
        metadata = {
            "packages": [
                {
                    "name": "allthecodes",
                    "targets": [{"name": "allthecodes", "kind": ["bin"]}],
                }
            ]
        }
        with self.assertRaisesRegex(self.runner.ShardContractError, "pty_tui_e2e"):
            self.runner.non_pty_commands(metadata)


if __name__ == "__main__":
    unittest.main(verbosity=2)
