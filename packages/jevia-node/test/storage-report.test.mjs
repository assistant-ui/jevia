import assert from "node:assert/strict";
import test from "node:test";
import { inspect } from "node:util";
import { JeviaClient, JeviaCommandError, JeviaProtocolError } from "../dist/index.js";
import { isStorageCheckReport } from "../dist/protocol.js";

const healthy = { schema_version: 1, backend: "sqlite", check: "basic", ok: true,
  records: 10, passive_history_index: "present", error: null };
const failure = { ...healthy, ok: false, records: null, passive_history_index: null,
  error: { code: "history_check_failed", message: "History check failed." } };
const fake = (report, exit = 0, args = ["storage", "check", "--json"]) => new JeviaClient({
  binary: process.execPath,
  binaryArgs: ["-e", `const assert = require('node:assert/strict'); assert.deepEqual(process.argv.slice(1), ${JSON.stringify(args)}); console.log(${JSON.stringify(JSON.stringify(report))}); process.exitCode = ${exit};`, "--"],
});

test("typed storage diagnostics preserve the legacy text method and return health failures", async () => {
  assert.deepEqual(await fake(healthy).checkStorageReport(), healthy);
  assert.deepEqual(await fake(failure, 1).checkStorageReport(), failure);
  const deep = { ...healthy, check: "deep" };
  assert.deepEqual(await fake(deep, 0, ["storage", "check", "--json", "--deep"]).checkStorageReport({ deep: true }), deep);
  assert.equal(typeof await fake(healthy, 0, ["storage", "check"]).checkStorage(), "string");
});

test("report validation rejects unsupported versions, unsafe counts, and inconsistent states", async () => {
  for (const report of [null, {}, { ...healthy, schema_version: 2 }, { ...healthy, records: -1 },
    { ...healthy, records: Number.MAX_SAFE_INTEGER + 1 }, { ...healthy, backend: null },
    { ...healthy, passive_history_index: "unsupported" }, { ...healthy, error: failure.error },
    { ...failure, records: 10 }, { ...failure, error: null }, { ...failure, error: { code: "PRIVATE", message: "PRIVATE" } },
  ]) assert.equal(isStorageCheckReport(report), false);
  for (const index of ["present", "missing", "unavailable"]) assert.ok(isStorageCheckReport({ ...healthy, passive_history_index: index }));
  assert.ok(isStorageCheckReport({ ...healthy, backend: "postgres", passive_history_index: "unsupported" }));
  assert.ok(isStorageCheckReport({ ...healthy, backend: "jsonl", passive_history_index: "not_applicable" }));
  for (const [report, code] of [[failure, 0], [healthy, 1], [{ ...healthy, check: "deep" }, 0], [{ PRIVATE: "payload" }, 0]]) {
    await assert.rejects(fake(report, code).checkStorageReport(), error => {
      assert.ok(error instanceof JeviaProtocolError);
      assert.ok(!inspect(error).includes("PRIVATE"));
      return true;
    });
  }
});

test("typed checks retain process errors, cancellation, and input validation", async () => {
  await assert.rejects(fake(healthy, 2).checkStorageReport(), JeviaCommandError);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(fake(healthy).checkStorageReport({ signal: controller.signal }), error => error.kind === "aborted");
  await assert.rejects(fake(healthy).checkStorageReport({ deep: "true" }), TypeError);
  await assert.rejects(new JeviaClient({ binary: "jevia-fixture-does-not-exist" }).checkStorageReport(), error => error.kind === "not_found");
});
