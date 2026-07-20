/** Single source of truth for names, links, and nav. */
export const PRODUCT = "RDB";
export const TAGLINE = "Fast, lightweight, open-source database management for everyone.";

export const GITHUB = "https://github.com/suiflex/rdb";
export const REPO_LINKS = {
  releases: `${GITHUB}/releases`,
  issues: `${GITHUB}/issues`,
  goodFirstIssues: `${GITHUB}/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22`,
  discussions: `${GITHUB}/discussions`,
  contributing: `${GITHUB}/blob/develop/CONTRIBUTING.md`,
  security: `${GITHUB}/blob/develop/SECURITY.md`,
  license: `${GITHUB}/blob/develop/LICENSE`,
  vision: `${GITHUB}/blob/develop/VISION.md`,
  readme: `${GITHUB}#readme`,
  newIssue: `${GITHUB}/issues/new/choose`,
} as const;

/** Prefix an internal path with the configured base (GitHub Pages subpath). */
export function url(path: string): string {
  const base = import.meta.env.BASE_URL.replace(/\/$/, "");
  return `${base}${path}`;
}

export const NAV = [
  { label: "Features", href: "/features" },
  { label: "Databases", href: "/features#databases" },
  { label: "Downloads", href: "/download" },
  { label: "Docs", href: "/docs" },
] as const;

export const FOOTER = {
  Product: [
    { label: "Features", href: "/features", external: false },
    { label: "Downloads", href: "/download", external: false },
    { label: "Changelog", href: "/changelog", external: false },
    { label: "Documentation", href: "/docs", external: false },
  ],
  Community: [
    { label: "GitHub", href: GITHUB, external: true },
    { label: "Contributing", href: "/open-source", external: false },
    { label: "Issues", href: REPO_LINKS.issues, external: true },
    { label: "Discussions", href: REPO_LINKS.discussions, external: true },
  ],
  Project: [
    { label: "Releases", href: REPO_LINKS.releases, external: true },
    { label: "License", href: "/license", external: false },
    { label: "Privacy", href: "/privacy", external: false },
    { label: "Security policy", href: REPO_LINKS.security, external: true },
  ],
} as const;
