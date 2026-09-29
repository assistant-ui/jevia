import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const docsPage = new URL("../src/app/docs/page.tsx", import.meta.url);
const docsMarkdown = new URL("../src/app/docs/page.md", import.meta.url);
const reference = new URL("../../docs/reference.md", import.meta.url);

test("documents default history locations and custom SQLite paths", async () => {
  for (const url of [docsPage, docsMarkdown, reference]) {
    const contents = await readFile(url, "utf8");

    assert.match(contents, /\.jevia\/jevia\.db/);
    assert.match(contents, /--path \.data\/jevia\/history\.db/);
    assert.match(contents, /sqlite:\/\/\.data\/jevia\/history\.db/);
    assert.match(contents, /absolute/);
  }

  for (const url of [docsPage, docsMarkdown]) {
    assert.match(await readFile(url, "utf8"), /\.jevia\/runs\.jsonl/);
  }
});
