#!/usr/bin/env python3
"""Stage allthecodes npm packages for release."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BUILD_SCRIPT = REPO_ROOT / "scripts" / "build_npm_package.py"
INSTALL_NATIVE_DEPS = REPO_ROOT / "scripts" / "install_native_deps.py"

_SPEC = importlib.util.spec_from_file_location("build_npm_package", BUILD_SCRIPT)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"Unable to load {BUILD_SCRIPT}")
_BUILD_MODULE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_BUILD_MODULE)
PACKAGE_NATIVE_COMPONENTS = getattr(_BUILD_MODULE, "PACKAGE_NATIVE_COMPONENTS", {})
PACKAGE_EXPANSIONS = getattr(_BUILD_MODULE, "PACKAGE_EXPANSIONS", {})
ALLTHECODES_PLATFORM_PACKAGES = getattr(_BUILD_MODULE, "ALLTHECODES_PLATFORM_PACKAGES", {})


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release-version",
        required=True,
        help="Version to stage (e.g. 0.1.0 or 0.1.0-alpha.1).",
    )
    parser.add_argument(
        "--package",
        dest="packages",
        action="append",
        required=True,
        help="Package name to stage. May be repeated.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Directory for npm tarballs (default: dist/npm).",
    )
    parser.add_argument(
        "--keep-staging-dirs",
        action="store_true",
        help="Retain temporary staging directories.",
    )
    return parser.parse_args()


def collect_native_components(packages: list[str]) -> set[str]:
    components: set[str] = set()
    for pkg in packages:
        components.update(PACKAGE_NATIVE_COMPONENTS.get(pkg, []))
    return components


def expand_packages(packages: list[str]) -> list[str]:
    expanded: list[str] = []
    for pkg in packages:
        for ep in PACKAGE_EXPANSIONS.get(pkg, [pkg]):
            if ep not in expanded:
                expanded.append(ep)
    return expanded


def install_native_components(
    components: set[str],
    vendor_root: Path,
    *,
    release_version: str,
) -> None:
    if not components:
        return

    # Build the native binary for the host platform.
    target = current_target_triple()
    print(f"Building allthecodes binary for {target}...")
    subprocess.run(
        ["cargo", "build", "--release", "--bin", "allthecodes"],
        cwd=REPO_ROOT,
        check=True,
    )

    # Copy the binary into the vendor layout expected by build_npm_package.py.
    binary_name = "allthecodes.exe" if os.name == "nt" else "allthecodes"
    binary_path = REPO_ROOT / "target" / "release" / binary_name
    if not binary_path.exists():
        raise RuntimeError(f"Binary not found: {binary_path}")

    target_dir = vendor_root / target / "allthecodes"
    target_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary_path, target_dir / binary_name)


def current_target_triple() -> str:
    """Get the current host target triple."""
    try:
        output = subprocess.check_output(
            ["rustc", "-vV"], text=True
        )
        for line in output.splitlines():
            if line.startswith("host:"):
                return line.split(":", 1)[1].strip()
    except FileNotFoundError:
        pass
    raise RuntimeError("Unable to determine host target triple via rustc.")


def tarball_name_for_package(package: str, version: str) -> str:
    if package in ALLTHECODES_PLATFORM_PACKAGES:
        platform = package.removeprefix("allthecodes-")
        return f"allthecodes-npm-{platform}-{version}.tgz"
    return f"{package}-npm-{version}.tgz"


def main() -> int:
    args = parse_args()
    output_dir = args.output_dir or (REPO_ROOT / "dist" / "npm")
    output_dir.mkdir(parents=True, exist_ok=True)

    runner_temp = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir()))
    packages = expand_packages(list(args.packages))

    vendor_temp_root: Path | None = None
    native_components = collect_native_components(packages)

    final_messages = []

    try:
        if native_components:
            vendor_temp_root = Path(tempfile.mkdtemp(prefix="allthecodes-vendor-", dir=runner_temp))
            vendor_root = vendor_temp_root / "vendor"
            install_native_components(native_components, vendor_root, release_version=args.release_version)

        for package in packages:
            staging_dir = Path(tempfile.mkdtemp(prefix=f"allthecodes-stage-{package}-", dir=runner_temp))
            pack_output = output_dir / tarball_name_for_package(package, args.release_version)

            cmd = [
                sys.executable,
                str(BUILD_SCRIPT),
                "--package",
                package,
                "--release-version",
                args.release_version,
                "--staging-dir",
                str(staging_dir),
                "--pack-output",
                str(pack_output),
            ]
            if vendor_temp_root is not None:
                cmd.extend(["--vendor-src", str(vendor_temp_root / "vendor")])

            try:
                subprocess.run(cmd, cwd=REPO_ROOT, check=True)
            finally:
                if not args.keep_staging_dirs:
                    shutil.rmtree(staging_dir, ignore_errors=True)

            final_messages.append(f"Staged {package} -> {pack_output}")

        # Copy installer scripts if they exist.
        install_sh = REPO_ROOT / "scripts" / "install.sh"
        if install_sh.exists():
            shutil.copy2(install_sh, output_dir / "install.sh")

    finally:
        if vendor_temp_root is not None and not args.keep_staging_dirs:
            shutil.rmtree(vendor_temp_root, ignore_errors=True)

    for msg in final_messages:
        print(msg)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
