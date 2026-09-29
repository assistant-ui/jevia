import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflow = readFileSync(
  new URL("../../.github/workflows/codeql.yml", import.meta.url),
  "utf8",
);

test("CodeQL scans the repository's workflow, TypeScript, and Rust code", () => {
  assert.match(workflow, /- actions/);
  assert.match(workflow, /- javascript-typescript/);
  assert.match(workflow, /- rust/);
  assert.match(workflow, /uses: github\/codeql-action\/init@v4/);
  assert.match(workflow, /uses: github\/codeql-action\/analyze@v4/);
  assert.match(workflow, /queries: security-extended/);
});

test("CodeQL runs for pull requests, main, scheduled, and manual scans", () => {
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /pull_request:/);
  assert.match(workflow, /push:/);
  assert.match(workflow, /schedule:/);
  assert.match(workflow, /security-events: write/);
});
