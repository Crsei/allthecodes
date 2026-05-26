#!/usr/bin/env python3
"""Stage and optionally pack the allthecodes npm module."""

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent

CLI_NAME = "allthecodes"
NPM_NAME = "allthecodes"

# Platform-specific package definitions.
ALLTHECODES_PLATFORM_PACKAGES: dict[str, dict[str, str]] = {
    "allthecodes-linux-x64": {
        "npm_name": "allthecodes-linux-x64",
        "npm_tag": "linux-x64",
        "target_triple": "x86_64-unknown-linux-gnu",
        "os": "linux",
        "cpu": "x64",
    },
    "allthecodes-linux-arm64": {
        "npm_name": "allthecodes-linux-arm64",
        "npm_tag": "linux-arm64",
        "target_triple": "aarch64-unknown-linux-gnu",
        "os": "linux",
        "cpu": "arm64",
    },
    "allthecodes-darwin-x64": {
        "npm_name": "allthecodes-darwin-x64",
        "npm_tag": "darwin-x64",
        "target_triple": "x86_64-apple-darwin",
        "os": "darwin",
        "cpu": "x64",
    },
    "allthecodes-darwin-arm64": {
        "npm_name": "allthecodes-darwin-arm64",
        "npm_tag": "darwin-arm64",
        "target_triple": "aarch64-apple-darwin",
        "os": "darwin",
        "cpu": "arm64",
    },
    "allthecodes-win32-x64": {
        "npm_name": "allthecodes-win32-x64",
        "npm_tag": "win32-x64",
        "target_triple": "x86_64-pc-windows-msvc",
        "os": "win32",
        "cpu": "x64",
    },
    "allthecodes-win32-arm64": {
        "npm_name": "allthecodes-win32-arm64",
        "npm_tag": "win32-arm64",
        "target_triple": "aarch64-pc-windows-msvc",
        "os": "win32",
        "cpu": "arm64",
    },
}

# When --package allthecodes is given, expand to all packages.
PACKAGE_EXPANSIONS: dict[str, list[str]] = {
    "allthecodes": ["allthecodes", *ALLTHECODES_PLATFORM_PACKAGES],
}

# Native component definitions per package.
PACKAGE_NATIVE_COMPONENTS: dict[str, list[str]] = {
    "allthecodes": [],
    "allthecodes-linux-x64": ["allthecodes"],
    "allthecodes-linux-arm64": ["allthecodes"],
    "allthecodes-darwin-x64": ["allthecodes"],
    "allthecodes-darwin-arm64": ["allthecodes"],
    "allthecodes-win32-x64": ["allthecodes"],
    "allthecodes-win32-arm64": ["allthecodes"],
}

PACKAGE_TARGET_FILTERS: dict[str, str] = {
    pkg: cfg["target_triple"]
    for pkg, cfg in ALLTHECODES_PLATFORM_PACKAGES.items()
}

PACKAGE_CHOICES = tuple(PACKAGE_NATIVE_COMPONENTS)

