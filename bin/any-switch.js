#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const childProcess = require("child_process");

const root = path.resolve(__dirname, "..");
const installScript = path.join(root, "npm", "install.js");
const binaryName = process.platform === "win32" ? "any-switch.exe" : "any-switch";
const binaryPath = path.join(root, "vendor", binaryName);

if (!fs.existsSync(binaryPath)) {
  const install = childProcess.spawnSync(process.execPath, [installScript], {
    stdio: "inherit",
    env: {
      ...process.env,
      npm_config_loglevel: process.env.npm_config_loglevel || "notice"
    }
  });
  if (install.signal) {
    process.kill(process.pid, install.signal);
  }
  if (install.status !== 0) {
    process.exit(install.status || 1);
  }
}

const result = childProcess.spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  shell: false
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status || 0);
