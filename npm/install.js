#!/usr/bin/env node
// postinstall: download the matching prebuilt `rdb` binary from the GitHub
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
    "darwin:arm64": ["aarch64-apple-darwin", "tar.gz", "rdb"],
    "darwin:x64": ["x86_64-apple-darwin", "tar.gz", "rdb"],
    "linux:x64": ["x86_64-unknown-linux-gnu", "tar.gz", "rdb"],
    "linux:arm64": ["aarch64-unknown-linux-gnu", "tar.gz", "rdb"],
    "win32:x64": ["x86_64-pc-windows-msvc", "zip", "rdb.exe"],
    "win32:arm64": ["aarch64-pc-windows-msvc", "zip", "rdb.exe"],
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

// macOS: the .dmg carries the signed RDB.app. Its name mirrors the tarball's
// target triple. Pure so it can be self-checked without touching the network.
function macDmgAsset(arch) {
  const target = resolveTarget("darwin", arch).target;
  return `rdb-${target}.dmg`;
}

// macOS: fetch the .dmg, copy RDB.app into ~/Applications (Launchpad), and
// vendor the inner binary so the `rdb` command still works. No sudo needed.
async function installMacApp(version, vendor) {
  const asset = macDmgAsset(process.arch);
  const tag = `v${version}`;
  const url = `https://github.com/${REPO}/releases/download/${tag}/${asset}`;
  const dmg = path.join(vendor, asset);

  process.stdout.write(`rdb: downloading ${asset} (${tag})…\n`);
  await download(url, dmg);

  const mnt = fs.mkdtempSync(path.join(require("os").tmpdir(), "rdb-dmg-"));
  try {
    execFileSync("hdiutil", ["attach", "-nobrowse", "-readonly", "-mountpoint", mnt, dmg], {
      stdio: "inherit",
    });
    const appName = fs.readdirSync(mnt).find((n) => n.endsWith(".app"));
    if (!appName) throw new Error("dmg did not contain a .app bundle");

    const appsDir = path.join(require("os").homedir(), "Applications");
    fs.mkdirSync(appsDir, { recursive: true });
    const dest = path.join(appsDir, appName);
    fs.rmSync(dest, { recursive: true, force: true });
    execFileSync("cp", ["-R", path.join(mnt, appName), dest], { stdio: "inherit" });
    // Clear quarantine so the ad-hoc-signed app opens without a Gatekeeper prompt.
    try {
      execFileSync("xattr", ["-dr", "com.apple.quarantine", dest], { stdio: "ignore" });
    } catch {
      /* xattr absent or nothing to clear */
    }
    process.stdout.write(`rdb: installed ${appName} to ${appsDir}\n`);

    // Vendor the inner binary so bin/rdb.js keeps resolving the `rdb` command.
    const binPath = path.join(vendor, "rdb");
    fs.copyFileSync(path.join(dest, "Contents", "MacOS", "rdb"), binPath);
    fs.chmodSync(binPath, 0o755);
  } finally {
    try {
      execFileSync("hdiutil", ["detach", mnt], { stdio: "ignore" });
    } catch {
      /* already detached */
    }
    fs.rmSync(dmg, { force: true });
  }
}

async function main() {
  const version = require("./package.json").version;

  const vendor = path.join(__dirname, "vendor");
  fs.mkdirSync(vendor, { recursive: true });

  if (process.platform === "darwin") {
    await installMacApp(version, vendor);
    return;
  }

  const { target, ext, bin } = resolveTarget(process.platform, process.arch);
  const asset = `rdb-${target}.${ext}`;
  const tag = `v${version}`;
  const url = `https://github.com/${REPO}/releases/download/${tag}/${asset}`;
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
  assert.strictEqual(resolveTarget("win32", "x64").bin, "rdb.exe");
  assert.strictEqual(resolveTarget("linux", "x64").ext, "tar.gz");
  assert.throws(() => resolveTarget("sunos", "sparc"));
  assert.strictEqual(macDmgAsset("arm64"), "rdb-aarch64-apple-darwin.dmg");
  assert.strictEqual(macDmgAsset("x64"), "rdb-x86_64-apple-darwin.dmg");
  console.log("selftest ok");
  process.exit(0);
}

main().catch((err) => {
  console.error(`rdb: install failed: ${err.message}`);
  process.exit(1);
});
