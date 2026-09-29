import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { before, test } from "node:test";

const output = new URL("../../.vercel/output/", import.meta.url);
const functionEntry = new URL("functions/__nitro.func/index.mjs", output);
let runtime;

before(async () => {
  // Exercise the packaged runtime, not only the prerendered HTML. A successful
  // Farm build can otherwise hide a Nitro/H3 error during prerendering.
  ({ default: runtime } = await import(functionEntry.href));
});

test("prerendering emits the homepage and the unchanged installer", async () => {
  const html = await readFile(new URL("static/index.html", output), "utf8");
  assert.match(html, /Model routing that/);
  assert.match(html, /Install the CLI/);
  assert.match(html, /Check the full path/);
  assert.match(html, /Open any harness/);
  assert.match(html, /Codex, Claude Code, OpenCode, Gemini CLI/);
  assert.match(html, /Node\.js API/);
  assert.match(html.replace(/<[^>]+>/g, ""), /npm install jevia/);
  assert.match(html, /href="\/docs"/);
  assert.doesNotMatch(html, /From task to evidence/);
  assert.match(html, /COPY: Launch/);
  assert.match(html, /rel="canonical" href="https:\/\/jevia\.dev\/"/);
  assert.match(html, /property="og:url" content="https:\/\/jevia\.dev\/"/);
  assert.match(html, /property="og:image" content="https:\/\/jevia\.dev\/og-image\.png"/);
  assert.match(html, /name="twitter:card" content="summary_large_image"/);
  const source = await readFile(new URL("../../public/install.sh", import.meta.url));
  const built = await readFile(new URL("static/install.sh", output));
  assert.deepEqual(built, source);
  const powershellSource = await readFile(new URL("../../public/install.ps1", import.meta.url));
  const builtPowershell = await readFile(new URL("static/install.ps1", output));
  assert.deepEqual(builtPowershell, powershellSource);
});

test("prerendering emits the Jevia documentation", async () => {
  const html = await readFile(new URL("static/docs/index.html", output), "utf8");
  assert.match(html, /Jevia documentation/);
  assert.match(html, /Route, run, and inspect/);
  assert.match(html, /How adaptive routing works/);
  assert.match(html, /Programmatic harness integration/);
  assertAutomaticPipelineDocs(html.replace(/<[^>]+>/g, ""));
  assert.match(html, /setupStorage/);
  assert.match(html, /JeviaCommandError/);
  assert.match(html, /Choose and configure storage/);
  assert.match(html, /\.jevia\/runs\.jsonl/);
  assert.match(html, /\.jevia\/jevia\.db/);
  assert.match(html, /--path \.data\/jevia\/history\.db/);
  assert.match(html, /Cache, diagnostics, and recovery/);
  assert.match(html, /token keyword/);
  assert.match(html, /aria-label="Documentation navigation"/);
  assert.match(html, /aria-label="Documentation sections"/);
  assert.match(html, /class="docs-nav-highlight" aria-hidden="true"/);
  assert.match(html, /href="\/api\/docs-markdown"/);
  assert.match(html, /VIEW \.MD/i);
  assert.match(html, /COPY \.MD/i);
  assert.match(html, /Need a portable version of this guide/);
  assert.match(html, /<title>Documentation — Jevia<\/title>/);
  assert.match(html, /rel="canonical" href="https:\/\/jevia\.dev\/docs"/);
});

