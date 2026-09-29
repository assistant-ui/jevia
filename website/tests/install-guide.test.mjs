import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/install-guide.ts", import.meta.url), "utf8");
const installerSource = readFileSync(new URL("../public/install.sh", import.meta.url), "utf8");
const powershellInstallerSource = readFileSync(
  new URL("../public/install.ps1", import.meta.url),
  "utf8",
);
const installerVersion = installerSource.match(
  /JEVIA_VERSION="\$\{JEVIA_VERSION:-([^}]+)\}"/,
)?.[1];
assert.ok(installerVersion, "installer declares a default version");
const powershellInstallerVersion = powershellInstallerSource.match(
  /\$DefaultJeviaVersion = "([^"]+)"/,
)?.[1];
assert.equal(powershellInstallerVersion, installerVersion);
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
});
const { getInstallGuide } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`
);

test("uses the current origin for localhost and production installers", () => {
  for (const origin of ["http://localhost:3000", "https://example.com"]) {
    assert.ok(getInstallGuide(origin).includes(`curl -fsSL ${origin}/install.sh | sh`));
    assert.ok(getInstallGuide(origin).includes(`irm ${origin}/install.ps1 | iex`));
  }
  assert.ok(!getInstallGuide("https://example.com").includes("http://localhost:3000"));
});

test("includes the complete installation and verification sequence", () => {
  const guide = getInstallGuide("http://localhost:3000");
  assert.ok(guide.startsWith("# Install and verify Jevia\n"));
  assert.match(guide, /prebuilt binary/);
  assert.match(guide, /do not require Rust or Cargo/);
  assert.ok(guide.includes(`releases/tag/v${installerVersion}`));
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

test("distinguishes automatic CLI recording from explicit SDK feedback and versioned detection", () => {
  const guide = getInstallGuide("https://example.com");
  assert.match(guide, /No manual `feedback` or `runs complete` step is needed after `jevia run`/);
  assert.match(guide, /CLI 0\.1\.4–0\.1\.5 enabled test discovery by default/);
  assert.match(guide, /additional verification is opt-in/);
  assert.match(guide, /passive observations inform later routing/);
  assert.match(guide, /Claude Code 2\.1\.251/);
  assert.match(guide, /Other harnesses currently supply process-level observations/);
  assert.match(guide, /Do not promise these features from the current published installer/);
  assert.match(guide, /when the launched process\/session finishes/);
  assert.match(guide, /not proof of task success/);
  assert.match(guide, /do not fill this gap by submitting guessed feedback/);
  assert.match(guide, /SDK-controlled work outside `jevia run`/);
  assert.match(guide, /no human feedback prompt is required/);
  assert.match(guide, /feedback and verification are optional/);
  assert.match(guide, /Skipping feedback leaves the recorded outcome unknown/);
});
