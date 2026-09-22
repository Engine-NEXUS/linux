#!/usr/bin/env node
// ==============================================================================
// NEXUS — Unified Cross-Platform Developer Command
// ==============================================================================
// One entry point for Windows, macOS, and Linux.
//
//   node nexus.mjs install — install prerequisites + build + global 'nexus' command
//   node nexus.mjs setup   — install prerequisites + build (no global command)
//   node nexus.mjs build   — build frontend + Rust release binary
//   node nexus.mjs dev     — tauri dev (hot reload via Vite)
//   node nexus.mjs start   — launch the built release binary (alias: run)
//   node nexus.mjs run     — launch the built release binary
//   node nexus.mjs check   — diagnostics (tools, frontend, Rust, NLU)
//   node nexus.mjs clean   — remove build artifacts
//   node nexus.mjs worker  — deploy the Cloudflare Worker (optional)
//   node nexus.mjs help    — show this help
//
// On Unix:  ./nexus setup     (symlink/shim with shebang)
// On Win:   nexus setup       (nexus.cmd shim)
// ==============================================================================

import { execSync, spawnSync } from "node:child_process";
// Suppress the Node DEP0190 deprecation warning about shell:true with args.
// We control all arguments (no user input) — the warning is a false positive
// for our use case of launching .cmd shims like npm.cmd on Windows.
process.removeAllListeners("warning");
import { existsSync, mkdirSync, rmSync, statSync, writeFileSync, copyFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { platform, arch } from "node:os";

const __filename = fileURLToPath(import.meta.url);
const ROOT = resolve(dirname(__filename));
const IS_WIN = platform() === "win32";
const IS_MAC = platform() === "darwin";
const IS_LINUX = platform() === "linux";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const C = {
  reset: "\x1b[0m", bold: "\x1b[1m", dim: "\x1b[2m",
  red: "\x1b[31m", green: "\x1b[32m", yellow: "\x1b[33m",
  blue: "\x1b[34m", magenta: "\x1b[35m", cyan: "\x1b[36m",
};

function log(tag, msg, color = "cyan") {
  const ts = new Date().toTimeString().slice(0, 8);
  console.error(`${C.dim}[${ts}]${C.reset} ${C[color]}${tag.padEnd(5)}${C.reset} ${msg}`);
}

function ok(msg)   { log("OK",   msg, "green"); }
function info(msg) { log("==>",  msg, "cyan"); }
function warn(msg) { log("WARN", msg, "yellow"); }
function err(msg)  { log("ERR",  msg, "red"); }

function run(cmd, args, opts = {}) {
  const cwd = opts.cwd || ROOT;
  const label = opts.label || `${cmd} ${args.join(" ")}`;
  if (opts.silent !== true) info(label);
  // On Windows, npm/cargo/python are .cmd/.bat/.exe shims. Node's spawnSync
  // cannot execute .cmd/.bat files without shell:true. We resolve the full
  // path via `where`, then use shell:true only when needed (.cmd/.bat).
  // For .exe files (cargo, python, nexus.exe) we use shell:false to avoid
  // the Node DEP0190 arg-injection deprecation warning.
  let resolvedCmd = cmd;
  let needsShell = opts.shell || false;
  if (IS_WIN && !opts.shell) {
    try {
      const lines = execSync(`where ${cmd}`, { stdio: "pipe", encoding: "utf-8" }).trim().split(/\r?\n/);
      const priority = [".cmd", ".bat", ".exe"];
      const best = lines.find(l => priority.some(ext => l.toLowerCase().endsWith(ext)));
      if (best) {
        resolvedCmd = best;
        // .cmd and .bat files require shell:true to execute
        if (best.toLowerCase().endsWith(".cmd") || best.toLowerCase().endsWith(".bat")) {
          needsShell = true;
        }
      } else if (lines[0]) {
        resolvedCmd = lines[0];
      }
    } catch {
      // `where` failed — cmd may be a full path already; fall through
    }
  }
  // When the resolved command is a .cmd/.bat with spaces in the path
  // (e.g. "C:\Program Files\nodejs\npm.cmd"), spawnSync with shell:true
  // fails because cmd.exe splits on spaces. In that case, use the bare
  // command name (npm, not the full path) — the directory is already on
  // PATH, so cmd.exe will find it.
  let result;
  let finalCmd = resolvedCmd;
  if (needsShell && IS_WIN && resolvedCmd.includes(" ")) {
    // Extract just the filename (npm.cmd -> npm) — cmd.exe resolves via PATH
    finalCmd = cmd; // use the original bare name, not the full path
  }
  result = spawnSync(finalCmd, args, {
    cwd,
    stdio: opts.stdio || "inherit",
    shell: needsShell,
    env: { ...process.env, ...opts.env },
    encoding: "utf-8",
  });
  if (result.status !== 0 && !opts.allowFail) {
    err(`Command failed (exit ${result.status}): ${label}`);
    if (opts.hint) warn(opts.hint);
    process.exit(result.status ?? 1);
  }
  return result;
}

function has(cmd) {
  try {
    execSync(`${IS_WIN ? "where" : "which"} ${cmd}`, { stdio: "ignore", shell: true });
    return true;
  } catch {
    return false;
  }
}

function hasNpm() { return has("npm"); }
function hasCargo() { return has("cargo"); }
function hasPython() { return has("python") || has("python3"); }
function pythonCmd() { return has("python") ? "python" : "python3"; }

// ─── OS-specific prerequisite installation ───────────────────────────────────

function installWindows() {
  info("Installing prerequisites on Windows...");

  // Node.js
  if (!hasNpm()) {
    info("Installing Node.js via winget...");
    run("winget", ["install", "-e", "--id", "OpenJS.NodeJS", "--accept-source-agreements", "--accept-package-agreements"],
      { hint: "If winget fails, download from https://nodejs.org/" });
    // Refresh PATH for this session
    process.env.PATH = execSync('powershell -NoProfile -Command "[Environment]::GetEnvironmentVariable(\'PATH\', \'User\')"', { encoding: "utf-8" }).trim() + ";" + process.env.PATH;
  }

  // Rust
  if (!hasCargo()) {
    info("Installing Rust via rustup...");
    run("winget", ["install", "-e", "--id", "Rustlang.Rustup", "--accept-source-agreements", "--accept-package-agreements"],
      { hint: "If winget fails, run https://win.rustup.rs/x86_64" });
    process.env.PATH = process.env.USERPROFILE + "\\.cargo\\bin;" + process.env.PATH;
  }

  // Python
  if (!hasPython()) {
    info("Installing Python 3.12 via winget...");
    run("winget", ["install", "-e", "--id", "Python.Python.3.12", "--accept-source-agreements", "--accept-package-agreements"],
      { hint: "If winget fails, download from https://python.org/" });
    process.env.PATH = process.env.LOCALAPPDATA + "\\Programs\\Python\\Python312;" + process.env.PATH;
  }

  // LLVM/libclang (needed for bindgen-based crates)
  if (!process.env.LIBCLANG_PATH || !existsSync(process.env.LIBCLANG_PATH + "\\libclang.dll")) {
    const llvmCandidates = [
      "C:\\Program Files\\LLVM\\bin",
      "C:\\LLVM\\bin",
    ];
    let found = llvmCandidates.find(p => existsSync(p + "\\libclang.dll"));
    if (!found) {
      info("Installing LLVM via winget (for libclang)...");
      run("winget", ["install", "-e", "--id", "LLVM.LLVM", "--accept-source-agreements", "--accept-package-agreements"],
        { allowFail: true, hint: "If winget fails, install from https://github.com/llvm/llvm-project/releases" });
      found = "C:\\Program Files\\LLVM\\bin";
    }
    if (existsSync(found + "\\libclang.dll")) {
      process.env.LIBCLANG_PATH = found;
      ok(`LIBCLANG_PATH=${found}`);
    } else {
      warn("libclang.dll not found — Rust build may fail. Install LLVM manually.");
    }
  }

  // MSVC C++ Build Tools
  if (!has("link")) {
    warn("MSVC C++ Build Tools (link.exe) not found in PATH.");
    warn("If the Rust build fails, install via:");
    warn('  winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools"');
  }
}

function installMacOS() {
  info("Installing prerequisites on macOS...");

  // Homebrew
  if (!has("brew")) {
    info("Installing Homebrew...");
    run("/bin/bash", ["-c", 'NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'],
      { hint: "See https://brew.sh" });
  }

  // Node.js
  if (!hasNpm()) {
    info("Installing Node.js via brew...");
    run("brew", ["install", "node"], { hint: "Or download from https://nodejs.org/" });
  }

  // Rust
  if (!hasCargo()) {
    info("Installing Rust via rustup...");
    run("curl", ["--proto", "=https", "--tlsv1.2", "-sSf", "https://sh.rustup.rs", "-o", "/tmp/rustup-init.sh"],
      { stdio: "ignore" });
    run("sh", ["/tmp/rustup-init.sh", "-y"], { hint: "Or visit https://rustup.rs" });
    process.env.PATH = process.env.HOME + "/.cargo/bin:" + process.env.PATH;
  }

  // Python
  if (!hasPython()) {
    info("Installing Python 3.12 via brew...");
    run("brew", ["install", "python@3.12"], { hint: "Or download from https://python.org/" });
  }

  // Xcode Command Line Tools (needed for C/C++ compilation)
  if (!existsSync("/Library/Developer/CommandLineTools/usr/bin/clang")) {
    info("Installing Xcode Command Line Tools...");
    run("xcode-select", ["--install"], { allowFail: true, hint: "This opens a GUI installer — click Install." });
    warn("After the GUI installer finishes, re-run: nexus setup");
  }
}

function installLinux() {
  info("Installing prerequisites on Linux...");

  // Detect package manager
  let pm, pmInstall;
  if (has("apt-get")) {
    pm = "apt-get"; pmInstall = ["install", "-y"];
  } else if (has("dnf")) {
    pm = "dnf"; pmInstall = ["install", "-y"];
  } else if (has("yum")) {
    pm = "yum"; pmInstall = ["install", "-y"];
  } else if (has("pacman")) {
    pm = "pacman"; pmInstall = ["-S", "--noconfirm"];
  } else if (has("zypper")) {
    pm = "zypper"; pmInstall = ["install", "-y"];
  } else {
    err("No supported package manager found (apt/dnf/yum/pacman/zypper).");
    err("Install Rust, Node.js, Python 3.12, and the Tauri system deps manually.");
    err("See: https://tauri.app/start/prerequisites/");
    process.exit(1);
  }

  // System libraries for Tauri on Linux (webkit2gtk-4.1)
  const sysDeps = {
    "apt-get": ["libwebkit2gtk-4.1-dev", "libgtk-3-dev", "libayatana-appindicator3-dev", "librsvg2-dev", "libasound2-dev", "libssl-dev", "pkg-config"],
    "dnf":     ["webkit2gtk4.1-devel", "gtk3-devel", "libappindicator-gtk3-devel", "librsvg2-devel", "alsa-lib-devel", "openssl-devel", "pkgconf-pkg-config"],
    "yum":     ["webkit2gtk4.1-devel", "gtk3-devel", "libappindicator-gtk3-devel", "librsvg2-devel", "alsa-lib-devel", "openssl-devel", "pkgconfig"],
    "pacman":  ["webkit2gtk-4.1", "gtk3", "libappindicator-gtk3", "librsvg", "alsa-lib", "openssl", "pkgconf"],
    "zypper":  ["webkit2gtk4-devel", "gtk3-devel", "libappindicator3-devel", "librsvg2-devel", "alsa-devel", "libopenssl-devel", "pkg-config"],
  };

  info(`Installing system libraries via ${pm}...`);
  if (pm === "apt-get") run("sudo", [pm, "update"]);
  run("sudo", [pm, ...pmInstall, ...sysDeps[pm]],
    { hint: "If this fails, install the packages manually." });

  // Node.js
  if (!hasNpm()) {
    if (pm === "pacman") {
      run("sudo", [pm, ...pmInstall, "nodejs", "npm"]);
    } else {
      // Use NodeSource for a recent version
      info("Installing Node.js 20.x via NodeSource...");
      run("sudo", ["bash", "-c",
        `curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && ${pm} ${pmInstall.join(" ")} nodejs`],
        { shell: true, allowFail: true, hint: "Or download from https://nodejs.org/" });
    }
  }

  // Rust
  if (!hasCargo()) {
    info("Installing Rust via rustup...");
    run("curl", ["--proto", "=https", "--tlsv1.2", "-sSf", "https://sh.rustup.rs", "-o", "/tmp/rustup-init.sh"],
      { stdio: "ignore" });
    run("sh", ["/tmp/rustup-init.sh", "-y"], { hint: "Or visit https://rustup.rs" });
    process.env.PATH = process.env.HOME + "/.cargo/bin:" + process.env.PATH;
  }

  // Python
  if (!hasPython()) {
    const pyPkg = pm === "pacman" ? "python" : "python3";
    run("sudo", [pm, ...pmInstall, pyPkg, "python3-pip"], { hint: "Or download from https://python.org/" });
  }
}

// ─── Commands ────────────────────────────────────────────────────────────────

/// Verify faster-whisper Python package is installed.
/// The STT server (server/stt_server.py) uses faster-whisper tiny.en
/// which auto-downloads the model from HuggingFace on first transcription.
function checkFasterWhisper() {
  const result = spawnSync(IS_WIN ? "python" : "python3", ["-c", "import faster_whisper; print('ok')"], {
    stdio: "pipe",
    encoding: "utf-8",
    shell: IS_WIN,
  });

  if (result.status === 0 && result.stdout?.trim() === "ok") {
    ok("faster-whisper installed (STT server ready)");
  } else {
    warn("faster-whisper not installed — installing now...");
    try {
      execSync(`${IS_WIN ? "python" : "python3"} -m pip install faster-whisper fastapi uvicorn python-multipart`, {
        stdio: "inherit",
        encoding: "utf-8",
        shell: IS_WIN,
      });
      ok("faster-whisper installed successfully");
    } catch {
      warn("Failed to install faster-whisper — STT will not work. Run: pip install faster-whisper fastapi uvicorn python-multipart");
    }
  }
}

function cmdSetup() {
  console.log(`\n${C.bold}${C.cyan}╔═══════════════════════════════════════════════════════════╗${C.reset}`);
  console.log(`${C.bold}${C.cyan}║         NEXUS — Cross-Platform Setup & Build               ║${C.reset}`);
  console.log(`${C.bold}${C.cyan}╚═══════════════════════════════════════════════════════════╝${C.reset}\n`);

  // 1. Install OS prerequisites
  if (IS_WIN) installWindows();
  else if (IS_MAC) installMacOS();
  else if (IS_LINUX) installLinux();
  else { err(`Unsupported platform: ${platform()}`); process.exit(1); }

  // 2. Verify tools
  info("Verifying installed tools...");
  const checks = [
    ["npm", hasNpm, "Node.js"],
    ["cargo", hasCargo, "Rust"],
    ["python", hasPython, "Python"],
  ];
  let allOk = true;
  for (const [cmd, check, name] of checks) {
    if (check()) { ok(`${name} found`); }
    else { err(`${name} NOT found — install it manually and re-run`); allOk = false; }
  }
  if (!allOk) {
    err("Prerequisites missing. Please install them and re-run: nexus setup");
    process.exit(1);
  }

  // 3. Install frontend dependencies
  info("Installing frontend dependencies...");
  run("npm", ["--prefix", "frontend", "install"]);

  // 4. Install Worker dependencies (optional, for self-deploy)
  if (existsSync(join(ROOT, "server", "worker", "package.json"))) {
    info("Installing Cloudflare Worker dependencies...");
    run("npm", ["--prefix", "server/worker", "install"], { allowFail: true });
  }

  // 5. Install NLU Python dependencies
  info("Installing NLU server Python dependencies...");
  const nluReq = join(ROOT, "server", "nlu", "requirements.txt");
  if (existsSync(nluReq)) {
    run(pythonCmd(), ["-m", "pip", "install", "-r", nluReq],
      { allowFail: true, hint: "If pip fails, create a venv: python -m venv .venv && activate it" });
  }

  // 6. Verify NLU model is present (either in server/nlu/model/ or resources/)
  const nluModelLocal = join(ROOT, "server", "nlu", "model", "nexus_nlu.onnx");
  const nluModelResources = join(ROOT, "src-tauri", "resources", "server", "nlu", "model", "nexus_nlu.onnx");
  if (existsSync(nluModelLocal)) {
    ok("NLU model found (local training output)");
  } else if (existsSync(nluModelResources)) {
    ok("NLU model found (committed in resources)");
  } else {
    warn("NLU model not found — NLU server will fail. Run: cd server/nlu && python train.py && python export_onnx.py");
  }

  // 7. Verify faster-whisper is installed (STT server dependency)
  checkFasterWhisper();

  // 8. Build
  info("Building NEXUS...");
  cmdBuild();

  console.log(`\n${C.bold}${C.green}═════════════════════════════════════════════════════════════${C.reset}`);
  ok("NEXUS is set up and ready!");
  console.log(`${C.green}  • Start: ${IS_WIN ? "nexus start" : "./nexus start"}${C.reset}`);
  console.log(`${C.green}  • Dev:   ${IS_WIN ? "nexus dev" : "./nexus dev"}${C.reset}`);
  console.log(`${C.green}  • Hotkey: Ctrl+Shift+Space  •  Wake word: "NEXUS"${C.reset}`);
  console.log(`${C.bold}${C.green}═════════════════════════════════════════════════════════════${C.reset}\n`);
}

// ─── Global command installation ─────────────────────────────────────────────
// Makes `nexus` available from any directory (not just the repo root).

function cmdInstallGlobal() {
  info("Installing global 'nexus' command...");

  if (IS_WIN) {
    // Windows: create nexus.cmd in a directory that's already on PATH.
    // Prefer %USERPROFILE%\.local\bin (created if needed), fall back to
    // %LOCALAPPDATA%\Microsoft\WindowsApps (always on PATH on Win10+).
    const localBin = join(process.env.USERPROFILE || "", ".local", "bin");
    if (!existsSync(localBin)) mkdirSync(localBin, { recursive: true });

    const shimPath = join(localBin, "nexus.cmd");
    const shimContent = `@echo off\r\nnode "${join(ROOT, "nexus.mjs")}" %*\r\n`;
    writeFileSync(shimPath, shimContent);
    ok(`Created ${shimPath}`);

    // Add to user PATH if not already there
    const pathVar = execSync(
      'powershell -NoProfile -Command "[Environment]::GetEnvironmentVariable(\'PATH\',\'User\')"',
      { encoding: "utf-8" }
    ).trim();
    if (!pathVar.split(";").includes(localBin)) {
      info(`Adding ${localBin} to user PATH...`);
      execSync(
        `powershell -NoProfile -Command "[Environment]::SetEnvironmentVariable('PATH','${pathVar};${localBin}','User')"`,
        { stdio: "ignore" }
      );
      ok(`Added ${localBin} to user PATH (restart your terminal to pick it up)`);
    } else {
      ok(`${localBin} already on user PATH`);
    }
    warn("Restart your terminal for the global 'nexus' command to take effect.");

  } else {
    // Unix (macOS + Linux): symlink to /usr/local/bin/nexus (needs sudo)
    // Fall back to ~/.local/bin/nexus if no sudo.
    const target = join(ROOT, "nexus.mjs");
    const globalPaths = ["/usr/local/bin/nexus", "/opt/homebrew/bin/nexus"];
    let installed = false;

    for (const binPath of globalPaths) {
      if (existsSync(dirname(binPath)) || has("sudo")) {
        info(`Symlinking ${binPath} -> ${target}...`);
        const args = ["ln", "-sf", target, binPath];
        if (!has("sudo")) {
          run("ln", ["-sf", target, binPath], { allowFail: true });
        } else {
          run("sudo", args, { allowFail: true });
        }
        if (existsSync(binPath)) {
          ok(`Created ${binPath}`);
          installed = true;
          break;
        }
      }
    }

    if (!installed) {
      // Fall back to ~/.local/bin
      const localBin = join(process.env.HOME || "", ".local", "bin");
      if (!existsSync(localBin)) mkdirSync(localBin, { recursive: true });
      const shimPath = join(localBin, "nexus");
      run("ln", ["-sf", target, shimPath]);
      ok(`Created ${shimPath}`);
      warn(`Add ${localBin} to your PATH if not already there:`);
      warn(`  echo 'export PATH="${localBin}:$PATH"' >> ~/.bashrc  (or ~/.zshrc)`);
    }
  }

  ok("Global 'nexus' command installed. You can now run 'nexus' from any directory.");
}

function cmdInstall() {
  // Full install: setup (prerequisites + build) + global command
  cmdSetup();
  cmdInstallGlobal();

  console.log(`\n${C.bold}${C.green}═════════════════════════════════════════════════════════════${C.reset}`);
  ok("NEXUS installed! The 'nexus' command is now available globally.");
  console.log(`${C.green}  • Start:    nexus start${C.reset}`);
  console.log(`${C.green}  • Dev:      nexus dev${C.reset}`);
  console.log(`${C.green}  • Rebuild:  nexus build${C.reset}`);
  console.log(`${C.green}  • Diagnostics: nexus check${C.reset}`);
  console.log(`${C.green}  • Hotkey: Ctrl+Shift+Space  •  Wake word: "NEXUS"${C.reset}`);
  console.log(`${C.bold}${C.green}═════════════════════════════════════════════════════════════${C.reset}\n`);
}

// Kill any running NEXUS process so cargo can replace the binary.
// On Windows, cargo build fails with "Access is denied (os error 5)" if
// nexus.exe is still running. On Unix, the linker may also fail.
function killRunningNexus() {
  if (IS_WIN) {
    try {
      execSync("taskkill /F /IM nexus.exe", { stdio: "pipe", encoding: "utf-8" });
      warn("Killed running nexus.exe — needed to rebuild the binary");
      // Give Windows time to release the file lock
      setTimeout(() => {}, 1000);
    } catch {
      // No nexus.exe running — good
    }
  } else {
    try {
      execSync("pkill -f nexus", { stdio: "pipe", encoding: "utf-8" });
      warn("Killed running nexus process");
    } catch {
      // No nexus process running — good
    }
  }
}

/// Sync the NLU model from server/nlu/model/ to src-tauri/resources/server/nlu/model/.
/// This ensures the installer always bundles the latest trained ONNX model,
/// labels.json, and tokenizer. Called before every build.
/// If the source model doesn't exist (fresh clone, no training done yet),
/// the existing resources copy is kept (it's committed to git).
function syncNluModel() {
  const srcDir = join(ROOT, "server", "nlu", "model");
  const dstDir = join(ROOT, "src-tauri", "resources", "server", "nlu", "model");

  // Files to sync (only if they exist in source)
  const files = [
    "nexus_nlu.onnx",
    "nexus_nlu.onnx.data",
    "labels.json",
  ];

  let synced = 0;
  for (const file of files) {
    const src = join(srcDir, file);
    const dst = join(dstDir, file);
    if (existsSync(src)) {
      copyFileSync(src, dst);
      synced++;
    }
  }

  // Sync tokenizer dir
  const srcTok = join(srcDir, "tokenizer");
  const dstTok = join(dstDir, "tokenizer");
  if (existsSync(srcTok)) {
    mkdirSync(dstTok, { recursive: true });
    for (const f of readdirSync(srcTok)) {
      copyFileSync(join(srcTok, f), join(dstTok, f));
      synced++;
    }
  }

  if (synced > 0) {
    ok(`NLU model synced to resources (${synced} file(s))`);
  } else {
    info("NLU model: using existing resources copy (no local training output found)");
  }
}

function cmdBuild() {
  // Kill any running instance first — cargo can't replace a running binary
  killRunningNexus();

  // Sync the NLU model from server/nlu/model/ to src-tauri/resources/server/nlu/model/
  // This ensures the installer always bundles the latest trained model.
  // The NLU server reads from resources/ in production (no server/ dir in installed builds).
  syncNluModel();

  info("Building frontend (Vite)...");
  run("npm", ["--prefix", "frontend", "install"], { allowFail: true, stdio: "ignore" });
  run("npm", ["--prefix", "frontend", "run", "build"]);

  info("Building Rust release binary (custom-protocol + admin-brain)...");
  const cargoEnv = {};
  if (IS_WIN && process.env.LIBCLANG_PATH) cargoEnv.LIBCLANG_PATH = process.env.LIBCLANG_PATH;
  // admin-brain: enables the Qwen brain (admin-only, runtime-gated by admin.json).
  // The brain code is compiled in but does nothing unless admin.json has is_admin=true.
  // Normal users never have admin.json, so the brain is dormant in their builds.
  run("cargo", ["build", "--release", "--features", "custom-protocol,admin-brain"],
    { cwd: join(ROOT, "src-tauri"), env: cargoEnv,
      hint: "Make sure LIBCLANG_PATH is set (Windows) or LLVM is installed" });

  // Report binary location
  const ext = IS_WIN ? ".exe" : "";
  const binPath = join(ROOT, "src-tauri", "target", "release", `nexus${ext}`);
  if (existsSync(binPath)) {
    const sizeMB = (statSync(binPath).size / 1024 / 1024).toFixed(1);
    ok(`Binary built: ${binPath} (${sizeMB} MB)`);
  }
}

function cmdDev() {
  info("Starting Tauri dev mode (hot reload via Vite)...");
  // tauri dev runs the Vite dev server + the Rust app with devUrl
  // We need the tauri CLI — check for cargo-tauri or npx tauri
  const tauriCli = has("cargo-tauri") ? ["cargo", "tauri", "dev"]
    : ["npx", "--prefix", join(ROOT, "frontend"), "tauri", "dev"];
  run(tauriCli[0], tauriCli.slice(1), { cwd: join(ROOT, "src-tauri") });
}

function cmdRun() {
  const ext = IS_WIN ? ".exe" : "";
  const binPath = join(ROOT, "src-tauri", "target", "release", `nexus${ext}`);
  if (!existsSync(binPath)) {
    err("Release binary not found. Run: nexus build");
    process.exit(1);
  }

  if (IS_WIN) {
    // Windows: use run.ps1 for the unified color-coded console experience.
    // It kills old instances, clears logs, launches nexus.exe, and tails
    // all logs (Rust wake-word, audio, frontend CDP) in one stream.
    const runScript = join(ROOT, "scripts", "run.ps1");
    if (existsSync(runScript)) {
      info("Launching NEXUS via unified console (run.ps1)...");
      // Pass through any extra args (e.g. -Build, -Debug)
      const extraArgs = process.argv.slice(3).filter(a => !a.startsWith("-"));
      const psArgs = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", runScript];
      // Check for --debug or -Debug flag
      if (process.argv.includes("--debug") || process.argv.includes("-Debug")) {
        psArgs.push("-Debug");
      }
      if (process.argv.includes("--build") || process.argv.includes("-Build")) {
        psArgs.push("-Build");
      }
      run("pwsh", psArgs, { shell: false });
      return;
    }
    // Fall back to direct launch if run.ps1 doesn't exist
    info(`Launching NEXUS: ${binPath}`);
    run(binPath, [], { stdio: "ignore" });
  } else {
    // Unix: launch directly (no run.ps1 equivalent)
    info(`Launching NEXUS: ${binPath}`);
    run(binPath, [], { stdio: "ignore" });
  }
}

function cmdCheck() {
  console.log(`\n${C.bold}${C.cyan}NEXUS Diagnostics${C.reset}\n`);
  let pass = 0, fail = 0;

  function check(name, fn) {
    try {
      const result = fn();
      if (result) { ok(name); pass++; }
      else { err(name); fail++; }
    } catch (e) {
      err(`${name}: ${e.message}`); fail++;
    }
  }

  // Tools
  check("Node.js / npm", () => hasNpm());
  check("Rust / cargo", () => hasCargo());
  check("Python", () => hasPython());

  // Frontend
  check("frontend/node_modules", () => existsSync(join(ROOT, "frontend", "node_modules")));
  check("frontend/dist", () => existsSync(join(ROOT, "frontend", "dist")));

  // Rust
  check("Rust release binary", () => {
    const ext = IS_WIN ? ".exe" : "";
    return existsSync(join(ROOT, "src-tauri", "target", "release", `nexus${ext}`));
  });

  // NLU — check either local training output or committed resources copy
  const nluModelLocal = join(ROOT, "server", "nlu", "model", "nexus_nlu.onnx");
  const nluModelResources = join(ROOT, "src-tauri", "resources", "server", "nlu", "model", "nexus_nlu.onnx");
  check("NLU model present", () => existsSync(nluModelLocal) || existsSync(nluModelResources));
  check("NLU requirements.txt", () => existsSync(join(ROOT, "server", "nlu", "requirements.txt")));

  // Worker
  check("Worker package.json", () => existsSync(join(ROOT, "server", "worker", "package.json")));

  // Wake word model
  check("Wake word ONNX model", () => existsSync(join(ROOT, "src-tauri", "resources", "oww", "nexus.onnx")));

  console.log(`\n${C.bold}${pass > 0 && fail === 0 ? C.green : C.yellow}${pass} passed, ${fail} failed${C.reset}\n`);
  if (fail > 0) process.exit(1);
}

function cmdClean() {
  info("Cleaning build artifacts...");
  const targets = [
    join(ROOT, "frontend", "dist"),
    join(ROOT, "src-tauri", "target"),
    join(ROOT, "target"),
  ];
  for (const t of targets) {
    if (existsSync(t)) {
      rmSync(t, { recursive: true, force: true });
      ok(`Removed ${t}`);
    }
  }
  ok("Clean complete.");
}

function cmdWorker() {
  const workerDir = join(ROOT, "server", "worker");
  if (!existsSync(join(workerDir, "package.json"))) {
    err("Worker directory not found: server/worker/");
    process.exit(1);
  }
  info("Installing Worker dependencies...");
  run("npm", ["install"], { cwd: workerDir });

  if (!has("wrangler") && !existsSync(join(workerDir, "node_modules", ".bin", "wrangler"))) {
    err("wrangler not found. Run: npm install wrangler --save-dev (in server/worker)");
    process.exit(1);
  }

  info("Deploying Cloudflare Worker...");
  info("Make sure you have set up wrangler.toml + D1 database + OAuth secrets.");
  info("See: server/worker/README.md");
  run("npx", ["wrangler", "deploy"], { cwd: workerDir });
}

/// Train or retrain the BERT-Mini NLU model.
/// Runs the full pipeline: clean dataset → generate examples → merge →
/// train → export ONNX → sync to resources.
/// Contributors just run: nexus train
function cmdTrain() {
  const trainScript = join(ROOT, "server", "nlu", "train_all.py");
  if (!existsSync(trainScript)) {
    err("Training script not found: " + trainScript);
    err("Ensure you have the latest code: git pull");
    process.exit(1);
  }

  // Check Python is available
  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }

  // Check training dependencies (torch, transformers)
  info("Checking training dependencies (PyTorch, Transformers)...");
  const depCheck = spawnSync(py, ["-c", "import torch; import transformers; print('ok')"], {
    encoding: "utf-8",
  });
  if (depCheck.stdout?.trim() !== "ok") {
    info("Installing training dependencies...");
    const reqPath = join(ROOT, "server", "nlu", "requirements-train.txt");
    if (existsSync(reqPath)) {
      run(py, ["-m", "pip", "install", "-r", reqPath], {
        allowFail: true,
        hint: "If pip fails, create a venv: python -m venv .venv && activate it",
      });
    } else {
      warn("requirements-train.txt not found — install manually: pip install torch transformers onnx");
    }
  } else {
    ok("Training dependencies OK");
  }

  // Run the unified training pipeline
  info("Starting NLU training pipeline...");
  const args = [trainScript];
  // Pass through --clean-only or --skip-train if provided
  const extraArgs = process.argv.slice(3);
  args.push(...extraArgs);

  run(py, args, { cwd: join(ROOT, "server", "nlu") });

  console.log(`\n${C.bold}${C.green}═════════════════════════════════════════════════════════════${C.reset}`);
  ok("NLU training complete!");
  console.log(`${C.green}  The new model is synced to src-tauri/resources/.${C.reset}`);
  console.log(`${C.green}  Run '${IS_WIN ? "nexus" : "./nexus"} build' to bundle it into the installer.${C.reset}`);
  console.log(`${C.bold}${C.green}═════════════════════════════════════════════════════════════${C.reset}\n`);
}

