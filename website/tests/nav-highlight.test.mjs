import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const source = await readFile(new URL("../src/lib/nav-highlight.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
});
const { navHighlightBox, navHighlightTarget } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

test("hover tracks independently of the active page, then returns to it", () => {
  assert.equal(navHighlightTarget("overview", "storage", null), "storage");
  assert.equal(navHighlightTarget("overview", null, null), "overview");
  assert.equal(navHighlightTarget("errors", null, null), "errors");
  assert.equal(navHighlightTarget("errors", null, "install"), "install");
  assert.equal(navHighlightTarget("errors", "storage", "install"), "storage");
});

test("vertical highlight preserves fractional row measurements", () => {
  assert.deepEqual(navHighlightBox(
    { left: 128, top: 229.625, width: 220, height: 27.75 },
    { left: 128, top: 146, width: 220, height: 200 },
  ), { x: 0, y: 83.625, width: 220, height: 27.75 });
});

test("horizontal highlight stays in content coordinates when the mobile rail scrolls", () => {
  const item = { left: 240, top: 130, width: 121.25, height: 32 };
  const container = { left: 30, top: 130, width: 800, height: 32 };
  const expected = navHighlightBox(item, container);
  assert.deepEqual(expected, { x: 210, y: 0, width: 121.25, height: 32 });
  assert.deepEqual(navHighlightBox(
    { ...item, left: item.left - 180 },
    { ...container, left: container.left - 180 },
  ), expected);
});
