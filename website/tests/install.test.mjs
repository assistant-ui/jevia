import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const installer = fileURLToPath(new URL("../public/install.sh", import.meta.url));
const installerSource = readFileSync(installer, "utf8");
const installerVersion = installerSource.match(
  /JEVIA_VERSION="\$\{JEVIA_VERSION:-([^}]+)\}"/,
)?.[1];
assert.ok(installerVersion, "installer declares a default version");
const systemPath = process.env.PATH ?? "/usr/bin:/bin";

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function writeCommand(directory, name, body) {
  const path = join(directory, name);
  writeFileSync(path, `#!/bin/sh\n${body}\n`, { mode: 0o755 });
  return path;
}

function createFixture(directory, target = "x86_64-unknown-linux-musl") {
  const fixtureDirectory = join(directory, "fixtures");
  mkdirSync(fixtureDirectory);
  const assetName = `jevia-v${installerVersion}-${target}`;
  const assetPath = join(fixtureDirectory, assetName);
  writeFileSync(assetPath, `#!/bin/sh\nprintf "%s\\n" "jevia ${installerVersion}"\n`);
  chmodSync(assetPath, 0o755);
  const digest = createHash("sha256").update(readFileSync(assetPath)).digest("hex");
  writeFileSync(join(fixtureDirectory, `${assetName}.sha256`), `${digest}  ${assetName}\n`);
  return { assetName, assetPath, fixtureDirectory };
}

function runInstaller(t, options = {}) {
  const directory = mkdtempSync(join(tmpdir(), "jevia-installer-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const commands = join(directory, "commands");
  const installDirectory = join(directory, "bin");
  mkdirSync(commands);

  if (options.stubPlatform !== false) {
    writeCommand(
      commands,
      "uname",
      `case "\${1:-}" in -s) printf '%s\\n' '${options.system ?? "Linux"}' ;; -m) printf '%s\\n' '${options.architecture ?? "x86_64"}' ;; esac`,
    );
  }

  if (options.stubDownload !== false) {
    writeCommand(
      commands,
      "curl",
      `
output=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --proto) shift 2 ;;
    --tlsv1.2 | -fsSL) shift ;;
    -o) output=$2; shift 2 ;;
    *) url=$1; shift ;;
  esac
done
cp "$JEVIA_FIXTURE_DIR/\${url##*/}" "$output"
      `.trim(),
    );
  }

  const fixture = createFixture(
    directory,
    options.target ?? "x86_64-unknown-linux-musl",
  );
  if (options.invalidChecksum) {
    writeFileSync(
      join(fixture.fixtureDirectory, `${fixture.assetName}.sha256`),
      `${"0".repeat(64)}  ${fixture.assetName}\n`,
    );
  }

  const path = options.path ?? `${commands}:${systemPath}`;
  mkdirSync(installDirectory);
  options.beforeInstall?.(installDirectory);
  const result = spawnSync("/bin/sh", [installer], {
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: directory,
      JEVIA_DOWNLOAD_BASE_URL: `https://downloads.example.test/v${installerVersion}`,
      JEVIA_FIXTURE_DIR: fixture.fixtureDirectory,
      JEVIA_INSTALL_DIR: installDirectory,
      PATH: path,
    },
  });

  return { directory, fixture, installDirectory, result };
}

test("explains the prerequisite when curl is missing", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "jevia-installer-path-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const result = spawnSync("/bin/sh", [installer], {
    encoding: "utf8",
    env: { HOME: directory, PATH: directory },
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /requires curl/);
  assert.doesNotMatch(result.stdout, /Installed Jevia/);
});

test("installs the verified Linux release binary without Cargo", (t) => {
  const { installDirectory, result } = runInstaller(t);
  assert.equal(result.status, 0, result.stderr);
  assert.match(
    result.stdout,
    new RegExp(`Downloading Jevia ${escapeRegExp(installerVersion)} for x86_64-unknown-linux-musl`),
  );
  assert.match(result.stdout, /OK/);
  assert.doesNotMatch(result.stdout, /cargo|crates\.io/i);

  const installed = join(installDirectory, "jevia");
  assert.ok(existsSync(installed));
  assert.ok((statSync(installed).mode & 0o111) !== 0);
  const version = spawnSync(installed, ["--version"], { encoding: "utf8" });
  assert.equal(version.status, 0);
  assert.equal(version.stdout.trim(), `jevia ${installerVersion}`);
});

test("maps Apple Silicon to the published Darwin target", (t) => {
  const { result } = runInstaller(t, {
    system: "Darwin",
    architecture: "arm64",
    target: "aarch64-apple-darwin",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /aarch64-apple-darwin/);
});

test("rejects a release binary with the wrong checksum", (t) => {
  const { installDirectory, result } = runInstaller(t, { invalidChecksum: true });
  assert.notEqual(result.status, 0);
  assert.ok(!existsSync(join(installDirectory, "jevia")));
  assert.doesNotMatch(result.stdout, /Installed Jevia/);
});

test("rejects unsupported architectures before downloading", (t) => {
  const { result } = runInstaller(t, { architecture: "riscv64" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unsupported architecture: riscv64/);
  assert.doesNotMatch(result.stdout, /Downloading Jevia/);
});

test("updates an existing regular binary and removes staging files", (t) => {
  const { installDirectory, result } = runInstaller(t, {
    beforeInstall: (directory) => writeFileSync(join(directory, "jevia"), "old binary"),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(readdirSync(installDirectory), ["jevia"]);
  assert.match(readFileSync(join(installDirectory, "jevia"), "utf8"), /#!\/bin\/sh/);
});

for (const kind of ["directory", "directory-link", "file-link", "dangling-link", "fifo"]) {
  test(`rejects a ${kind} binary destination before downloading, preserving its contents`, (t) => {
    const { installDirectory, result } = runInstaller(t, {
      beforeInstall: (directory) => {
        const destination = join(directory, "jevia");
        const target = join(directory, "target");
        if (kind === "directory") mkdirSync(destination);
        if (kind === "directory-link") { mkdirSync(target); symlinkSync(target, destination); }
        if (kind === "file-link") { writeFileSync(target, "keep"); symlinkSync(target, destination); }
        if (kind === "dangling-link") symlinkSync(target, destination);
        if (kind === "fifo") assert.equal(spawnSync("mkfifo", [destination]).status, 0);
        if (kind === "directory" || kind === "directory-link") writeFileSync(join(destination, "keep"), "keep");
      },
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /install destination/);
    assert.doesNotMatch(result.stdout, /Downloading|Installed Jevia/);
    assert.ok(!readdirSync(installDirectory).some((name) => name.startsWith(".jevia.install.")));
    if (kind === "directory" || kind === "directory-link") {
      assert.deepEqual(readdirSync(join(installDirectory, "jevia")), ["keep"]);
    }
    if (kind === "file-link") assert.equal(readFileSync(join(installDirectory, "target"), "utf8"), "keep");
  });
}
