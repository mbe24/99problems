#!/usr/bin/env node
// Builds per-platform npm packages from CI artifacts and publishes them,
// then updates + publishes the main wrapper package.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const version = process.env.VERSION?.replace(/^v/, "");
if (!version) throw new Error("VERSION env var is required (e.g. v0.1.0)");

const platforms = [
  {
    pkg: "@mbe24/99problems-win32-x64",
    dir: "npm-win32-x64",
    artifact: "artifacts/binary-x86_64-pc-windows-msvc/99problems.exe",
    binary: "99problems.exe",
    os: "win32",
    cpu: "x64",
  },
  {
    pkg: "@mbe24/99problems-linux-x64",
    dir: "npm-linux-x64",
    artifact: "artifacts/binary-x86_64-unknown-linux-gnu/99problems",
    binary: "99problems",
    os: "linux",
    cpu: "x64",
  },
  {
    pkg: "@mbe24/99problems-darwin-x64",
    dir: "npm-darwin-x64",
    artifact: "artifacts/binary-x86_64-apple-darwin/99problems",
    binary: "99problems",
    os: "darwin",
    cpu: "x64",
  },
  {
    pkg: "@mbe24/99problems-darwin-arm64",
    dir: "npm-darwin-arm64",
    artifact: "artifacts/binary-aarch64-apple-darwin/99problems",
    binary: "99problems",
    os: "darwin",
    cpu: "arm64",
  },
  {
    pkg: "@mbe24/99problems-linux-arm64",
    dir: "npm-linux-arm64",
    artifact: "artifacts/binary-aarch64-unknown-linux-gnu/99problems",
    binary: "99problems",
    os: "linux",
    cpu: "arm64",
  },
];

for (const p of platforms) {
  const pkgDir = path.join(p.dir);
  fs.mkdirSync(path.join(pkgDir, "bin"), { recursive: true });

  // Copy binary
  fs.copyFileSync(p.artifact, path.join(pkgDir, "bin", p.binary));
  if (p.os !== "win32") {
    fs.chmodSync(path.join(pkgDir, "bin", p.binary), 0o755);
  }

  // Write package.json
  const pkgJson = {
    name: p.pkg,
    version,
    description: `${p.os}-${p.cpu} binary for @mbe24/99problems`,
    os: [p.os],
    cpu: [p.cpu],
    main: `bin/${p.binary}`,
    license: "Apache-2.0",
    repository: { type: "git", url: "https://github.com/mbe24/99problems" },
  };
  fs.writeFileSync(
    path.join(pkgDir, "package.json"),
    JSON.stringify(pkgJson, null, 2) + "\n"
  );

  console.log(`Publishing ${p.pkg}@${version} ...`);
  execSync("npm publish --access public", { cwd: pkgDir, stdio: "inherit" });
}

// Update main package.json version + optionalDependencies versions
const mainPkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
mainPkg.version = version;
for (const p of platforms) {
  mainPkg.optionalDependencies[p.pkg] = version;
}
fs.writeFileSync("package.json", JSON.stringify(mainPkg, null, 2) + "\n");

console.log(`Publishing @mbe24/99problems@${version} ...`);
execSync("npm publish --access public", { stdio: "inherit" });
