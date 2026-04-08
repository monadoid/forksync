"use strict";

const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const runtime = require("../scripts/runtime");

function getInput(name, fallback = "") {
  return process.env[`INPUT_${name.replace(/ /g, "_").replace(/-/g, "_").toUpperCase()}`] ?? fallback;
}

function asBool(value) {
  return runtime.parseBool(value);
}

function log(message) {
  process.stdout.write(`${message}\n`);
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

function installOpencodeIfNeeded() {
  if (!asBool(getInput("install-opencode", "true"))) {
    return;
  }

  if (!runtime.commandExists("opencode")) {
    log("Installing OpenCode for ForkSync action.");
    const result = spawnSync("bash", ["-lc", "curl -fsSL https://opencode.ai/install | bash"], {
      stdio: "inherit",
    });
    if (result.error) {
      throw result.error;
    }
    if (result.status !== 0) {
      throw new Error(`bash exited with status ${result.status}`);
    }
  }

  const result = spawnSync("bash", ["-lc", "opencode --version"], { stdio: "inherit" });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`bash exited with status ${result.status}`);
  }
}

async function ensureBinary() {
  const directBinary = getInput("binary-path", "").trim();
  const version = getInput("binary-version", "latest").trim() || "latest";
  const repository =
    getInput("action-repository", "").trim() ||
    process.env.GITHUB_ACTION_REPOSITORY ||
    "samfinton/forksync";

  return runtime.resolveForkSyncBinary({
    binaryPath: directBinary,
    binaryVersion: version,
    repository,
    allowBuildFallback: asBool(getInput("allow-build-fallback", "false")),
    sourceBuildRoot: process.env.GITHUB_ACTION_PATH || path.resolve(__dirname, ".."),
    cacheRoot: path.join(os.homedir(), ".cache", "forksync"),
    log,
  });
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

  const result = spawnSync(binaryPath, args, {
    stdio: "inherit",
    cwd,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${binaryPath} exited with status ${result.status}`);
  }
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
