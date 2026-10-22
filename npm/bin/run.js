"use strict";

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const command = path.basename(process.argv[1], ".js");
const binary = process.platform === "win32" ? `${command}.exe` : command;
const binaryPath = path.join(__dirname, "..", "vendor", binary);

if (!fs.existsSync(binaryPath)) {
  console.error(`${command}: binary not found. Reinstall with \`npm i -g @suiflex/rdb\`.`);
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`${command}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 0);