test("the packaged runtime serves the Markdown documentation", async () => {
  const response = await runtime.fetch(
    new Request("https://jevia.test/docs.md", {
      headers: { accept: "text/markdown" },
    }),
  );
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /text\/markdown/);
  const markdown = await response.text();
  assert.match(markdown, /^# Jevia documentation$/m);
  assert.match(markdown, /^## Route, run, and inspect$/m);
  assert.match(markdown, /^### Client methods$/m);
  assert.match(markdown, /confirmStopped: true/);
  assert.match(markdown, /urlEnv: "JEVIA_DATABASE_URL"/);
  assert.match(markdown, /^## Choose and configure storage$/m);
  assert.match(markdown, /jevia storage setup postgres --project my-project/);
  assert.match(markdown, /^## Cache, diagnostics, and recovery$/m);
  assert.match(markdown, /import \{ JeviaClient \} from "jevia";/);
  assertAutomaticPipelineDocs(markdown.replace(/[`*]/g, ""));
  assert.doesNotMatch(markdown, /View \.mdCOPY \.MD/);
});

function assertAutomaticPipelineDocs(content) {
  const text = content.replace(/\s+/g, " ");
  assert.match(text, /Automatic CLI pipeline/);
  assert.match(text, /jevia run codex/);
  assert.match(text, /No manual feedback or runs complete step is needed/);
  assert.match(text, /CLI 0\.1\.4 detects root Rust/);
  assert.match(text, /Missing or ambiguous checks stay unverified and are excluded from learning/);
  assert.match(text, /when the agent process\/session finishes/);
  assert.match(text, /SDK routing does not launch an agent or run tests/);
  assert.match(text, /no human feedback prompt is required/);
  assert.match(text, /Recorded outcomes inform the next route/);
  assert.match(text, /No manual cache clearing is needed/);
  assert.match(text, /application-reported/);
  assert.match(text, /history_limit/);
  assert.match(text, /feedback reasons stay in storage/);
  assert.match(text, /Feedback and verification are optional/);
  assert.match(text, /Skipping feedback still records the route with an unknown outcome/);
  assert.doesNotMatch(text, /useHistory|verifyResult\(result\)/);
}

test("View .md serves raw Markdown under normal browser headers", async () => {
  const response = await runtime.fetch(
    new Request("https://jevia.test/api/docs-markdown", {
      headers: { accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8" },
    }),
  );
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /text\/markdown/);
  assert.equal(response.headers.get("content-disposition"), 'inline; filename="docs.md"');
  const markdown = await response.text();
  assert.match(markdown, /^# Jevia documentation$/m);
  assert.doesNotMatch(markdown, /<!doctype html>/i);
});

test("the build includes crawler discovery and social image assets", async () => {
  const robots = await readFile(new URL("static/robots.txt", output), "utf8");
  assert.match(robots, /^User-agent: \*$/m);
  assert.match(robots, /^Sitemap: https:\/\/jevia\.dev\/sitemap\.xml$/m);

  const sitemap = await readFile(new URL("static/sitemap.xml", output), "utf8");
  assert.match(sitemap, /<loc>https:\/\/jevia\.dev\/<\/loc>/);
  assert.match(sitemap, /<loc>https:\/\/jevia\.dev\/docs<\/loc>/);

  for (const asset of ["favicon.ico", "favicon.png", "og-image.png"]) {
    const contents = await readFile(new URL(`static/${asset}`, output));
    assert.ok(contents.length > 0, `${asset} should not be empty`);
  }
});

test("the Nitro runtime renders the homepage through patched H3", { timeout: 5000 }, async () => {
  const response = await runtime.fetch(new Request("https://jevia.test/"));
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /text\/html/);
  const html = await response.text();
  assert.match(html, /Model routing that/);
  assert.match(html, /Get started/);
  assert.match(html, /Check the full path/);
  assert.match(html, /Open any harness/);
});

test("the Nitro runtime renders the documentation route", { timeout: 5000 }, async () => {
  const response = await runtime.fetch(new Request("https://jevia.test/docs"));
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /text\/html/);
  const html = await response.text();
  assert.match(html, /Route, run, and inspect/);
  assert.match(html, /Choose and configure storage/);
  assert.match(html, /Cache, diagnostics, and recovery/);
});

test("unmatched routes return 404 instead of a runtime error", { timeout: 5000 }, async () => {
  const response = await runtime.fetch(new Request("https://jevia.test/missing-security-smoke-test"));
  assert.equal(response.status, 404);
  await response.text();
});

test("the packaged Sharp runtime includes working native image support", async () => {
  const runtimeRequire = createRequire(functionEntry);
  const sharp = runtimeRequire("sharp");
  assert.equal(runtimeRequire("sharp/package.json").version, "0.35.4");
  const png = await sharp({
    create: { width: 2, height: 2, channels: 4, background: "#090909" },
  }).png().toBuffer();
  const metadata = await sharp(png).metadata();
  assert.equal(metadata.format, "png");
  assert.equal(metadata.width, 2);
  assert.equal(metadata.height, 2);
});