function cmdAudit() {
  const auditScript = join(ROOT, "server", "nlu", "audit_nlu.py");
  if (!existsSync(auditScript)) {
    err("NLU audit script not found: " + auditScript);
    process.exit(1);
  }
  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }
  const extraArgs = process.argv.slice(3);
  info("Auditing NLU dataset and ONNX model...");
  run(py, [auditScript, ...extraArgs], { cwd: ROOT });
}

/// Collect real voice samples for NLU training.
/// Prompts you with phrases to speak, records your voice, transcribes via STT,
/// and saves the real transcripts as training data for BERT-Mini.
/// Contributors run: nexus collect
function cmdCollect() {
  const collectScript = join(ROOT, "scripts", "collect_nlu_samples.py");
  if (!existsSync(collectScript)) {
    err("Collection script not found: " + collectScript);
    err("Ensure you have the latest code: git pull");
    process.exit(1);
  }

  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }

  // Check audio recording deps
  info("Checking audio recording dependencies (sounddevice, numpy, scipy)...");
  const depCheck = spawnSync(py, ["-c", "import sounddevice; import numpy; import scipy; print('ok')"], {
    encoding: "utf-8",
  });
  if (depCheck.stdout?.trim() !== "ok") {
    info("Installing audio recording dependencies...");
    run(py, ["-m", "pip", "install", "sounddevice", "numpy", "scipy"], {
      allowFail: true,
      hint: "If pip fails, create a venv: python -m venv .venv && activate it",
    });
  } else {
    ok("Audio recording dependencies OK");
  }

  // Pass through all extra args to the Python script
  const extraArgs = process.argv.slice(3);
  info("Starting voice sample collector...");
  info("Make sure NEXUS is running (for STT) or GROQ_API_KEY is set.");
  run(py, [collectScript, ...extraArgs], { cwd: ROOT });
}

