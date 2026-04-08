#!/usr/bin/env node
"use strict";

const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const runtime = require("../scripts/runtime");

function log(message) {
  process.stdout.write(`${message}\n`);
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

function parseLauncherArgs(argv) {
  const passthrough = [];
  const options = {
    binaryPath: process.env.FORKSYNC_BINARY_PATH?.trim() || "",
    binaryVersion: process.env.FORKSYNC_BINARY_VERSION?.trim() || "latest",
    repository:
      process.env.FORKSYNC_BINARY_REPOSITORY?.trim() ||
      process.env.GITHUB_ACTION_REPOSITORY?.trim() ||
      "samfinton/forksync",
    allowBuildFallback: runtime.parseBool(process.env.FORKSYNC_ALLOW_BUILD_FALLBACK ?? "false"),
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--") {
      passthrough.push(...argv.slice(index));
      break;
    }

    if (arg === "--binary-path") {
      options.binaryPath = argv[++index] ?? "";
      continue;
    }
    if (arg.startsWith("--binary-path=")) {
      options.binaryPath = arg.slice("--binary-path=".length);
      continue;
    }

    if (arg === "--binary-version") {
      options.binaryVersion = argv[++index] ?? "latest";
      continue;
    }
    if (arg.startsWith("--binary-version=")) {
      options.binaryVersion = arg.slice("--binary-version=".length);
      continue;
    }

    if (arg === "--repository") {
      options.repository = argv[++index] ?? options.repository;
      continue;
    }
    if (arg.startsWith("--repository=")) {
      options.repository = arg.slice("--repository=".length);
      continue;
    }

    if (arg === "--allow-build-fallback") {
      options.allowBuildFallback = true;
      continue;
    }
    if (arg === "--no-allow-build-fallback") {
      options.allowBuildFallback = false;
      continue;
    }

    passthrough.push(arg);
  }

  return { options, passthrough };
}

function run(binaryPath, args) {
  const result = spawnSync(binaryPath, args, { stdio: "inherit" });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

async function main() {
  try {
    const { options, passthrough } = parseLauncherArgs(process.argv.slice(2));
    const binaryPath = await runtime.resolveForkSyncBinary({
      binaryPath: options.binaryPath,
      binaryVersion: options.binaryVersion,
      repository: options.repository,
      allowBuildFallback: options.allowBuildFallback,
      sourceBuildRoot: runtime.findCargoProjectRoot(process.cwd()),
      cacheRoot: path.join(os.homedir(), ".cache", "forksync"),
      log,
    });
    run(binaryPath, passthrough);
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
}

void main();
