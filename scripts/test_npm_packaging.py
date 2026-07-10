#!/usr/bin/env python3
"""Fast npm package staging and launcher smoke test."""

from __future__ import annotations

import json
import os
import platform
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
BUILD = ROOT / "scripts" / "build_npm_package.py"
VERSION = "0.0.0-ci.0"


def host_package() -> tuple[str, str, str]:
    machine = platform.machine().lower()
    arch = "arm64" if machine in {"aarch64", "arm64"} else "x64"
    if os.name == "nt":
        return f"allthecodes-win32-{arch}", f"{'aarch64' if arch == 'arm64' else 'x86_64'}-pc-windows-msvc", "allthecodes.exe"
    if platform.system() == "Darwin":
        return f"allthecodes-darwin-{arch}", f"{'aarch64' if arch == 'arm64' else 'x86_64'}-apple-darwin", "allthecodes"
    return f"allthecodes-linux-{arch}", f"{'aarch64' if arch == 'arm64' else 'x86_64'}-unknown-linux-gnu", "allthecodes"


class NpmPackagingSmoke(unittest.TestCase):
    def test_stage_pack_install_and_launch(self) -> None:
        package, target, binary_name = host_package()
        with tempfile.TemporaryDirectory(prefix="allthecodes-npm-smoke-") as tmp:
            root = Path(tmp)
            vendor = root / "vendor" / target / "allthecodes"
            vendor.mkdir(parents=True)
            binary = vendor / binary_name
            if os.name == "nt":
                binary.write_bytes((Path(os.environ["SystemRoot"]) / "System32" / "where.exe").read_bytes())
            else:
                binary.write_text("#!/bin/sh\nprintf 'launcher-smoke\\n'\n", encoding="utf-8")
                binary.chmod(0o755)

            tarballs = root / "tarballs"
            tarballs.mkdir()
            platform_tarball = tarballs / "platform.tgz"
            root_tarball = tarballs / "root.tgz"
            for selected, output in ((package, platform_tarball), ("allthecodes", root_tarball)):
                subprocess.run(
                    [
                        "python3", str(BUILD), "--package", selected,
                        "--release-version", VERSION, "--staging-dir", str(root / f"stage-{selected}"),
                        "--pack-output", str(output), *([] if selected == "allthecodes" else ["--vendor-src", str(root / "vendor")]),
                    ],
                    cwd=ROOT,
                    check=True,
                )

            project = root / "project"
            project.mkdir()
            (project / "package.json").write_text(json.dumps({"private": True}), encoding="utf-8")
            subprocess.run(
                [
                    "npm", "install", "--ignore-scripts", "--no-audit", "--no-fund",
                    f"{package}@file:{platform_tarball}", str(root_tarball),
                ],
                cwd=project,
                check=True,
            )
            result = subprocess.run(
                [
                    "node",
                    str(project / "node_modules" / "allthecodes" / "bin" / "allthecodes.js"),
                    "where",
                ],
                cwd=project,
                text=True,
                capture_output=True,
                check=True,
            )
            if os.name != "nt":
                self.assertEqual("launcher-smoke", result.stdout.strip())


if __name__ == "__main__":
    unittest.main(verbosity=2)
