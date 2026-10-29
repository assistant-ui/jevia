import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const websiteRoot = new URL("../", import.meta.url);

test("local development and CI use Node 26 LTS; Vercel builds on the 24 line", async () => {
  const version = (await readFile(new URL(".node-version", websiteRoot), "utf8")).trim();
  const packageJson = JSON.parse(
    await readFile(new URL("package.json", websiteRoot), "utf8"),
  );
  const workflow = await readFile(
    new URL("../.github/workflows/ci.yml", websiteRoot),
    "utf8",
  );

  assert.match(version, /^26\.\d+\.\d+$/);
  // Vercel reads engines.node and does not offer Node 26 yet.
  assert.equal(packageJson.engines.node, "24.x");
  assert.match(workflow, /node-version-file: website\/\.node-version/);
});
