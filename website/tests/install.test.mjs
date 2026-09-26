import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const installer = fileURLToPath(new URL("../public/install.sh", import.meta.url));

function runInstaller(t, cargoStub) {
  const directory = mkdtempSync(join(tmpdir(), "jevia-installer-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  if (cargoStub) {
    writeFileSync(join(directory, "cargo"), `#!/bin/sh\n${cargoStub}\n`, { mode: 0o755 });
  }
  return spawnSync("/bin/sh", [installer], {
    encoding: "utf8",
    env: { PATH: directory },
  });
}

test("explains the prerequisite when Cargo is missing", (t) => {
  const result = runInstaller(t);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /requires Rust 1\.92\+ and Cargo/);
  assert.doesNotMatch(result.stdout, /Jevia installed/);
});

test("installs the pinned crates.io release with locked dependencies", (t) => {
  const result = runInstaller(t, 'printf "arg: %s\\n" "$@"');
  assert.equal(result.status, 0);
  assert.match(result.stdout, /Installing Jevia 0\.1\.0 from crates\.io/);
  assert.match(result.stdout, /arg: install\narg: --locked\narg: --version\narg: 0\.1\.0\narg: jevia\n/);
  assert.doesNotMatch(result.stdout, /--git|github\.com/);
  assert.match(result.stdout, /Jevia installed/);
});

test("preserves an installation failure without reporting success", (t) => {
  const result = runInstaller(t, "exit 42");
  assert.equal(result.status, 42);
  assert.doesNotMatch(result.stdout, /Jevia installed/);
});
