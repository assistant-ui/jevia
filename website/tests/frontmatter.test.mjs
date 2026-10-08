import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import test from "node:test";

const require = createRequire(import.meta.url);
const farmRequire = createRequire(require.resolve("@farm.js/core"));
const docsRequire = createRequire(farmRequire.resolve("@farming-labs/docs"));
const matter = docsRequire("gray-matter");
const matterRequire = createRequire(docsRequire.resolve("gray-matter"));

test("Farm's frontmatter parser uses the migrated safe YAML API", () => {
  assert.equal(matterRequire("js-yaml/package.json").version, "4.3.2");
  const parsed = matter("---\ntitle: Getting started\norder: 1\npublished: true\ntags: [cli, sdk]\nsummary: |\n  Line one\n  Line two\n---\n# Hello\n");
  assert.deepEqual(parsed.data, {
    title: "Getting started", order: 1, published: true,
    tags: ["cli", "sdk"], summary: "Line one\nLine two\n",
  });
  assert.equal(parsed.content, "# Hello\n");
  const roundtrip = matter(matter.stringify(parsed.content, parsed.data));
  assert.deepEqual(roundtrip.data, parsed.data);
  assert.equal(roundtrip.content, parsed.content);
  assert.deepEqual(matter("# No metadata\n").data, {});
});

test("YAML frontmatter rejects malformed and executable YAML tags", () => {
  for (const metadata of ["title: [", "x: !!js/function 'function () { return 1; }'", "x: !!js/regexp /test/"]) {
    assert.throws(() => matter(`---\n${metadata}\n---\nbody\n`));
  }
});

test("the installed graph no longer includes the legacy sprintf-js chain", async () => {
  const lock = await readFile(new URL("../pnpm-lock.yaml", import.meta.url), "utf8");
  assert.doesNotMatch(lock, /^\s+(?:sprintf-js@|argparse@1\.|js-yaml@3\.|next@)/m);
});

test("Geist's local font assets do not need the removed Next.js peer", async () => {
  for (const font of ["geist-sans/Geist-Variable.woff2", "geist-mono/GeistMono-Variable.woff2"]) {
    const bytes = await readFile(new URL(`../node_modules/geist/dist/fonts/${font}`, import.meta.url));
    assert.equal(bytes.subarray(0, 4).toString(), "wOF2");
  }
  const layout = await readFile(new URL("../src/app/layout.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(layout, /from ["'](?:next|geist\/font)/);
  assert.match(layout, /localFont/);
});
