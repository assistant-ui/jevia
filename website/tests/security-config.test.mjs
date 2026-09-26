import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const configUrl = new URL("../vercel.json", import.meta.url);

test("Vercel applies the website security headers to every route", async () => {
  const config = JSON.parse(await readFile(configUrl, "utf8"));
  assert.equal(config.headers.length, 1);
  assert.equal(config.headers[0].source, "/(.*)");

  const headers = new Map(
    config.headers[0].headers.map(({ key, value }) => [key.toLowerCase(), value]),
  );

  assert.match(headers.get("content-security-policy"), /default-src 'self'/);
  assert.match(headers.get("content-security-policy"), /frame-ancestors 'none'/);
  assert.equal(headers.get("permissions-policy"), "camera=(), geolocation=(), microphone=()");
  assert.equal(headers.get("referrer-policy"), "strict-origin-when-cross-origin");
  assert.equal(headers.get("strict-transport-security"), "max-age=63072000; includeSubDomains");
  assert.equal(headers.get("x-content-type-options"), "nosniff");
  assert.equal(headers.get("x-frame-options"), "DENY");
});
