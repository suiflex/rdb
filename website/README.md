# RDBS website

Marketing site for RDBS, the open-source database manager in this repo.
Static site built with [Astro](https://astro.build) and Tailwind CSS v4.

## Requirements

- Node.js 20+ and npm

## Development

```bash
cd website
npm install
npm run dev        # http://localhost:4321/
```

## Build and test

```bash
npm run build      # static output in dist/
npm run test       # post-build checks: pages, links, metadata, download assets
npm run preview    # serve dist/ locally
```

`npm run test` needs `npm run build` first. Set `SKIP_NETWORK=1` to skip the
HEAD checks against GitHub release assets when offline.

## Content and data

- **Copy** lives directly in `src/pages/*.astro`; shared chrome in
  `src/components/` and `src/layouts/Base.astro`.
- **Design tokens** (colors, fonts, radius, motion) are defined once in
  `src/styles/global.css` under `@theme`. Links and nav live in `src/config.ts`.
- **Release data** (`src/data/release.json`) is committed as a fallback and
  refreshed from the GitHub API with `npm run fetch-release`. Run it after
  cutting a release, commit the diff, and redeploy.
- **Changelog and license pages** read `app/CHANGELOG.md` and `LICENSE` from
  the repo root at build time; they update themselves.

## Screenshots

All product screenshots are real captures of the app's mock-data harness
(seeded demo data, no credentials). To refresh them after a UI change:

```bash
cargo build -p rdb --features mock
for s in workspace sql palette connections; do
  RDB_MOCK=1 RDB_SCREEN=$s RDB_WIN=1440x900 RDB_SHOT=/tmp/$s.bmp \
  RDB_SHOT_DELAY_MS=3500 SLINT_SCALE_FACTOR=2 SLINT_BACKEND=winit-software \
  ./target/debug/rdb
  sips -s format png /tmp/$s.bmp --out website/src/assets/shots/$s.png
done
```

Then regenerate the social image: crop `workspace.png` to 1200x630 as
`public/og.png`. Astro converts everything to responsive WebP at build time.

## Deployment

Hosted on Cloudflare Workers (static assets); canonical home is
`https://rdbs.suiflex.dev` (root base). Workers Builds runs from the repo
root, so the wrangler config and a build delegator live there
(`../wrangler.jsonc`, `../package.json`), not in `website/`. Dashboard
settings for the `rdbs` service:

- Git repository: `suiflex/rdb`, branch `develop`
- Root directory: `/` (repo root)
- Build command: `npm run build` (delegates into `website/`)
- Deploy command: `npx wrangler deploy`
- Domains & Routes: add `rdbs.suiflex.dev`

`.github/workflows/website.yml` runs build + tests as a CI check.
`public/_headers` sets caching and security headers; 404s are served from
the built `404.html`.

Manual deploy from a local checkout (run at the repo root):

```bash
npm run build && npx wrangler deploy
```

Two env vars retarget the build for another host or a subpath preview:

```bash
SITE_URL=https://example.com SITE_BASE=/preview npm run build
```

## Contributing

Same flow as the rest of the repo: see
[CONTRIBUTING.md](../CONTRIBUTING.md) at the repository root. No secrets are
needed to build or run the site.
