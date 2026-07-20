// @ts-check
import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";
import tailwindcss from "@tailwindcss/vite";

// Canonical home is the custom subdomain (public/CNAME points GitHub Pages
// at it). SITE_URL / SITE_BASE override for previews or a different host.
const site = process.env.SITE_URL ?? "https://rdbs.suiflex.dev";
const base = process.env.SITE_BASE ?? "/";

export default defineConfig({
  site,
  base,
  trailingSlash: "ignore",
  integrations: [sitemap()],
  vite: {
    plugins: [tailwindcss()],
  },
});
