// Refresh src/data/release.json from the GitHub Releases API.
// Run before a deploy to pick up a new release; on any failure the
// committed JSON stays in place, so the site always has valid data.
import { writeFileSync } from "node:fs";

const TAG = process.env.RELEASE_TAG?.trim();
const API = TAG
  ? `https://api.github.com/repos/suiflex/rdb/releases/tags/${encodeURIComponent(TAG)}`
  : "https://api.github.com/repos/suiflex/rdb/releases/latest";
const OUT = new URL("../src/data/release.json", import.meta.url);

try {
  const res = await fetch(API, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!res.ok) throw new Error(`GitHub API responded ${res.status}`);
  const data = await res.json();
  const release = {
    tag: data.tag_name,
    published: data.published_at.split("T")[0],
    assets: data.assets.map((a) => {
      const asset = {
        name: a.name,
        size: a.size,
        sha256: (a.digest ?? "").replace(/^sha256:/, ""),
        url: a.browser_download_url,
      };
      if (!asset.name || !asset.size || !asset.sha256 || !asset.url) {
        throw new Error(`release asset missing data: ${asset.name || "unknown"}`);
      }
      return asset;
    }),
  };
  if (!release.tag || !release.published || release.assets.length === 0) {
    throw new Error("release payload missing tag or assets");
  }
  writeFileSync(OUT, JSON.stringify(release, null, 2) + "\n");
  console.log(`release.json updated to ${release.tag} (${release.assets.length} assets)`);
} catch (err) {
  console.warn(`fetch-release: keeping committed release.json (${err.message})`);
  if (TAG) process.exitCode = 1;
}
