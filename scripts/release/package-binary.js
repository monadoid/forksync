#!/usr/bin/env node
"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

function parseArgs(argv) {
  const result = {
    input: "",
    outputDir: "",
    assetName: "",
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--input") {
      result.input = argv[++index] ?? "";
      continue;
    }
    if (arg.startsWith("--input=")) {
      result.input = arg.slice("--input=".length);
      continue;
    }
    if (arg === "--output-dir") {
      result.outputDir = argv[++index] ?? "";
      continue;
    }
    if (arg.startsWith("--output-dir=")) {
      result.outputDir = arg.slice("--output-dir=".length);
      continue;
    }
    if (arg === "--asset-name") {
      result.assetName = argv[++index] ?? "";
      continue;
    }
    if (arg.startsWith("--asset-name=")) {
      result.assetName = arg.slice("--asset-name=".length);
      continue;
    }
  }

  if (!result.input || !result.outputDir || !result.assetName) {
    throw new Error("usage: package-binary.js --input <binary> --output-dir <dir> --asset-name <name>");
  }

  return result;
}

function sha256(filePath) {
  const content = fs.readFileSync(filePath);
  return crypto.createHash("sha256").update(content).digest("hex");
}

function main() {
  const { input, outputDir, assetName } = parseArgs(process.argv.slice(2));
  fs.mkdirSync(outputDir, { recursive: true });

  const outputPath = path.join(outputDir, assetName);
  fs.copyFileSync(input, outputPath);
  if (process.platform !== "win32") {
    fs.chmodSync(outputPath, 0o755);
  }

  const checksum = `${sha256(outputPath)}  ${assetName}\n`;
  fs.writeFileSync(`${outputPath}.sha256`, checksum);
  process.stdout.write(`${outputPath}\n`);
}

main();
