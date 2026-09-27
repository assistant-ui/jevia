import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

// Use the existing TypeScript compiler so these tests also run on Node 22.13.
const source = readFileSync(new URL("../src/lib/shell-tokens.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
});
const { tokenizeShellCommand } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

test("highlights executables, flags, and the pipe in the install command", () => {
  const tokens = tokenizeShellCommand("curl -fsSL http://localhost:3000/install.sh | sh");
  assert.deepEqual(
    tokens.filter((token) => token.kind !== "plain").map(({ text, kind }) => ({ text, kind })),
    [
      { text: "curl", kind: "command" },
      { text: "-fsSL", kind: "option" },
      { text: "|", kind: "operator" },
      { text: "sh", kind: "command" },
    ],
  );
});

test("highlights environment assignments and quoted values", () => {
  const tokens = tokenizeShellCommand('export TYPESAFE_API_KEY="your-key"');
  assert.deepEqual(
    tokens.filter((token) => token.kind !== "plain").map(({ text, kind }) => ({ text, kind })),
    [
      { text: "export", kind: "command" },
      { text: "TYPESAFE_API_KEY", kind: "variable" },
      { text: "=", kind: "operator" },
      { text: '"your-key"', kind: "string" },
    ],
  );
});

test("keeps quoted operators inside strings", () => {
  const tokens = tokenizeShellCommand('jevia route "fix foo | bar and \\"quotes\\""');
  assert.equal(tokens.filter((token) => token.kind === "string").length, 1);
  assert.equal(tokens.filter((token) => token.kind === "operator").length, 0);
});

test("preserves whitespace, quotes, and every character of the original command", () => {
  for (const command of [
    "",
    "jevia init",
    "jevia check",
    'jevia route "fix the flaky integration test"',
    'jevia run agent "fix the flaky integration test"',
    "  jevia\t--json route 'fix the test'  ",
    'export TYPESAFE_API_KEY="your-key"',
    'echo "unterminated',
    "curl -fsSL https://example.com/install.sh | sh",
  ]) {
    const tokens = tokenizeShellCommand(command);
    assert.equal(tokens.map((token) => token.text).join(""), command);
    for (const token of tokens) {
      assert.equal(command.slice(token.start, token.start + token.text.length), token.text);
    }
  }
});
