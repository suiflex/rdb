#!/usr/bin/env node
// Drive the UI through Suitest: import tests/suitest/rdb-desktop.json, run it,
// wait, report. Exits non-zero when a step fails, so it can gate a change.
//
// Suitest must already be running for this project (`suitest up` in the repo
// root, dashboard on SUITEST_URL). The binary must carry Slint's MCP server
// *and* its debug info — `make ui-test` builds it that way; a plain
// `cargo build` silently drops the element ids every step addresses.
//
// Credentials: SUITEST_API_KEY imports the suite; starting a run needs a user
// session, so the local admin from .suitest/credentials.json is used for that.
// Both files are gitignored — nothing here reads or writes a secret elsewhere.

import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const base = process.env.SUITEST_URL ?? "http://127.0.0.1:4000";
const suitePath = resolve(root, "tests/suitest/rdb-desktop.json");
const binary = process.env.RDB_BINARY ?? resolve(root, "target/debug/rdb");
const credentialsPath = resolve(root, ".suitest/credentials.json");

const die = (message) => {
  console.error(`suitest-run: ${message}`);
  process.exit(2);
};

const credentials = existsSync(credentialsPath)
  ? JSON.parse(readFileSync(credentialsPath, "utf8"))
  : null;
const apiKey = process.env.SUITEST_API_KEY ?? credentials?.apiKey;
if (!apiKey) die(`no API key — set SUITEST_API_KEY, or start Suitest so ${credentialsPath} exists`);
if (!existsSync(binary)) die(`no binary at ${binary} — run \`make ui-test\`, not this script directly`);

const api = async (path, { method = "GET", headers = {}, body, raw = false } = {}) => {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: { ...(body ? { "Content-Type": "application/json" } : {}), ...headers },
    body: body ? JSON.stringify(body) : undefined,
    redirect: "manual",
  });
  if (raw) return response;
  const text = await response.text();
  if (!response.ok) die(`${method} ${path} -> ${response.status} ${text.slice(0, 300)}`);
  return text ? JSON.parse(text) : null;
};

// ----- import the suite, with the binary path filled in ---------------------
const suite = JSON.parse(readFileSync(suitePath, "utf8").replaceAll("${RDB_BINARY}", binary));
const imported = await api("/api/v1/test-cases/bulk-import", {
  method: "POST",
  headers: { Authorization: `Bearer ${apiKey}` },
  body: suite,
});
console.log(`imported ${imported.imported.length} case(s) into ${suite.suiteName}`);

// ----- a run needs a session, not an API key -------------------------------
if (!credentials?.email) die("starting a run needs the local admin from .suitest/credentials.json");
// Form-encoded, so this one goes through fetch directly rather than `api`.
const loginResponse = await fetch(`${base}/auth/cookie/login`, {
  method: "POST",
  headers: { "Content-Type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({ username: credentials.email, password: credentials.password }),
  redirect: "manual",
});
if (!loginResponse.ok) die(`login -> ${loginResponse.status}`);
const cookie = (loginResponse.headers.get("set-cookie") ?? "").split(";")[0];
const me = await api("/api/v1/auth/me", { headers: { Cookie: cookie } });
const workspace = me.memberships?.[0]?.workspace_id;
if (!workspace) die("the account has no workspace");
const scoped = { Cookie: cookie, "X-Workspace-Id": workspace };

const run = await api(`/api/v1/suites/${imported.suiteId}/run`, {
  method: "POST",
  headers: scoped,
  body: { name: "RDB desktop UI", env: "local", branch: process.env.GIT_BRANCH ?? "" },
});
console.log(`run ${run.public_id ?? run.id} queued — ${base}/runs/${run.id}`);

// ----- wait it out ---------------------------------------------------------
const done = new Set(["PASS", "FAIL", "ERROR", "CANCELLED"]);
let final = run;
for (let i = 0; i < 120; i += 1) {
  await new Promise((r) => setTimeout(r, 5000));
  final = await api(`/api/v1/runs/${run.id}`, { headers: scoped });
  if (done.has(String(final.status).toUpperCase())) break;
}
const summary = final.summary ?? {};
console.log(
  `${final.public_id}: ${final.status} — ${summary.passed_steps ?? 0}/${summary.total_steps ?? 0} steps in ${
    final.duration_ms ?? 0
  }ms`,
);

const steps = await api(`/api/v1/runs/${run.id}/steps`, { headers: scoped });
for (const step of steps.filter((s) => s.outcome !== "PASS")) {
  console.log(`  FAIL ${step.case_public_id} step ${step.step_order}: ${step.error_message ?? ""}`);
}
const artifacts = await api(`/api/v1/runs/${run.id}/artifacts`, { headers: scoped });
const kinds = artifacts.reduce((acc, a) => ({ ...acc, [a.kind]: (acc[a.kind] ?? 0) + 1 }), {});
console.log(`  artifacts: ${JSON.stringify(kinds)}`);
console.log(`  dashboard: ${base}/runs/${run.id}`);

process.exit(String(final.status).toUpperCase() === "PASS" ? 0 : 1);
