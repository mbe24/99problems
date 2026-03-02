#!/usr/bin/env node
// Thin shim: finds the platform-specific binary and executes it.
const { execFileSync } = require("child_process");
const path = require("path");
const fs = require("fs");

const PLATFORMS = {
  "win32-x64":    "@mbe24/99problems-win32-x64",
  "linux-x64":    "@mbe24/99problems-linux-x64",
  "linux-arm64":  "@mbe24/99problems-linux-arm64",
  "darwin-x64":   "@mbe24/99problems-darwin-x64",
  "darwin-arm64": "@mbe24/99problems-darwin-arm64",
};

const key = `${process.platform}-${process.arch}`;
const pkg = PLATFORMS[key];

if (!pkg) {
  console.error(`99problems: unsupported platform ${key}`);
  process.exit(1);
}

const binaryName = process.platform === "win32" ? "99problems.exe" : "99problems";

let binaryPath;
try {
  binaryPath = require.resolve(`${pkg}/${binaryName}`);
} catch {
  // Fallback: look for binary placed by install.js next to this shim
  binaryPath = path.join(__dirname, binaryName);
}

if (!fs.existsSync(binaryPath)) {
  console.error(`99problems: binary not found at ${binaryPath}`);
  process.exit(1);
}

try {
  execFileSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
} catch (err) {
  process.exit(err.status ?? 1);
}
