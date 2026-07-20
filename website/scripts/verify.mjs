// Post-build checks against dist/. Run `npm run build` first.
// Fails (exit 1) on: missing pages, broken internal links, missing
// metadata, placeholder text, or banned typography in visible copy.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const DIST = new URL("../dist/", import.meta.url).pathname;
const BASE = (process.env.SITE_BASE ?? "/").replace(/\/$/, "");
let failures = 0;

function fail(msg) {
  failures++;
  console.error(`FAIL ${msg}`);
}

function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

if (!existsSync(DIST)) {
  console.error("dist/ not found. Run `npm run build` first.");
  process.exit(1);
}

// 1. Expected pages
const expected = [
  "index.html",
  "features/index.html",
  "download/index.html",
  "docs/index.html",
  "open-source/index.html",
  "changelog/index.html",
  "privacy/index.html",
  "license/index.html",
  "404.html",
  "robots.txt",
  "sitemap-index.xml",
  "manifest.webmanifest",
  "favicon.svg",
  "og.png",
];
for (const page of expected) {
  if (!existsSync(join(DIST, page))) fail(`missing ${page}`);
}

const htmlFiles = walk(DIST).filter((f) => f.endsWith(".html"));

// 2. Per-page checks
for (const file of htmlFiles) {
  const rel = relative(DIST, file);
  const html = readFileSync(file, "utf-8");

  // Metadata
  for (const [name, re] of [
    ["<title>", /<title>[^<]{5,}<\/title>/],
    ["meta description", /name="description" content="[^"]{20,}"/],
    ["canonical", /rel="canonical"/],
    ["og:image", /property="og:image"/],
    ["lang attr", /<html lang="en"/],
  ]) {
    if (!re.test(html)) fail(`${rel}: missing ${name}`);
  }

  // Placeholder text
  for (const banned of ["lorem ipsum", "TODO", "placeholder text"]) {
    if (html.toLowerCase().includes(banned.toLowerCase())) {
      fail(`${rel}: contains "${banned}"`);
    }
  }

  // Banned typography in visible copy (em/en dashes)
  const text = html
    .replace(/<script[\s\S]*?<\/script>/g, "")
    .replace(/<[^>]+>/g, " ");
  if (/[—–]/.test(text)) fail(`${rel}: em/en dash in visible text`);

  // Internal links resolve
  for (const m of html.matchAll(/href="([^"#?]+)(?:[#?][^"]*)?"/g)) {
    const href = m[1];
    if (/^(https?:|mailto:|data:)/.test(href)) continue;
    let path = href.startsWith(BASE) ? href.slice(BASE.length) : href;
    if (path === "" || path === "/") path = "/index.html";
    const candidates = [
      join(DIST, path),
      join(DIST, path, "index.html"),
      join(DIST, `${path.replace(/\/$/, "")}.html`),
    ];
    if (!candidates.some((c) => existsSync(c))) {
      fail(`${rel}: broken internal link ${href}`);
    }
  }
}

// 3. Download links point at the release recorded in release.json
const release = JSON.parse(
  readFileSync(new URL("../src/data/release.json", import.meta.url), "utf-8"),
);
const downloadHtml = readFileSync(join(DIST, "download/index.html"), "utf-8");
for (const asset of release.assets) {
  if (!downloadHtml.includes(asset.url)) {
    fail(`download page missing asset link ${asset.name}`);
  }
}
if (!downloadHtml.includes(release.tag)) fail("download page missing release tag");

// 4. Optional network check of download URLs (skipped offline)
if (!process.env.SKIP_NETWORK) {
  const sample = release.assets.slice(0, 3);
  for (const asset of sample) {
    try {
      const res = await fetch(asset.url, { method: "HEAD", redirect: "follow" });
      if (!res.ok) fail(`asset URL ${asset.name} responded ${res.status}`);
    } catch {
      console.warn(`WARN network unavailable, skipped HEAD ${asset.name}`);
      break;
    }
  }
}

if (failures) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log(`OK ${htmlFiles.length} pages verified: links, metadata, downloads, typography`);
