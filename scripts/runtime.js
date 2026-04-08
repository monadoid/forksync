"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const https = require("node:https");
const { spawnSync } = require("node:child_process");

function parseBool(value) {
  return ["1", "true", "yes", "on"].includes(String(value).toLowerCase());
}

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function commandExists(command) {
  const checker = process.platform === "win32" ? "where" : "which";
  const result = spawnSync(checker, [command], { stdio: "ignore" });
  return result.status === 0;
}

function requestText(url, headers = {}) {
  return new Promise((resolve, reject) => {
    const request = https.get(
      url,
      {
        headers: {
          "User-Agent": "ForkSync",
          Accept: "application/vnd.github+json",
          ...headers,
        },
      },
      (response) => {
        if (
          response.statusCode &&
          response.statusCode >= 300 &&
          response.statusCode < 400 &&
          response.headers.location
        ) {
          response.resume();
          requestText(response.headers.location, headers).then(resolve, reject);
          return;
        }

        let body = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => {
          body += chunk;
        });
        response.on("end", () => {
          if (response.statusCode !== 200) {
            reject(new Error(`request to ${url} failed with status ${response.statusCode}: ${body.trim()}`));
            return;
          }
          resolve(body);
        });
      },
    );

    request.on("error", reject);
  });
}

function requestJson(url) {
  return requestText(url).then((text) => JSON.parse(text));
}

function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const request = https.get(
      url,
      {
        headers: {
          "User-Agent": "ForkSync",
        },
      },
      (response) => {
        if (
          response.statusCode &&
          response.statusCode >= 300 &&
          response.statusCode < 400 &&
          response.headers.location
        ) {
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
      },
    );

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

  throw new Error(`unsupported platform for ForkSync binary download: ${platform}/${arch}`);
}

function releaseUrl(repository, version, assetName) {
  return `https://github.com/${repository}/releases/download/${version}/${assetName}`;
}

function releaseApiLatestUrl(repository) {
  return `https://api.github.com/repos/${repository}/releases/latest`;
}

async function resolveLatestReleaseTag(repository) {
  const response = await requestJson(releaseApiLatestUrl(repository));
  if (!response.tag_name) {
    throw new Error(`latest release lookup for ${repository} did not return a tag`);
  }
  return response.tag_name;
}

function cachedBinaryPath(cacheRoot, version, executableName) {
  return path.join(cacheRoot, "bin", version, executableName);
}

function checksumPath(binaryPath) {
  return `${binaryPath}.sha256`;
}

function sha256File(filePath) {
  const content = fs.readFileSync(filePath);
  return crypto.createHash("sha256").update(content).digest("hex");
}

function verifyChecksumFile(checksumFilePath, binaryPath) {
  const expected = fs
    .readFileSync(checksumFilePath, "utf8")
    .trim()
    .split(/\s+/)[0];
  if (!expected) {
    throw new Error(`checksum file ${checksumFilePath} did not contain a digest`);
  }
  const actual = sha256File(binaryPath);
  if (expected !== actual) {
    throw new Error(
      `checksum mismatch for ${binaryPath}: expected ${expected}, found ${actual}`,
    );
  }
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

  throw new Error("cargo was not available on PATH");
}

function findCargoProjectRoot(startDir) {
  let current = path.resolve(startDir);
  while (true) {
    if (fs.existsSync(path.join(current, "Cargo.toml"))) {
      return current;
    }
    const parent = path.dirname(current);
    if (parent === current) {
      return startDir;
    }
    current = parent;
  }
}

function buildReleaseBinary({ buildRoot, executableName, cachedPath }) {
  const cargo = cargoCommand();
  const result = spawnSync(cargo, ["build", "--release", "--bin", "forksync", "--locked"], {
    cwd: buildRoot,
    stdio: "inherit",
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${cargo} exited with status ${result.status}`);
  }

  const builtBinary = path.join(buildRoot, "target", "release", executableName);
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

async function resolveForkSyncBinary({
  binaryPath,
  binaryVersion = "latest",
  repository = "monadoid/forksync",
  allowBuildFallback = false,
  sourceBuildRoot,
  cacheRoot = path.join(os.homedir(), ".cache", "forksync"),
  log = () => {},
}) {
  if (binaryPath) {
    return binaryPath;
  }

  ensureDir(cacheRoot);
  const { assetName, executableName } = resolveTargetAsset();
  const resolvedVersion =
    binaryVersion === "latest" ? await resolveLatestReleaseTag(repository) : binaryVersion;
  const cachedPath = cachedBinaryPath(cacheRoot, resolvedVersion, executableName);
  if (fs.existsSync(cachedPath)) {
    return cachedPath;
  }

  const releaseBinaryUrl = releaseUrl(repository, resolvedVersion, assetName);
  const checksumUrl = releaseUrl(repository, resolvedVersion, `${assetName}.sha256`);

  try {
    log(`Downloading ForkSync binary from ${releaseBinaryUrl}`);
    await downloadFile(releaseBinaryUrl, cachedPath);
    try {
      const checksumTmp = checksumPath(cachedPath);
      await downloadFile(checksumUrl, checksumTmp);
      verifyChecksumFile(checksumTmp, cachedPath);
    } catch (error) {
      if (!String(error.message).includes("status 404")) {
        throw error;
      }
    }
    if (process.platform !== "win32") {
      fs.chmodSync(cachedPath, 0o755);
    }
    return cachedPath;
  } catch (error) {
    if (!allowBuildFallback) {
      throw error;
    }
    log(`Falling back to source build because binary download failed: ${error.message}`);
  }

  const buildRoot = sourceBuildRoot || findCargoProjectRoot(process.cwd());
  return buildReleaseBinary({
    buildRoot,
    executableName,
    cachedPath,
  });
}

module.exports = {
  buildReleaseBinary,
  cachedBinaryPath,
  checksumPath,
  commandExists,
  downloadFile,
  ensureDir,
  findCargoProjectRoot,
  parseBool,
  releaseUrl,
  resolveForkSyncBinary,
  resolveLatestReleaseTag,
  resolveTargetAsset,
  requestJson,
  requestText,
  sha256File,
  verifyChecksumFile,
};