/// Verify every registered MCP server end to end (read-only, no sends).
/// Registry check + reachability probe + one read-only tool call +
/// one expected failure per server. Exit 0 only if all are reachable.
/// Contributors run: nexus mcp check
function cmdMcpCheck() {
  const checkScript = join(ROOT, "scripts", "mcp_check.py");
  if (!existsSync(checkScript)) {
    err("MCP check script not found: " + checkScript);
    process.exit(1);
  }
  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }
  run(py, [checkScript], { cwd: ROOT });
}

/// Inspect training dataset category coverage, voice sample health, and targeted recommendations.
/// Contributors run: nexus stats
function cmdStats() {
  const statsScript = join(ROOT, "scripts", "nlu_stats.py");
  if (!existsSync(statsScript)) {
    err("NLU stats script not found: " + statsScript);
    process.exit(1);
  }
  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }
  const extraArgs = process.argv.slice(3);
  run(py, [statsScript, ...extraArgs], { cwd: ROOT });
}

/// Record wake word audio samples or view wake dataset statistics.
/// Examples:
///   nexus wake record 300
///   nexus wake record positive 300
///   nexus wake record negative 100
///   nexus wake stats
function cmdWake() {
  const wakeScript = join(ROOT, "scripts", "record_wake_samples.py");
  if (!existsSync(wakeScript)) {
    err("Wake recorder script not found: " + wakeScript);
    process.exit(1);
  }

  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }

  // Check audio recording dependencies
  info("Checking wake word recording dependencies (sounddevice, numpy, scipy)...");
  const depCheck = spawnSync(py, ["-c", "import sounddevice; import numpy; import scipy; print('ok')"], {
    encoding: "utf-8",
  });
  if (depCheck.stdout?.trim() !== "ok") {
    info("Installing audio recording dependencies...");
    run(py, ["-m", "pip", "install", "sounddevice", "numpy", "scipy"], {
      allowFail: true,
      hint: "If pip fails, create a venv: python -m venv .venv && activate it",
    });
  } else {
    ok("Audio recording dependencies OK");
  }

  const sub = process.argv[3]?.toLowerCase();
  let args = [];

  if (sub === "probe" || sub === "calibrate") {
    const probeScript = join(ROOT, "scripts", "probe_microphone.py");
    if (!existsSync(probeScript)) {
      err("Probe script not found: " + probeScript);
      process.exit(1);
    }
    info("Starting microphone hardware acoustic prober...");
    run(py, [probeScript], { cwd: ROOT });
    return;
  }

  if (sub === "test" || sub === "live" || sub === "benchmark") {
    const testScript = join(ROOT, "scripts", "test_wake_live.py");
    if (!existsSync(testScript)) {
      err("Test script not found: " + testScript);
      process.exit(1);
    }
    const testArgs = process.argv.slice(4);
    if (sub === "benchmark" && !testArgs.includes("--batch")) {
      testArgs.push("--batch");
    }
    info("Starting wake word live tester / benchmark...");
    run(py, [testScript, ...testArgs], { cwd: ROOT });
    return;
  }

  if (!sub || sub === "record") {
    // Default to positive 300 if no extra arg, or check if next arg is a number/mode
    const nextArg = process.argv[4];
    const thirdArg = process.argv[5];
    if (!nextArg) {
      args = ["positive", "300"];
    } else if (!isNaN(Number(nextArg))) {
      args = ["positive", nextArg];
    } else if (["positive", "negative", "free", "background"].includes(nextArg.toLowerCase())) {
      args = [nextArg.toLowerCase(), thirdArg || "300"];
    } else {
      args = [nextArg, thirdArg || "300"];
    }
  } else if (["positive", "negative", "free", "background", "stats"].includes(sub)) {
    args = process.argv.slice(3);
  } else {
    err(`Unknown wake sub-command: ${sub}`);
    info("Usage:");
    info("  nexus wake probe                 (probe microphone hardware & calibrate acoustic profile)");
    info("  nexus wake test                  (real-time microphone listening test)");
    info("  nexus wake test --batch          (benchmark model against all dataset WAVs)");
    info("  nexus wake record 300            (record 300 positive wake words)");
    info("  nexus wake record negative 100   (record 100 negative soundalikes)");
    info("  nexus wake stats                 (show sample counts & size)");
    process.exit(1);
  }

  info(`Starting wake recorder: ${args.join(" ")}...`);
  run(py, [wakeScript, ...args], { cwd: ROOT });
}

