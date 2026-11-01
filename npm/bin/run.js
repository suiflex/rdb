"use strict";

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const command = path.basename(process.argv[1], ".js");
const binary = process.platform === "win32" ? `${command}.exe` : command;
const binaryPath = path.join(__dirname, "..", "vendor", binary);
const installer = path.join(__dirname, "..", "install.js");

// npm can skip postinstall (--ignore-scripts, allowScripts policy), and a
// reinstall skips it again. Download on first run instead. Installer output
// goes to stderr: stdout belongs to `rdb-mcp`'s stdio protocol.
if (!fs.existsSync(binaryPath)) {
  spawnSync(process.execPath, [installer], { stdio: ["ignore", 2, 2] });
}

if (!fs.existsSync(binaryPath)) {
  console.error(`${command}: binary not found. Run \`node "${installer}"\` to download it.`);
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`${command}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 0);