# Maps native component name to its subdirectory under the vendor target dir.
COMPONENT_DEST_DIR: dict[str, str] = {
    "allthecodes": "allthecodes",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or stage the allthecodes npm package.")
    parser.add_argument(
        "--package",
        choices=PACKAGE_CHOICES,
        default="allthecodes",
        help="Which npm package to stage (default: allthecodes).",
    )
    parser.add_argument(
        "--release-version",
        help="Version to stage for npm release (e.g. 0.1.0).",
    )
    parser.add_argument(
        "--staging-dir",
        type=Path,
        help="Directory to stage the package contents. Must be empty if provided.",
    )
    parser.add_argument(
        "--pack-output",
        type=Path,
        help="Path where the generated npm tarball should be written.",
    )
    parser.add_argument(
        "--vendor-src",
        type=Path,
        help="Directory containing pre-built native binaries to bundle (vendor root).",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    package = args.package
    version = args.release_version
    if not version:
        raise RuntimeError("Must specify --release-version.")

    staging_dir, created_temp = prepare_staging_dir(args.staging_dir)
    try:
        stage_sources(staging_dir, version, package)

        vendor_src = args.vendor_src.resolve() if args.vendor_src else None
        native_components = PACKAGE_NATIVE_COMPONENTS.get(package, [])
        target_filter = PACKAGE_TARGET_FILTERS.get(package)

        if native_components:
            if vendor_src is None:
                raise RuntimeError(
                    f"Native components ({', '.join(native_components)}) required "
                    f"for package '{package}'. Provide --vendor-src."
                )
            copy_native_binaries(
                vendor_src,
                staging_dir,
                native_components,
                target_filter={target_filter} if target_filter else None,
            )

        if args.pack_output is not None:
            output_path = run_npm_pack(staging_dir, args.pack_output)
            print(f"npm pack output: {output_path}")

        print(f"Staged {package} v{version} in {staging_dir}")
    finally:
        if created_temp:
            shutil.rmtree(staging_dir, ignore_errors=True)

    return 0


def prepare_staging_dir(staging_dir: Path | None) -> tuple[Path, bool]:
    if staging_dir is not None:
        staging_dir = staging_dir.resolve()
        staging_dir.mkdir(parents=True, exist_ok=True)
        if any(staging_dir.iterdir()):
            raise RuntimeError(f"Staging directory {staging_dir} is not empty.")
        return staging_dir, False
    temp_dir = Path(tempfile.mkdtemp(prefix="allthecodes-npm-stage-"))
    return temp_dir, True


def stage_sources(staging_dir: Path, version: str, package: str) -> None:
    package_json: dict

    if package == "allthecodes":
        # Root wrapper package: ship the JS launcher.
        bin_dir = staging_dir / "bin"
        bin_dir.mkdir(parents=True, exist_ok=True)
        shutil.copy2(REPO_ROOT / "bin" / "allthecodes.js", bin_dir / "allthecodes.js")

        readme = REPO_ROOT / "README.md"
        if readme.exists():
            shutil.copy2(readme, staging_dir / "README.md")

        # Build package.json for the root wrapper.
        with open(REPO_ROOT / "package.json") as fh:
            package_json = json.load(fh)
        package_json["version"] = version
        package_json["files"] = ["bin"]
        package_json["optionalDependencies"] = {
            ALLTHECODES_PLATFORM_PACKAGES[pp]["npm_name"]: (
                f"npm:{NPM_NAME}@"
                f"{compute_platform_version(version, ALLTHECODES_PLATFORM_PACKAGES[pp]['npm_tag'])}"
            )
            for pp in ALLTHECODES_PLATFORM_PACKAGES
        }

    elif package in ALLTHECODES_PLATFORM_PACKAGES:
        # Platform-specific binary package.
        pp = ALLTHECODES_PLATFORM_PACKAGES[package]
        platform_version = compute_platform_version(version, pp["npm_tag"])

        readme = REPO_ROOT / "README.md"
        if readme.exists():
            shutil.copy2(readme, staging_dir / "README.md")

        package_json = {
            "name": NPM_NAME,
            "version": platform_version,
            "license": "Apache-2.0",
            "os": [pp["os"]],
            "cpu": [pp["cpu"]],
            "files": ["vendor"],
            "repository": {
                "type": "git",
                "url": "git+https://github.com/Crsei/allthecodes.git",
                "directory": "allthecodes",
            },
        }
    else:
        raise RuntimeError(f"Unknown package '{package}'.")

    with open(staging_dir / "package.json", "w") as out:
        json.dump(package_json, out, indent=2)
        out.write("\n")


def compute_platform_version(version: str, platform_tag: str) -> str:
    return f"{version}-{platform_tag}"


def copy_native_binaries(
    vendor_src: Path,
    staging_dir: Path,
    components: list[str],
    target_filter: set[str] | None = None,
) -> None:
    vendor_src = vendor_src.resolve()
    if not vendor_src.exists():
        raise RuntimeError(f"Vendor source not found: {vendor_src}")

    components_set = {c for c in components if c in COMPONENT_DEST_DIR}
    if not components_set:
        return

    vendor_dest = staging_dir / "vendor"
    if vendor_dest.exists():
        shutil.rmtree(vendor_dest)
    vendor_dest.mkdir(parents=True)

    copied_targets: set[str] = set()

    for target_dir in vendor_src.iterdir():
        if not target_dir.is_dir():
            continue
        if target_filter is not None and target_dir.name not in target_filter:
            continue

        dest_target_dir = vendor_dest / target_dir.name
        dest_target_dir.mkdir(parents=True)
        copied_targets.add(target_dir.name)

        for component in components_set:
            dest_dir_name = COMPONENT_DEST_DIR.get(component)
            if dest_dir_name is None:
                continue
            src_component_dir = target_dir / dest_dir_name
            if not src_component_dir.exists():
                raise RuntimeError(
                    f"Missing native component '{component}' in vendor: {src_component_dir}"
                )
            dest_component_dir = dest_target_dir / dest_dir_name
            if dest_component_dir.exists():
                shutil.rmtree(dest_component_dir)
            shutil.copytree(src_component_dir, dest_component_dir)

    if target_filter is not None:
        missing = sorted(target_filter - copied_targets)
        if missing:
            raise RuntimeError(f"Missing targets in vendor source: {missing}")


def run_npm_pack(staging_dir: Path, output_path: Path) -> Path:
    output_path = output_path.resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="allthecodes-npm-pack-") as pack_dir_str:
        pack_dir = Path(pack_dir_str)
        stdout = subprocess.check_output(
            ["npm", "pack", "--json", "--pack-destination", str(pack_dir)],
            cwd=staging_dir,
            text=True,
        )
        try:
            pack_output = json.loads(stdout)
        except json.JSONDecodeError:
            raise RuntimeError("Failed to parse npm pack output.")

        if not pack_output:
            raise RuntimeError("npm pack produced no output.")

        tarball_name = pack_output[0].get("filename") or pack_output[0].get("name")
        if not tarball_name:
            raise RuntimeError("Cannot determine npm pack output filename.")

        tarball_path = pack_dir / tarball_name
        if not tarball_path.exists():
            raise RuntimeError(f"npm pack output not found: {tarball_path}")

        shutil.move(str(tarball_path), output_path)

    return output_path


if __name__ == "__main__":
    sys.exit(main())
