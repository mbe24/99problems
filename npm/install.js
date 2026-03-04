// postinstall: verify the platform binary is available and install shell completions when possible.
const { execFileSync } = require("child_process");
const fs = require("fs");
const os = require("os");
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
  const binaryName = process.platform === "win32" ? "99problems.exe" : "99problems";
  const binaryPath = require.resolve(`${pkg}/bin/${binaryName}`);
  console.log(`[99problems] Binary found for ${key}.`);
  installCompletions(binaryPath);
} catch {
  console.warn(`[99problems] Optional dependency ${pkg} not installed (this is OK if you're building from source).`);
}

function detectShell() {
  const shellPath = (process.env.SHELL || "").toLowerCase();
  if (shellPath.includes("bash")) return "bash";
  if (shellPath.includes("zsh")) return "zsh";
  if (shellPath.includes("fish")) return "fish";
  if (shellPath.includes("elvish")) return "elvish";

  // Best-effort PowerShell detection on Windows
  if (
    process.platform === "win32" &&
    ((process.env.ComSpec || "").toLowerCase().includes("powershell") ||
      process.env.PSModulePath)
  ) {
    return "powershell";
  }

  return null;
}

function completionTarget(shell) {
  const home = os.homedir();
  switch (shell) {
    case "bash":
      return {
        path: path.join(home, ".local", "share", "bash-completion", "completions", "99problems"),
        hint: 'Open a new shell, or run: source ~/.local/share/bash-completion/completions/99problems',
      };
    case "zsh":
      return {
        path: path.join(home, ".zfunc", "_99problems"),
        hint:
          "Ensure '~/.zfunc' is in fpath and compinit is enabled (then restart shell).",
      };
    case "fish":
      return {
        path: path.join(home, ".config", "fish", "completions", "99problems.fish"),
        hint: "Open a new fish shell session.",
      };
    default:
      return null;
  }
}

function installCompletions(binaryPath) {
  if (process.env.NINETY_NINE_PROBLEMS_SKIP_COMPLETIONS === "1") {
    return;
  }

  const shell = detectShell();
  if (!shell) {
    console.log(
      "[99problems] Could not detect your shell. Generate completions manually, e.g. '99problems completions bash'."
    );
    return;
  }

  const target = completionTarget(shell);
  if (!target) {
    console.log(
      `[99problems] Shell '${shell}' detected. Auto-install is not supported; generate manually with '99problems completions ${shell}'.`
    );
    return;
  }

  try {
    const script = execFileSync(binaryPath, ["completions", shell], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    fs.mkdirSync(path.dirname(target.path), { recursive: true });
    fs.writeFileSync(target.path, script, "utf8");
    console.log(`[99problems] ${shell} completions installed at ${target.path}`);
    console.log(`[99problems] ${target.hint}`);
  } catch (err) {
    console.warn(
      `[99problems] Could not install ${shell} completions automatically: ${err.message}`
    );
    console.warn(
      `[99problems] You can still generate them manually with: 99problems completions ${shell}`
    );
  }
}
