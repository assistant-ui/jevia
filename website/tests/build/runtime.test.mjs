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
  const source = await readFile(new URL("../../public/install.sh", import.meta.url));
  const built = await readFile(new URL("static/install.sh", output));
  assert.deepEqual(built, source);
});

test("the Nitro runtime renders the homepage through patched H3", { timeout: 5000 }, async () => {
  const response = await runtime.fetch(new Request("https://jevia.test/"));
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /text\/html/);
  const html = await response.text();
  assert.match(html, /Model routing that/);
  assert.match(html, /Get started/);
  assert.match(html, /Check the full path/);
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
