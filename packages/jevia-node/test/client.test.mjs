import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { JeviaClient, JeviaCommandError } from "../dist/index.js";

const fixture = fileURLToPath(new URL("./fixture-cli.mjs", import.meta.url));

function client() {
  return new JeviaClient({
    binary: process.execPath,
    binaryArgs: [fixture],
    cwd: new URL("./", import.meta.url),
  });
}

test("reports the installed Jevia CLI version", async () => {
  assert.equal(await client().version(), "0.2.0");
});

test("optionally records passive execution events over stdin, not arguments", async () => {
  const execution = { harness: "app", model: "requested", duration_ms: 2, events: [
    { kind: "model_observed", model: "actual", recorded_at_ms: 1 },
  ] };
  const recorded = await client().recordExecution("--option-like-id", execution);
  assert.equal(recorded.outcome, "unknown");
  assert.equal(recorded.execution.observations.source, "application");
  assert.deepEqual(recorded.execution.observations.events, execution.events);
  assert.deepEqual(recorded.jev_model.split("|"), ["runs", "record-execution", "--json", "--", "--option-like-id"]);
  for (const invalid of [
    { ...execution, prompt: "PRIVATE" }, { ...execution, outcome: "success" },
    { ...execution, duration_ms: -1 }, { ...execution, exit_code: 2147483648 },
    { ...execution, harness: "PRIVATE text" }, { ...execution, model: "" },
    { ...execution, events: Array(257).fill(execution.events[0]) },
    { ...execution, events: [{ ...execution.events[0], output: "PRIVATE" }] },
    { ...execution, events: [{ ...execution.events[0], recorded_at_ms: Number.MAX_SAFE_INTEGER + 1 }] },
  ]) await assert.rejects(client().recordExecution("run", invalid), (error) => error instanceof TypeError && !String(error).includes("PRIVATE"));
  const controller = new AbortController();
  controller.abort("PRIVATE reason");
  await assert.rejects(client().recordExecution("run", execution, { signal: controller.signal }), (error) => error.kind === "aborted" && !String(error).includes("PRIVATE"));
});

test("routes a task as one shell-free argument and supports cache bypass", async () => {
  const task = 'fix $(touch should-not-exist) and "quote" this';
  const route = await client().route(task, { noCache: true });

  assert.equal(route.task, task);
  assert.equal(route.tier, "balanced");
  assert.deepEqual(route.jev_model.split("|"), ["route", "--json", "--no-cache", "--", task]);
});

test("keeps option-like text behind the CLI option boundary", async () => {
  for (const task of ["--help", "- fix this", "--", "--json"]) {
    const route = await client().route(task);
    assert.equal(route.task, task);
    assert.deepEqual(route.jev_model.split("|"), ["route", "--json", "--", task]);
  }
  const updated = await client().feedback("--help", "success", { reason: "--reason=a=b" });
  assert.equal(updated.run_id, "--help");
  assert.equal(updated.feedback[0].reason, "--reason=a=b");
  assert.equal((await client().show("--help")).run_id, "--help");
});

test("records explicit feedback with a reason", async () => {
  const updated = await client().feedback("run-42", "failure", {
    reason: "verification failed",
  });

  assert.equal(updated.run_id, "run-42");
  assert.equal(updated.outcome, "failure");
  assert.equal(updated.feedback?.[0]?.reason, "verification failed");
});

test("lists and inspects typed run records", async () => {
  const runs = await client().runs({ limit: 2 });
  assert.deepEqual(runs.map(({ run_id }) => run_id), ["run-1", "run-2"]);

  const shown = await client().show("run-9");
  assert.equal(shown.run_id, "run-9");
});

test("returns structured command failures", async () => {
  await assert.rejects(client().route("fail"), (error) => {
    assert.ok(error instanceof JeviaCommandError);
    assert.equal(error.exitCode, 2);
    assert.match(error.stderr, /simulated routing failure/);
    return true;
  });
});

test("supports timeouts and AbortSignal cancellation", async () => {
  const timed = new JeviaClient({
    binary: process.execPath,
    binaryArgs: [fixture],
    timeoutMs: 20,
  });
  await assert.rejects(timed.route("wait"), JeviaCommandError);

  const controller = new AbortController();
  const pending = client().route("wait", { signal: controller.signal });
  controller.abort();
  await assert.rejects(pending, JeviaCommandError);
});

test("validates inputs before starting the CLI", async () => {
  await assert.rejects(client().route("  "), /task cannot be empty/);
  await assert.rejects(client().runs({ limit: 0 }), /limit must be a positive safe integer/);
});

test("completion is explicit and preserves option-like identifiers and reasons", async () => {
  for (const options of [undefined, {}, { confirmStopped: false }, { confirmStopped: "true" }]) {
    await assert.rejects(client().complete("id", "success", options), /confirmStopped/);
  }
  const done = await client().complete("--help", "success", { confirmStopped: true, reason: "--tests passed" });
  assert.equal(done.lifecycle.state, "completed");
  assert.deepEqual(done.jev_model.split("|"), ["runs", "complete", "--json", "--confirm-stopped", "--reason=--tests passed", "--", "--help", "success"]);
  await assert.rejects(client().complete("id", "invalid", { confirmStopped: true }), TypeError);
  await assert.rejects(client().complete("id", "success", { confirmStopped: true, reason: "private\0" }), TypeError);
});
