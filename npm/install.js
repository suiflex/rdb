#!/usr/bin/env node
// postinstall: download the matching prebuilt `rdbs` binary from the GitHub
// Release whose tag equals this package's version, and drop it in vendor/.
// ponytail: shells out to the system `tar` (bsdtar handles both .tar.gz and
// .zip on macOS/Linux/Win10+) instead of pulling an archive dependency.
"use strict";

const fs = require("fs");
const path = require("path");
const https = require("https");
const { execFileSync } = require("child_process");

const REPO = "suiflex/rdb";

// platform/arch -> { target triple, archive ext, binary name }. Pure so it can
// be self-checked without touching the network.
function resolveTarget(platform, arch) {
  const map = {
    "darwin:arm64": ["aarch64-apple-darwin", "tar.gz", "rdbs"],
    "darwin:x64": ["x86_64-apple-darwin", "tar.gz", "rdbs"],
    "linux:x64": ["x86_64-unknown-linux-gnu", "tar.gz", "rdbs"],
    "linux:arm64": ["aarch64-unknown-linux-gnu", "tar.gz", "rdbs"],
    "win32:x64": ["x86_64-pc-windows-msvc", "zip", "rdbs.exe"],
    "win32:arm64": ["aarch64-pc-windows-msvc", "zip", "rdbs.exe"],
  };
  const hit = map[`${platform}:${arch}`];
  if (!hit) {
    throw new Error(`unsupported platform ${platform}/${arch}`);
  }
  const [target, ext, bin] = hit;
  return { target, ext, bin };
}

function download(url, dest, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > 5) return reject(new Error("too many redirects"));
    https
      .get(url, { headers: { "User-Agent": "rdb-npm-installer" } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          return resolve(download(res.headers.location, dest, redirects + 1));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`download failed: HTTP ${res.statusCode} for ${url}`));
        }
        const out = fs.createWriteStream(dest);
        res.pipe(out);
        out.on("finish", () => out.close(resolve));
        out.on("error", reject);
      })
      .on("error", reject);
  });
}

async function main() {
  const version = require("./package.json").version;
  const { target, ext, bin } = resolveTarget(process.platform, process.arch);
  const asset = `rdbs-${target}.${ext}`;
  const tag = `v${version}`;
  const url = `https://github.com/${REPO}/releases/download/${tag}/${asset}`;

  const vendor = path.join(__dirname, "vendor");
  fs.mkdirSync(vendor, { recursive: true });
  const archive = path.join(vendor, asset);

  process.stdout.write(`rdb: downloading ${asset} (${tag})…\n`);
  await download(url, archive);

  // bsdtar extracts both gzip tarballs and zips with the same flags.
  execFileSync("tar", ["-xf", archive, "-C", vendor], { stdio: "inherit" });
  fs.unlinkSync(archive);

  const binPath = path.join(vendor, bin);
  if (!fs.existsSync(binPath)) {
    throw new Error(`extracted archive did not contain ${bin}`);
  }
  if (process.platform !== "win32") fs.chmodSync(binPath, 0o755);
  process.stdout.write(`rdb: installed ${bin}\n`);
}

// `node install.js --selftest` exercises the pure mapping without a network.
if (process.argv.includes("--selftest")) {
  const assert = require("assert");
  assert.strictEqual(resolveTarget("darwin", "arm64").target, "aarch64-apple-darwin");
  assert.strictEqual(resolveTarget("win32", "x64").bin, "rdbs.exe");
  assert.strictEqual(resolveTarget("linux", "x64").ext, "tar.gz");
  assert.throws(() => resolveTarget("sunos", "sparc"));
  console.log("selftest ok");
  process.exit(0);
}

main().catch((err) => {
  console.error(`rdb: install failed: ${err.message}`);
  process.exit(1);
});
