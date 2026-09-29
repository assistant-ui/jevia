import assert from "node:assert/strict";
import { inspect } from "node:util";
import test from "node:test";
import { JeviaClient, JeviaCommandError, JeviaProtocolError } from "../dist/index.js";

const secret = "PRIVATE_DIAGNOSTIC_SENTINEL";
function safe(error) {
  for (const text of [String(error), error.stack, inspect(error), inspect(error, { showHidden: true }), JSON.stringify(error), JSON.stringify({ ...error })]) {
    assert.ok(!text.includes(secret), text);
  }
  assert.equal(error.cause, undefined);
  return true;
}
function client(script, options = {}) {
  return new JeviaClient({ binary: process.execPath, binaryArgs: ["-e", script, "--"], ...options });
}

test("command errors hide raw diagnostics while preserving explicit access", () => {
  const command = ["jevia", secret];
  const error = new JeviaCommandError(command, Object.assign(new Error(secret), { code: 17, signal: "SIGTERM" }), secret, secret);
  safe(error);
  assert.equal(error.exitCode, 17);
  assert.equal(error.signal, "SIGTERM");
  assert.equal(error.stdout, secret);
  assert.equal(error.stderr, secret);
  assert.deepEqual(error.command, command);
  command.push("later mutation");
  assert.equal(error.command.length, 2);
  assert.ok(Object.isFrozen(error.command));
});

test("failed processes do not echo task text with or without stderr", async () => {
  for (const script of ["process.exit(17)", `process.stdout.write('${secret}'); process.stderr.write('${secret}'); process.exit(17)`]) {
    await assert.rejects(client(script).route(secret), (error) => {
      assert.ok(error instanceof JeviaCommandError);
      assert.equal(error.exitCode, 17);
      assert.equal(error.command.at(-1), secret);
      return safe(error);
    });
  }
});

test("invalid JSON does not retain the parser's output-bearing cause", async () => {
  await assert.rejects(client(`process.stdout.write('${secret}')`).route(secret), (error) => {
    assert.ok(error instanceof JeviaProtocolError);
    return safe(error);
  });
});

test("spawn errors, timeouts, aborts, and buffer limits are private", async () => {
  const controller = new AbortController();
  controller.abort(secret);
  const cases = [
    new JeviaClient({ binary: `/nonexistent/${secret}` }).route(secret),
    client("", { cwd: `${secret}\0` }).route(secret),
    client("setTimeout(() => {}, 10000)", { timeoutMs: 20 }).route(secret),
    client("", {}).route(secret, { signal: controller.signal }),
    client(`process.stdout.write('${secret}'.repeat(100))`, { maxBufferBytes: 10 }).route(secret),
  ];
  await Promise.all(cases.map((pending) => assert.rejects(pending, (error) => {
    assert.ok(error instanceof JeviaCommandError);
    return safe(error);
  })));
});

test("invalid arguments fail without echoing input", async () => {
  await assert.rejects(client("").route(`${secret}\0`), safe);
  await assert.rejects(client("").feedback("id", "success", { reason: `${secret}\0` }), safe);
});
