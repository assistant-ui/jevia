import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const websiteRoot = new URL("../", import.meta.url);

test("local development, CI, and Vercel use the Node 22 release line", async () => {
  const major = (await readFile(new URL(".node-version", websiteRoot), "utf8")).trim();
  const packageJson = JSON.parse(
    await readFile(new URL("package.json", websiteRoot), "utf8"),
  );
  const workflow = await readFile(
    new URL("../.github/workflows/ci.yml", websiteRoot),
    "utf8",
  );

  assert.equal(major, "22");
  assert.equal(packageJson.engines.node, `${major}.x`);
  assert.match(workflow, /node-version-file: website\/\.node-version/);
});