function cmdData() {
  const py = pythonCmd();
  if (!py) {
    err("Python not found. Install Python 3.12+ and add it to PATH.");
    process.exit(1);
  }

  const area = process.argv[3];
  const action = process.argv[4] || "validate";
  if (area === "nlu") {
    if (action === "import-clinc") {
      const importer = join(ROOT, "server", "nlu", "import_clinc150.py");
      run(py, [importer, ...process.argv.slice(5)], { cwd: ROOT });
      return;
    }
    const script = join(ROOT, "server", "nlu", "data_foundation.py");
    const allowed = new Set(["validate", "freeze-evaluation"]);
    if (!allowed.has(action)) {
      err(`Unknown NLU data action: ${action}`);
      process.exit(1);
    }
    run(py, [script, action, ...process.argv.slice(5)], { cwd: ROOT });
    return;
  }
  if (area === "wake") {
    const script = join(ROOT, "scripts", "wake_data_foundation.py");
    const mappedAction = action === "fingerprint" ? "fingerprint-models" : action;
    const allowed = new Set(["validate", "fingerprint-models"]);
    if (!allowed.has(mappedAction)) {
      err(`Unknown wake data action: ${action}`);
      process.exit(1);
    }
    run(py, [script, mappedAction, ...process.argv.slice(5)], { cwd: ROOT });
    return;
  }
  err("Usage: nexus data <nlu|wake> <validate|freeze-evaluation|import-clinc|fingerprint>");
  process.exit(1);
}

