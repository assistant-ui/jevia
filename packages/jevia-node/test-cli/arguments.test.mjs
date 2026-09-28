import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import { JeviaClient } from "../dist/index.js";

const execute = promisify(execFile);
const binary = process.env.JEVIA_TEST_BINARY ?? fileURLToPath(new URL(
  `../../../target/debug/jevia${process.platform === "win32" ? ".exe" : ""}`,
  import.meta.url,
));

async function fixture(t) {
  const cwd = await mkdtemp(join(tmpdir(), "jevia-node-arguments-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const tasks = [];
  const server = createServer(async (request, response) => {
    try {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      const payload = JSON.parse(Buffer.concat(chunks).toString());
      tasks.push(payload.state.current_task);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({
        model: "jev-test",
        answers: { tier: {
          type: "choice", choice: "fast", confidence: 0.94,
          probabilities: { fast: 0.94, balanced: 0.05, strong: 0.01 },
        } },
      }));
    } catch {
      response.writeHead(400);
      response.end();
    }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => new Promise((resolve) => {
    server.closeAllConnections();
    server.close(resolve);
  }));
  const env = { ...process.env, TYPESAFE_API_KEY: "local-test-key", TOKIO_WORKER_THREADS: "2" };
  await execute(binary, ["init"], { cwd, env, timeout: 10_000 });
  const path = join(cwd, ".jevia", "config.toml");
  const config = await readFile(path, "utf8");
  await writeFile(path, config.replace(
    /base_url = "[^"]+"/,
    `base_url = "http://127.0.0.1:${server.address().port}"`,
  ));
  return { cwd, tasks, client: new JeviaClient({ binary, cwd, env }) };
}

test("real CLI receives option-like tasks literally and still honors cache bypass", async (t) => {
  const { client, tasks } = await fixture(t);
  for (const task of ["- fix the parser", "--help", "--json", "--no-cache", "--", "-"]) {
    const record = await client.route(task);
    assert.equal(record.tier, "fast");
    assert.equal(tasks.at(-1), task);
    const cached = await client.route(task);
    assert.equal(cached.source, "cache");
    const requestCount = tasks.length;
    const live = await client.route(task, { noCache: true });
    assert.equal(live.source, "live");
    assert.equal(tasks.length, requestCount + 1);
    assert.equal(tasks.at(-1), task);
  }
});

test("real CLI preserves option-like feedback reasons", async (t) => {
  const { client } = await fixture(t);
  const record = await client.route("a normal task");
  for (const reason of ["- tests passed", "--help", "--json", "--", "-", "--reason=a=b"]) {
    const updated = await client.feedback(record.run_id, "success", { reason });
    assert.equal(updated.outcome, "success");
    assert.equal(updated.feedback.at(-1).reason, reason);
    assert.equal((await client.show(record.run_id)).feedback.at(-1).reason, reason);
  }
});

test("real CLI completes external work and makes it eligible for archival", async (t) => {
  const { client, cwd } = await fixture(t);
  const records = [await client.route("first task"), await client.route("second task")];
  await assert.rejects(execute(binary, ["runs", "complete", records[0].run_id, "success"], { cwd }));
  for (const record of records) {
    const done = await client.complete(record.run_id, "success", { confirmStopped: true, reason: "--tests passed" });
    assert.equal(done.lifecycle.state, "completed");
    assert.equal(done.lifecycle.started_at_ms, null);
    assert.ok(done.lifecycle.finished_at_ms > 0);
    assert.equal(done.execution, undefined);
    assert.equal(done.outcome_evidence.source, "manual");
    assert.equal(done.feedback.at(-1).reason, "--tests passed");
    await assert.rejects(client.complete(record.run_id, "success", { confirmStopped: true }));
  }
  const { stdout } = await execute(binary, ["runs", "archive", "--keep", "1", "--json"], { cwd });
  assert.equal(JSON.parse(stdout).archived_records, 1);
});

test("real CLI looks up option-like imported run IDs literally", async (t) => {
  const { client, cwd } = await fixture(t);
  const record = await client.route("a normal task");
  for (const runId of ["--help", "--json", "-run-1"]) {
    const imported = { ...record, run_id: runId };
    await writeFile(join(cwd, ".jevia", "runs.jsonl"), `${JSON.stringify(imported)}\n`);
    assert.equal((await client.show(runId)).run_id, runId);
    const updated = await client.feedback(runId, "success", { reason: "checked" });
    assert.equal(updated.run_id, runId);
    assert.equal(updated.outcome, "success");
  }
});
