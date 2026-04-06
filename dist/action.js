"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const https = require("node:https");
const { spawnSync } = require("node:child_process");

function getInput(name, fallback = "") {
  return process.env[`INPUT_${name.replace(/ /g, "_").replace(/-/g, "_").toUpperCase()}`] ?? fallback;
}

function asBool(value) {
  return ["1", "true", "yes", "on"].includes(String(value).toLowerCase());
}

function log(message) {
  process.stdout.write(`${message}\n`);
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: "inherit",
    ...options,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

function commandExists(command) {
  const checker = process.platform === "win32" ? "where" : "which";
  const result = spawnSync(checker, [command], { stdio: "ignore" });
  return result.status === 0;
}

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const request = https.get(url, (response) => {
      if (response.statusCode && response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        response.resume();
        downloadFile(response.headers.location, dest).then(resolve, reject);
        return;
      }

      if (response.statusCode !== 200) {
        response.resume();
        reject(new Error(`download failed from ${url} with status ${response.statusCode}`));
        return;
      }

      ensureDir(path.dirname(dest));
      const file = fs.createWriteStream(dest, { mode: 0o755 });
      response.pipe(file);
      file.on("finish", () => file.close(resolve));
      file.on("error", reject);
    });

    request.on("error", reject);
  });
}

function resolveTargetAsset() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "linux" && arch === "x64") {
    return { assetName: "forksync-x86_64-unknown-linux-gnu", executableName: "forksync" };
  }
  if (platform === "darwin" && arch === "arm64") {
    return { assetName: "forksync-aarch64-apple-darwin", executableName: "forksync" };
  }
  if (platform === "darwin" && arch === "x64") {
    return { assetName: "forksync-x86_64-apple-darwin", executableName: "forksync" };
  }
  if (platform === "win32" && arch === "x64") {
    return { assetName: "forksync-x86_64-pc-windows-msvc.exe", executableName: "forksync.exe" };
  }

  throw new Error(`unsupported runner platform for ForkSync binary download: ${platform}/${arch}`);
}

function releaseUrl(repo, version, assetName) {
  if (version === "latest") {
    return `https://github.com/${repo}/releases/latest/download/${assetName}`;
  }
  return `https://github.com/${repo}/releases/download/${version}/${assetName}`;
}

function cargoCommand() {
  if (commandExists("cargo")) {
    return "cargo";
  }

  const homeCargo = path.join(os.homedir(), ".cargo", "bin", "cargo");
  if (fs.existsSync(homeCargo)) {
    return homeCargo;
  }

  if (process.platform === "win32") {
    throw new Error("cargo is required for ForkSync source-build fallback on Windows");
  }

  log("Installing Rust toolchain for ForkSync source-build fallback.");
  run("bash", ["-lc", "curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal"]);

  if (fs.existsSync(homeCargo)) {
    return homeCargo;
  }

  throw new Error("cargo was not available after Rust toolchain installation");
}

function installOpencodeIfNeeded() {
  if (!asBool(getInput("install-opencode", "true"))) {
    return;
  }

  if (!commandExists("opencode")) {
    log("Installing OpenCode for ForkSync action.");
    run("bash", ["-lc", "curl -fsSL https://opencode.ai/install | bash"]);
  }

  run("bash", ["-lc", "opencode --version"]);
}

function cachedBinaryPath(version, executableName) {
  return path.join(os.homedir(), ".cache", "forksync", "bin", version, executableName);
}

async function ensureBinary() {
  ensureDir(path.join(os.homedir(), ".cache", "forksync"));
  const directBinary = getInput("binary-path", "").trim();
  if (directBinary) {
    return directBinary;
  }

  const version = getInput("binary-version", "latest").trim() || "latest";
  const repository = getInput("action-repository", "").trim() || process.env.GITHUB_ACTION_REPOSITORY || "samfinton/forksync";
  const { assetName, executableName } = resolveTargetAsset();
  const cachedPath = cachedBinaryPath(version, executableName);

  if (fs.existsSync(cachedPath)) {
    return cachedPath;
  }

  const url = releaseUrl(repository, version, assetName);
  try {
    log(`Downloading ForkSync binary from ${url}`);
    await downloadFile(url, cachedPath);
    if (process.platform !== "win32") {
      fs.chmodSync(cachedPath, 0o755);
    }
    return cachedPath;
  } catch (error) {
    if (!asBool(getInput("allow-build-fallback", "true"))) {
      throw error;
    }
    log(`Falling back to source build because binary download failed: ${error.message}`);
  }

  const actionPath = process.env.GITHUB_ACTION_PATH || path.resolve(__dirname, "..");

  const cargo = cargoCommand();
  run(cargo, ["build", "--release", "--bin", "forksync", "--locked"], { cwd: actionPath });
  const builtBinary = path.join(actionPath, "target", "release", executableName);
  if (!fs.existsSync(builtBinary)) {
    throw new Error(`expected built ForkSync binary at ${builtBinary}`);
  }
  ensureDir(path.dirname(cachedPath));
  fs.copyFileSync(builtBinary, cachedPath);
  if (process.platform !== "win32") {
    fs.chmodSync(cachedPath, 0o755);
  }
  return cachedPath;
}

function runForkSync(binaryPath) {
  const workspace = process.env.GITHUB_WORKSPACE;
  if (!workspace) {
    throw new Error("GITHUB_WORKSPACE is not set");
  }

  const workingDirectory = getInput("working-directory", ".").trim() || ".";
  const configPath = getInput("config-path", ".forksync.yml").trim() || ".forksync.yml";
  const trigger = getInput("trigger", "schedule").trim() || "schedule";
  const cwd = path.resolve(workspace, workingDirectory);

  const args = [];
  if (asBool(getInput("verbose", "false"))) {
    args.push("--verbose");
  }
  if (asBool(getInput("json-logs", "false"))) {
    args.push("--json-logs");
  }
  args.push("--config", configPath, "sync", "--trigger", trigger);

  run(binaryPath, args, { cwd });
}

async function main() {
  try {
    if (!process.env.GITHUB_ACTION_PATH) {
      process.env.GITHUB_ACTION_PATH = path.resolve(__dirname, "..");
    }
    installOpencodeIfNeeded();
    const binaryPath = await ensureBinary();
    runForkSync(binaryPath);
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
}

void main();
