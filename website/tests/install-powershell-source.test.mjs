import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("../public/install.ps1", import.meta.url), "utf8");

test("the Windows installer downloads and verifies the native release asset", () => {
  assert.match(source, /x86_64-pc-windows-msvc/);
  assert.match(source, /\.exe"/);
  assert.match(source, /Invoke-WebRequest/);
  assert.match(source, /Get-FileHash[^\n]+SHA256/);
  assert.match(source, /checksum verification failed/);
  assert.match(source, /Move-Item/);
});

test("the Windows installer keeps its destination and release endpoint configurable", () => {
  assert.match(source, /JEVIA_VERSION/);
  assert.match(source, /JEVIA_INSTALL_DIR/);
  assert.match(source, /JEVIA_DOWNLOAD_BASE_URL/);
  assert.match(source, /LocalApplicationData/);
  assert.doesNotMatch(source, /SetEnvironmentVariable/);
});
