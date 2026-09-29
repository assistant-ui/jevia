import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflows = ["ci.yml", "codeql.yml", "release-binaries.yml"].map((name) => ({
  name,
  contents: readFileSync(
    new URL(`../../.github/workflows/${name}`, import.meta.url),
    "utf8",
  ),
}));

test("third-party workflow actions are pinned to immutable commits", () => {
  for (const { name, contents } of workflows) {
    for (const line of contents.split("\n")) {
      const match = line.match(/uses: ([^/\s]+)\/[^@\s]+@([^\s]+)/);
      if (!match || match[1] === "actions" || match[1] === "github") {
        continue;
      }

      assert.match(
        match[2],
        /^[0-9a-f]{40}$/,
        `${name} must pin ${match[1]} actions to a full commit SHA`,
      );
    }
  }
});
