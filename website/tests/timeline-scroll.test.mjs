import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/timeline-scroll.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
});
const { isTimelineVisible, wheelTarget, slidePosition } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

test("activates for a visible timeline even when the outer section extends offscreen", () => {
  assert.equal(isTimelineVisible({ top: 480, bottom: 700, height: 220 }, 720), true);
  assert.equal(isTimelineVisible({ top: -20, bottom: 200, height: 220 }, 720), true);
  assert.equal(isTimelineVisible({ top: 650, bottom: 870, height: 220 }, 720), false);
  assert.equal(isTimelineVisible({ top: 800, bottom: 1020, height: 220 }, 720), false);
  assert.equal(isTimelineVisible({ top: -300, bottom: -80, height: 220 }, 720), false);
});

test("accumulates wheel movement, reverses immediately, and clamps at the ends", () => {
  assert.equal(wheelTarget(80, 120, 60, 700), 180);
  assert.equal(wheelTarget(80, 80, 0.5, 700), 80.5);
  assert.equal(wheelTarget(80, 120, -30, 700), 50);
  assert.equal(wheelTarget(650, 650, 100, 700), 700);
  assert.equal(wheelTarget(20, 20, -100, 700), 0);
  assert.equal(wheelTarget(0, 0, -100, 700), 0);
  assert.equal(wheelTarget(700, 700, 100, 700), 700);
});

test("slides without overshooting and settles within 300ms", () => {
  let position = 0;
  for (let frame = 0; frame < 18; frame++) {
    const next = slidePosition(position, 320, 1000 / 60);
    assert.ok(next >= position && next <= 320);
    position = next;
  }
  assert.equal(position, 320);
  assert.ok(slidePosition(320, 0, 16) < 320);
  assert.equal(slidePosition(0, 0, 16), 0);
});

test("uses elapsed time rather than frame rate for consistent motion", () => {
  const once = slidePosition(0, 320, 32);
  const twice = slidePosition(slidePosition(0, 320, 16), 320, 16);
  assert.ok(Math.abs(once - twice) < 0.0001);
});
