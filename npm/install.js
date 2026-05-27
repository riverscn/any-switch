#!/usr/bin/env node
"use strict";

const childProcess = require("child_process");
const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "..");
const vendorDir = path.join(root, "vendor");
const binaryName = process.platform === "win32" ? "any-switch.exe" : "any-switch";
const binaryPath = path.join(vendorDir, binaryName);

function commandExists(command) {
  const probe = process.platform === "win32" ? "where" : "command";
  const args = process.platform === "win32" ? [command] : ["-v", command];
  const result = childProcess.spawnSync(probe, args, {
    shell: process.platform !== "win32",
    stdio: "ignore"
  });
  return result.status === 0;
}

function run(command, args) {
  const result = childProcess.spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    shell: false
  });
  if (result.error) {
    throw result.error;
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with exit code ${result.status}`);
  }
}

function main() {
  if (!commandExists("cargo")) {
    throw new Error(
      [
        "Rust toolchain is required to install any-switch from npm.",
        "Install Rust from https://rustup.rs, then run `npm install -g any-switch` again."
      ].join("\n")
    );
  }

  console.error("any-switch: building from source with Cargo");
  run("cargo", ["build", "--release", "--locked"]);

  const builtBinary = path.join(root, "target", "release", binaryName);
  if (!fs.existsSync(builtBinary)) {
    throw new Error(`Cargo build succeeded but ${builtBinary} was not found`);
  }

  fs.mkdirSync(vendorDir, { recursive: true });
  fs.copyFileSync(builtBinary, binaryPath);
  if (process.platform !== "win32") {
    fs.chmodSync(binaryPath, 0o755);
  }
  console.error(`any-switch: installed ${binaryName}`);
}

try {
  main();
} catch (error) {
  console.error(`any-switch: ${error.message}`);
  process.exit(1);
}
