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

test("shows the persisted run shape and explains outcome provenance", async () => {
  for (const url of [docsPage, docsMarkdown]) {
    const contents = await readFile(url, "utf8");

    assert.match(contents, /"schema_version": 6/);
    assert.match(contents, /"state": "completed"/);
    assert.match(contents, /"source": "verification"/);
    assert.match(contents, /privacy\.store_task_text = false/);
    assert.match(contents, /task: null/);
  }
});

test("documents passive history separately from optional verification in both formats", async () => {
  for (const url of [docsPage, docsMarkdown]) {
    const contents = await readFile(url, "utf8");
    assert.match(contents, /Unreleased: additional verification is opt-in/);
    assert.match(contents, /Claude Code 2\.1\.251/);
    assert.match(contents, /Codex 0\.158\.x/);
    assert.match(contents, /OpenCode\s+v1/);
    assert.match(contents, /normal hook trust/);
    assert.match(contents, /process-level/);
    assert.match(contents, /execution\.observations/);
    assert.match(contents, /each history window/);
    assert.doesNotMatch(contents, /Unknown, active, and process-exit-only records are excluded/);
  }
});
