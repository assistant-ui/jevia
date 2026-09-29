import assert from "node:assert/strict";
import test from "node:test";

import { syncInstallerSources } from "../../scripts/sync-installer-version.mjs";

test("syncs release references without changing feature minimums", () => {
  const installer = 'JEVIA_VERSION="${JEVIA_VERSION:-0.1.4}"\n';
  const guide = [
    "downloads the matching Jevia 0.1.4 binary from https://github.com/assistant-ui/jevia/releases/tag/v0.1.4, and verifies it.",
    "CLI 0.1.4 automatically detects existing tests.",
  ].join("\n");

  const next = syncInstallerSources(installer, guide, "0.1.5");

  assert.match(next.installer, /JEVIA_VERSION:-0\.1\.5/);
  assert.match(next.guide, /matching Jevia 0\.1\.5 binary/);
  assert.match(next.guide, /releases\/tag\/v0\.1\.5/);
  assert.match(next.guide, /CLI 0\.1\.4 automatically detects/);
});

test("rejects unsafe or ambiguous release updates", () => {
  assert.throws(() => syncInstallerSources("", "", "latest"), /invalid release version/);
  assert.throws(
    () => syncInstallerSources("no version", "no release", "0.1.5"),
    /exactly one installer version declaration/,
  );
});
