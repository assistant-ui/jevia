import assert from "node:assert/strict";
import { inspect } from "node:util";
import test from "node:test";
import { JeviaClient, JeviaCommandError, JeviaProtocolError } from "../dist/index.js";

const secret = "PRIVATE_ERROR_KIND_SENTINEL";
function check(error, kind) {
  assert.equal(error.kind, kind);
  for (const text of [String(error), error.stack, inspect(error, { showHidden: true }), JSON.stringify(error), JSON.stringify({ ...error })]) {
    assert.ok(!text.includes(secret), text);
  }
  assert.equal(error.cause, undefined);
  return true;
}

test("categories use an allowlist and never retain raw error codes or causes", () => {
  for (const [code, kind] of [
    ["ENOENT", "not_found"], ["ENOTDIR", "not_found"],
    ["EACCES", "permission_denied"], ["EPERM", "permission_denied"],
    ["JEVIA_TIMEOUT", "timeout"], ["ABORT_ERR", "aborted"],
    ["ERR_CHILD_PROCESS_STDIO_MAXBUFFER", "output_limit"],
    ["ERR_INVALID_ARG_TYPE", "invalid_options"], ["ERR_INVALID_ARG_VALUE", "invalid_options"],
    ["ERR_OUT_OF_RANGE", "invalid_options"], ["ERR_INVALID_FILE_URL_PATH", "invalid_options"],
    ["ERR_INVALID_FILE_URL_HOST", "invalid_options"], ["ERR_INVALID_URL_SCHEME", "invalid_options"],
    [17, "exit"], [secret, "spawn_failed"], [undefined, "spawn_failed"],
  ]) {
    const cause = Object.assign(new Error(secret), { code, path: secret, cause: new Error(secret) });
    const error = new JeviaCommandError([secret], cause, secret, secret);
    check(error, kind);
    assert.equal(error.stdout, secret);
    assert.equal(error.stderr, secret);
    assert.equal(error.command[0], secret);
    assert.equal(error.code, undefined, "raw code must not be exposed");
  }
  check(new JeviaCommandError([], Object.assign(new Error(secret), { signal: "SIGTERM" }), "", ""), "signal");
  check(new JeviaCommandError([], Object.assign(new Error(secret), { signal: secret }), "", ""), "spawn_failed");
  check(new JeviaProtocolError("Jevia returned invalid JSON"), "protocol");
});

function client(script, options = {}) {
  return new JeviaClient({ binary: process.execPath, binaryArgs: ["-e", script, "--"], ...options });
}

test("real commands distinguish missing paths, aborts, timeouts, limits, and exits", async () => {
  const cases = [
    [new JeviaClient({ binary: `/nonexistent/${secret}` }).route(secret), "not_found"],
    [client("", { cwd: `/nonexistent/${secret}` }).route(secret), "not_found"],
    [client("", { cwd: `${secret}\0` }).route(secret), "invalid_options"],
    [client("", {}).route(secret, { signal: AbortSignal.abort(secret) }), "aborted"],
    [client("setTimeout(() => {}, 6000)", { timeoutMs: 50 }).route(secret), "timeout"],
    [client(`process.stdout.write('${secret}'.repeat(10))`, { maxBufferBytes: 10 }).route(secret), "output_limit"],
    [client("process.exit(17)").route(secret), "exit"],
    [client(`process.stdout.write('${secret}')`).route(secret), "protocol"],
  ];
  await Promise.all(cases.map(([promise, kind]) => assert.rejects(promise, error => check(error, kind))));
});

test("signal exits have a safe category", { skip: process.platform === "win32" }, async () => {
  await assert.rejects(client("process.kill(process.pid, 'SIGTERM')").route(secret), error => {
    assert.equal(error.signal, "SIGTERM");
    return check(error, "signal");
  });
});
