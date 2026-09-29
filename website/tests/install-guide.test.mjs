import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/install-guide.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
});
const { getInstallGuide } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

test("uses the current origin for localhost and production installers", () => {
  for (const origin of ["http://localhost:3000", "https://example.com"]) {
    assert.ok(getInstallGuide(origin).includes(`curl -fsSL ${origin}/install.sh | sh`));
  }
  assert.ok(!getInstallGuide("https://example.com").includes("http://localhost:3000"));
});

test("includes the complete installation and verification sequence", () => {
  const guide = getInstallGuide("http://localhost:3000");
  assert.ok(guide.startsWith("# Install and verify Jevia\n"));
  assert.match(guide, /prebuilt binary/);
  assert.match(guide, /does not require Rust or Cargo/);
  assert.match(guide, /releases\/tag\/v0\.1\.3/);
  assert.match(guide, /SHA-256 checksum/);
  const steps = [
    "curl --version",
    "command -v sha256sum",
    "curl -fsSL",
    "jevia --version",
    "jevia init",
    "export TYPESAFE_API_KEY",
    "jevia doctor",
    "jevia check",
    "jevia route --json",
    "jevia runs --json",
    "jevia cache status",
  ];
  let previous = -1;
  for (const step of steps) {
    const index = guide.indexOf(step);
    assert.ok(index > previous, `${step} should follow the previous setup step`);
    previous = index;
  }
  assert.match(guide, /jev api: ok/);
  assert.match(guide, /jevia: ready/);
  assert.equal(guide.match(/^```/gm).length % 2, 0);
});

test("preserves configuration, protects credentials, and distinguishes routing from success", () => {
  const guide = getInstallGuide("http://localhost:3000");
  assert.match(guide, /if \[ ! -f \.jevia\/config\.toml \]/);
  assert.match(guide, /Never ask the user to paste a secret into chat/);
  assert.match(guide, /stop on failure/);
  assert.match(guide, /set -euo pipefail/);
  assert.match(guide, /may incur provider usage/);
  assert.match(guide, /does not execute the coding task/);
  assert.match(guide, /Do not record success feedback/);
});

test("distinguishes automatic CLI recording from explicit SDK feedback and unreleased detection", () => {
  const guide = getInstallGuide("https://example.com");
  assert.match(guide, /No manual `feedback` or `runs complete` step is needed after `jevia run`/);
  assert.match(guide, /installed CLI 0\.1\.3 needs a configured verification command/);
  assert.match(guide, /test detection is unreleased, pending the next CLI release after 0\.1\.3/);
  assert.match(guide, /when the launched process\/session finishes/);
  assert.match(guide, /unverified and is excluded from learning/);
  assert.match(guide, /do not fill this gap by submitting guessed feedback/);
  assert.match(guide, /SDK-controlled work outside `jevia run`/);
  assert.match(guide, /no human feedback prompt is required/);
});
