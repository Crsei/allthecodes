#!/usr/bin/env python3
"""Static regression tests for CI and release quality gates."""

from __future__ import annotations

import re
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CI = ROOT / ".github" / "workflows" / "ci.yml"
RELEASE = ROOT / ".github" / "workflows" / "release.yml"
DENY = ROOT / "deny.toml"


def parse_workflow(path: Path) -> None:
    subprocess.run(
        ["ruby", "-e", "require 'yaml'; YAML.load_file(ARGV.fetch(0))", str(path)],
        check=True,
        capture_output=True,
        text=True,
    )


def cargo_lines(text: str) -> list[str]:
    return [
        line.strip()
        for line in text.splitlines()
        if re.search(r"^\s*(?:run:\s*)?(?:run\s+)?cargo\s+(build|check|clippy|test|nextest)\b", line)
    ]


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