function cmdHelp() {
  console.log(`
${C.bold}NEXUS — Unified Cross-Platform Developer Command${C.reset}

${C.cyan}Usage:${C.reset}
  ${IS_WIN ? "nexus" : "./nexus"} <command> [options]

${C.cyan}Commands:${C.reset}
  ${C.green}install${C.reset}  Install prerequisites + build + global 'nexus' command (first time)
  ${C.green}setup${C.reset}    Install prerequisites + build (no global command)
  ${C.green}build${C.reset}    Build frontend + Rust release binary (custom-protocol)
  ${C.green}dev${C.reset}      Start Tauri dev mode (hot reload via Vite dev server)
  ${C.green}start${C.reset}    Launch the built release binary (alias for 'run')
  ${C.green}run${C.reset}      Launch the built release binary
  ${C.green}check${C.reset}    Run diagnostics (tools, frontend, Rust, NLU, Worker)
  ${C.green}clean${C.reset}    Remove build artifacts (target/, dist/)
  ${C.green}worker${C.reset}   Deploy the Cloudflare Worker (optional, self-host backend)
  ${C.green}train${C.reset}    Retrain the BERT-Mini NLU model (clean → generate → train → export)
  ${C.green}audit${C.reset}    Audit NLU data/model quality, coverage, leakage, slots, and OOS behavior
  ${C.green}collect${C.reset}  Collect real voice samples for NLU training (speak phrases → transcribe → save)
  ${C.green}wake${C.reset}     Record wake word audio samples for wake model training (nexus wake record 300)
  ${C.green}stats${C.reset}    Inspect dataset category coverage, voice health, and training recommendations
  ${C.green}data${C.reset}     Validate NLU provenance/evaluation locks and wake data/model manifests
  ${C.green}mcp check${C.reset} Verify every registered MCP server end to end (read-only, no sends)
  ${C.green}help${C.reset}     Show this help

${C.cyan}Examples:${C.reset}
  ${IS_WIN ? "nexus" : "./nexus"} install     # first time: install everything + global command
  ${IS_WIN ? "nexus" : "./nexus"} start       # launch the built app
  ${IS_WIN ? "nexus" : "./nexus"} dev         # develop with hot reload
  ${IS_WIN ? "nexus" : "./nexus"} build       # rebuild after changes
  ${IS_WIN ? "nexus" : "./nexus"} train       # retrain the NLU model with new data
  ${IS_WIN ? "nexus" : "./nexus"} stats       # inspect dataset coverage & weakest categories
  ${IS_WIN ? "nexus" : "./nexus"} audit       # audit the current NLU dataset and ONNX model
  ${IS_WIN ? "nexus" : "./nexus"} collect     # speak phrases → save real voice samples for training
  ${IS_WIN ? "nexus" : "./nexus"} wake record 300 # record 300 real wake word samples ("NEXUS")
  ${IS_WIN ? "nexus" : "./nexus"} wake stats  # inspect wake word audio dataset statistics
  ${IS_WIN ? "nexus" : "./nexus"} data nlu validate
  ${IS_WIN ? "nexus" : "./nexus"} data wake validate

${C.cyan}Environment:${C.reset}
  NEXUS_SERVER_URL   Cloudflare Worker URL (default: baked at build time)
  LIBCLANG_PATH      Path to LLVM bin (Windows, for bindgen)
  NEXUS_STT_PORT     STT port override (default: 39217)

${C.cyan}Notes:${C.reset}
  • STT uses faster-whisper tiny.en (lazy-started Python sidecar on port 39217)
  • NLU server is lazy-started Python (BERT-Mini, model committed in repo)
  • TTS uses Fish Audio (set API key in Settings) with Web Speech fallback
  • Wake word uses openWakeWord (ONNX, pure Rust, model in repo)
`);
}

// ─── Main ────────────────────────────────────────────────────────────────────

const command = process.argv[2] || "help";

switch (command) {
  case "install": cmdInstall(); break;
  case "setup":   cmdSetup(); break;
  case "build":   cmdBuild(); break;
  case "dev":     cmdDev(); break;
  case "start":
  case "run":     cmdRun(); break;
  case "check":   cmdCheck(); break;
  case "clean":   cmdClean(); break;
  case "worker":  cmdWorker(); break;
  case "train":   cmdTrain(); break;
  case "audit":   cmdAudit(); break;
  case "collect": cmdCollect(); break;
  case "wake":    cmdWake(); break;
  case "stats":
  case "status":
  case "coverage":
    cmdStats(); break;
  case "mcp":
    if (process.argv[3] === "check") { cmdMcpCheck(); break; }
    err("Usage: nexus mcp check");
    process.exit(1);
  case "data":    cmdData(); break;
  case "help":
  case "--help":
  case "-h":
    cmdHelp(); break;
  default:
    err(`Unknown command: ${command}`);
    cmdHelp();
    process.exit(1);
}
