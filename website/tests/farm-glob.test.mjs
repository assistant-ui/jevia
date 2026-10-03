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
