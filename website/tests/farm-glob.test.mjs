import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { globFiles as esmGlob } from "@farm.js/core";

const require = createRequire(import.meta.url);
const { globFiles: cjsGlob } = require("@farm.js/core");

test("Farm scans route alternatives and nested paths in ESM and CommonJS", async () => {
  const root = await mkdtemp(join(tmpdir(), "jevia-farm-glob-"));
  try {
    for (const file of ["page.tsx", "docs/page.tsx", "docs/layout.jsx", "docs/note.md", ".hidden/page.tsx"]) {
      await mkdir(dirname(join(root, file)), { recursive: true });
      await writeFile(join(root, file), "fixture");
    }
    for (const glob of [esmGlob, cjsGlob]) {
      assert.deepEqual((await glob("**/{page,layout}.{tsx,jsx}", root)).sort(), [
        "docs/layout.jsx", "docs/page.tsx", "page.tsx",
      ]);
      assert.deepEqual(await glob("missing/**/*.tsx", root), []);
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("the installed dependency graph removes the vulnerable glob chain", async () => {
  const lock = await readFile(new URL("../pnpm-lock.yaml", import.meta.url), "utf8");
  assert.doesNotMatch(lock, /^\s+(?:braces|micromatch|fast-glob)@/m);
  const farmRequire = createRequire(require.resolve("@farm.js/core"));
  assert.equal(typeof farmRequire("tinyglobby").glob, "function");
});

test("Farm upgrades keep CLI/core and version-scoped security protections aligned", async () => {
  const manifest = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  const installed = JSON.parse(await readFile(new URL("../node_modules/@farm.js/core/package.json", import.meta.url), "utf8"));
  const cli = JSON.parse(await readFile(new URL("../node_modules/@farm.js/cli/package.json", import.meta.url), "utf8"));
  const version = manifest.dependencies["@farm.js/core"];
  const key = `@farm.js/core@${version}`;
  assert.equal(manifest.devDependencies["@farm.js/cli"], version);
  assert.equal(installed.version, version);
  assert.equal(cli.version, version);
  assert.equal(cli.dependencies["@farm.js/core"], version);
  assert.equal(manifest.pnpm.overrides[`${key}>fast-glob`], "-");
  assert.equal(manifest.pnpm.overrides[`${key}>@scalar/api-reference`], "1.72.1");
  assert.equal(manifest.pnpm.packageExtensions[key]?.dependencies.tinyglobby, "0.2.17");
  const patch = manifest.pnpm.patchedDependencies[key];
  assert.equal(typeof patch, "string", "the current Farm version requires its reviewed glob patch");
  const contents = await readFile(new URL(`../${patch}`, import.meta.url), "utf8");
  assert.match(contents, /\+.*import\("tinyglobby"\)/);
  assert.doesNotMatch(contents, /\+.*import\("fast-glob"\)/);
});
