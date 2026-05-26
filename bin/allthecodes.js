#!/usr/bin/env node
// Unified entry point for the allthecodes CLI.
// Detects the current platform, locates the native Rust binary,
// and spawns it with forwarded stdio and signals.

import { spawn } from "node:child_process";
import { existsSync, realpathSync } from "fs";
import { createRequire } from "node:module";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

const BINARY_NAME = process.platform === "win32" ? "allthecodes.exe" : "allthecodes";
const COMPONENT_DIR = "allthecodes";

const PLATFORM_PACKAGE_BY_TARGET = {
  "x86_64-unknown-linux-gnu": "allthecodes-linux-x64",
  "aarch64-unknown-linux-gnu": "allthecodes-linux-arm64",
  "x86_64-apple-darwin": "allthecodes-darwin-x64",
  "aarch64-apple-darwin": "allthecodes-darwin-arm64",
  "x86_64-pc-windows-msvc": "allthecodes-win32-x64",
  "aarch64-pc-windows-msvc": "allthecodes-win32-arm64",
};

function detectTarget() {
  const { platform, arch } = process;
  switch (platform) {
    case "linux":
      switch (arch) {
        case "x64": return "x86_64-unknown-linux-gnu";
        case "arm64": return "aarch64-unknown-linux-gnu";
      }
      break;
    case "darwin":
      switch (arch) {
        case "x64": return "x86_64-apple-darwin";
        case "arm64": return "aarch64-apple-darwin";
      }
      break;
    case "win32":
      switch (arch) {
        case "x64": return "x86_64-pc-windows-msvc";
        case "arm64": return "aarch64-pc-windows-msvc";
      }
      break;
  }
  throw new Error(`Unsupported platform: ${platform} (${arch})`);
}

const targetTriple = detectTarget();
const platformPackage = PLATFORM_PACKAGE_BY_TARGET[targetTriple];

// Resolve the vendor root: first try the platform-specific optional dependency,
// then fall back to a local vendor directory (development / local build).
const localVendorRoot = path.join(__dirname, "..", "vendor");
const localBinaryPath = path.join(localVendorRoot, targetTriple, COMPONENT_DIR, BINARY_NAME);

let vendorRoot;
try {
  const pkgJsonPath = require.resolve(`${platformPackage}/package.json`);
  vendorRoot = path.join(path.dirname(pkgJsonPath), "vendor");
} catch {
  if (existsSync(localBinaryPath)) {
    vendorRoot = localVendorRoot;
  } else {
    const updateCmd = process.env.npm_config_user_agent?.includes("bun")
      ? "bun install -g allthecodes@latest"
      : "npm install -g allthecodes@latest";
    throw new Error(
      `Missing platform binary for ${targetTriple}.\n` +
      `Reinstall: ${updateCmd}`
    );
  }
}

const archRoot = path.join(vendorRoot, targetTriple);
const binaryPath = path.join(archRoot, COMPONENT_DIR, BINARY_NAME);

// Augment PATH with bundled tool directories, if any (e.g. ripgrep, sandbox).
const bundledPathDirs = [];
const bundledPathDir = path.join(archRoot, "path");
if (existsSync(bundledPathDir)) {
  bundledPathDirs.push(bundledPathDir);
}

const env = { ...process.env };
if (bundledPathDirs.length > 0) {
  const pathSep = process.platform === "win32" ? ";" : ":";
  env.PATH = [
    ...bundledPathDirs,
    ...(env.PATH || "").split(pathSep).filter(Boolean),
  ].join(pathSep);
}
env.ALLTHECODES_MANAGED_PACKAGE_ROOT = realpathSync(path.join(__dirname, ".."));

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env,
});

child.on("error", (err) => {
  console.error("Failed to launch allthecodes:", err.message);
  process.exit(1);
});

// Forward termination signals to the child process.
function forwardSignal(signal) {
  if (child.killed) return;
  try { child.kill(signal); } catch { /* ignore */ }
}

["SIGINT", "SIGTERM", "SIGHUP"].forEach((sig) => {
  process.on(sig, () => forwardSignal(sig));
});

const childResult = await new Promise((resolve) => {
  child.on("exit", (code, signal) => {
    resolve(signal ? { signal } : { exitCode: code ?? 1 });
  });
});

if (childResult.signal) {
  process.kill(process.pid, childResult.signal);
} else {
  process.exit(childResult.exitCode);
}
