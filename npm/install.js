// postinstall: verify the platform binary is available.
const { execSync } = require("child_process");
const path = require("path");

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
  console.warn(`[99problems] No prebuilt binary for platform ${key}. You may need to build from source.`);
  process.exit(0);
}

try {
  require.resolve(`${pkg}/99problems${process.platform === "win32" ? ".exe" : ""}`);
  console.log(`[99problems] Binary found for ${key}.`);
} catch {
  console.warn(`[99problems] Optional dependency ${pkg} not installed (this is OK if you're building from source).`);
}
