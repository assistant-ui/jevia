import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/scroll-motion.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
});
const { glidePosition, scrollDuration } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

test("glides through long jumps without pausing or overshooting", () => {
  const start = 500;
  const target = 2200;
  const duration = scrollDuration(target - start);
  const visited = [];

  for (let elapsed = 1000 / 60; elapsed <= duration + 1000 / 60; elapsed += 1000 / 60) {
    const next = glidePosition(start, target, elapsed, duration);
    assert.ok(next > (visited.at(-1) ?? start) && next <= target);
    visited.push(next);
  }

  assert.equal(visited.at(-1), target);
  assert.ok(visited.some((value) => value > 900 && value < 1500));
  assert.ok(visited.some((value) => value > 1500 && value < 2100));
});

test("retargets from the visible position and scales timing with distance", () => {
  const duration = scrollDuration(1700);
  const forward = glidePosition(500, 2200, 100, duration);
  const reversed = glidePosition(forward, 500, 16, scrollDuration(forward - 500));
  assert.ok(reversed < forward);

  assert.equal(glidePosition(500, 2200, duration / 2, duration), 1987.5);
  assert.ok(scrollDuration(1700) > scrollDuration(300));
  assert.equal(scrollDuration(10_000), 620);
});
