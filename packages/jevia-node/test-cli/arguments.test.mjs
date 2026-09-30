import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
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

async function fixture(t, overrides = {}) {
  const cwd = await mkdtemp(join(tmpdir(), "jevia-node-arguments-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const tasks = [];
  const requests = [];
  const server = createServer(async (request, response) => {
    try {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      const payload = JSON.parse(Buffer.concat(chunks).toString());
      tasks.push(payload.state.current_task);
      requests.push(payload);
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
  const env = { ...process.env, TYPESAFE_API_KEY: "local-test-key", TOKIO_WORKER_THREADS: "2", ...overrides };
  await execute(binary, ["init"], { cwd, env, timeout: 10_000 });
  const path = join(cwd, ".jevia", "config.toml");
  const config = await readFile(path, "utf8");
  await writeFile(path, config.replace(
    /base_url = "[^"]+"/,
    `base_url = "http://127.0.0.1:${server.address().port}"`,
  ));
  return { cwd, tasks, requests, client: new JeviaClient({ binary, cwd, env }) };
}

for (const backend of ["jsonl", "sqlite", "postgres"]) {
  test(`SDK routing automatically reuses recorded outcomes with ${backend} storage`, {
    skip: backend === "postgres" && !process.env.JEVIA_TEST_POSTGRES_URL
      && "requires isolated PostgreSQL test database",
  }, async (t) => {
    const { cwd, client, requests } = await fixture(t, {
      SDK_TEST_DB: process.env.JEVIA_TEST_POSTGRES_URL,
    });
    if (backend !== "jsonl") {
      const target = backend === "sqlite"
        ? { backend, path: ".jevia/history.db" }
        : { backend, project: `sdk-learning-${randomUUID()}`, urlEnv: "SDK_TEST_DB", allowInsecureLocalhost: true };
      await client.setupStorage(target, { apply: true, confirmStopped: true });
    }
    const task = "fix the parser";
    const outcomes = () => requests.at(-1).state.recent_completed_outcomes;
    const first = await client.route(task);
    assert.equal(first.outcome, "unknown");
    assert.equal(first.feedback, undefined);
    assert.equal(first.execution, undefined);
    assert.deepEqual(outcomes(), [], "a routed task is not an observed outcome");
    assert.equal((await client.route(task)).source, "cache");
    assert.equal(requests.length, 1, "pending records do not invalidate the cache");

    // The application reports its result; no Jevia verifier or completion call is required.
    const recorded = await client.feedback(first.run_id, "success", { reason: "PRIVATE_FEEDBACK_NOTE" });
    assert.equal(recorded.outcome_evidence.source, "manual");
    assert.equal(recorded.execution, undefined);
    const afterSuccess = await client.route(task);
    assert.equal(afterSuccess.source, "live", "new evidence invalidates an older cached decision");
    assert.equal(requests.length, 2);
    assert.deepEqual(outcomes().map(({ task, tier, outcome, outcome_source }) => ({ task, tier, outcome, outcome_source })), [
      { task, tier: "fast", outcome: "success", outcome_source: "manual" },
    ]);
    assert.ok(!JSON.stringify(requests.at(-1)).includes("PRIVATE_FEEDBACK_NOTE"));
    assert.equal((await client.route(task)).source, "cache");
    assert.equal(requests.length, 2, "unchanged evidence still permits caching");

    // Skip both feedback and verification: routing still records the task and
    // includes the known outcome from other work automatically.
    const unreported = await client.route("a task with no feedback or verifier");
    const saved = await client.show(unreported.run_id);
    assert.equal(saved.outcome, "unknown");
    assert.equal(saved.feedback, undefined);
    assert.equal(saved.execution, undefined);
    assert.deepEqual(outcomes().map(({ outcome }) => outcome), ["success"]);

    await client.feedback(afterSuccess.run_id, "failure");
    const afterFailure = await client.route(task);
    assert.equal(afterFailure.source, "live");
    assert.deepEqual(outcomes().map(({ outcome }) => outcome), ["success", "failure"]);
    assert.ok(!outcomes().some(({ task }) => task === unreported.task));

    // Explicit external completion also supplies evidence, without inventing verifier provenance.
    await client.complete(afterFailure.run_id, "success", { confirmStopped: true });
    assert.equal((await client.route(task)).source, "live");
    assert.deepEqual(outcomes().map(({ outcome }) => outcome), ["success", "failure", "success"]);
    assert.ok(outcomes().every(({ outcome_source, execution }) => outcome_source === "manual" && execution === null));

    await client.feedback(first.run_id, "unknown", { reason: "Withdraw an uncertain result" });
    assert.equal((await client.route(task)).source, "live");
    assert.deepEqual(outcomes().map(({ outcome }) => outcome), ["failure", "success"]);

    const configPath = join(cwd, ".jevia", "config.toml");
    const config = await readFile(configPath, "utf8");
    assert.match(config, /history_limit = 20/);
    await writeFile(configPath, config.replace("history_limit = 20", "history_limit = 1"));
    assert.equal((await client.route(task)).source, "live");
    assert.deepEqual(outcomes().map(({ outcome }) => outcome), ["success"], "only the latest eligible record is included");

    const limited = await readFile(configPath, "utf8");
    assert.match(limited, /store_task_text = true/);
    await writeFile(configPath, limited.replace("store_task_text = true", "store_task_text = false"));
    const privateTask = await client.route("PRIVATE_HISTORICAL_TASK");
    assert.equal(privateTask.task, null);
    await client.feedback(privateTask.run_id, "failure");
    await client.route("another task");
    assert.equal(outcomes().length, 1);
    assert.equal(outcomes()[0].task, null);
    assert.equal(outcomes()[0].outcome, "failure");
    assert.ok(!JSON.stringify(requests.at(-1)).includes("PRIVATE_HISTORICAL_TASK"));
  });
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

test("CLI run records by default and only adds Node tests after opting in", async (t) => {
  const { client, cwd, requests } = await fixture(t);
  const configPath = join(cwd, ".jevia", "config.toml");
  const initial = await readFile(configPath, "utf8");
  const adapter = `\n[harnesses.agent]\ncommand = ${JSON.stringify(process.execPath)}\nargs = ["agent.cjs", "{model}", "{task}"]\n[harnesses.agent.models]\nfast = "test"\nbalanced = "test"\nstrong = "test"\n`;
  await writeFile(configPath, initial + adapter);
  await writeFile(join(cwd, "package.json"), JSON.stringify({ scripts: { test: "node check.cjs" } }));
  await writeFile(join(cwd, "agent.cjs"), `require('node:fs').writeFileSync('agent-finished', 'ok');`);
  await writeFile(join(cwd, "check.cjs"), `
    const fs = require('node:fs'); const assert = require('node:assert/strict');
    assert.equal(fs.readFileSync('agent-finished', 'utf8'), 'ok');
    assert.equal(process.env.CI, 'true');
    fs.appendFileSync('verified', 'v');
    process.exit(fs.existsSync('fail-check') ? 1 : 0);
  `);
  const run = () => execute(binary, ["run", "agent", "fix task"], { cwd, env: client.env, timeout: 30000 });
  await run();
  let [record] = await client.runs({ limit: 1 });
  assert.equal(record.execution.verification, undefined);
  assert.equal(record.lifecycle.state, "completed");
  assert.equal(record.outcome, "unknown");
  const { stdout: listing } = await execute(binary, ["runs", "--limit", "1"], { cwd, env: client.env });
  assert.match(listing, /quality_evidence=false execution_observed=true/);
  assert.match(listing, /requested_model=test/);
  await client.route("use passive history");
  assert.deepEqual(requests.at(-1).state.recent_completed_outcomes, []);
  assert.equal(requests.at(-1).state.recent_execution_observations[0].requested_model, "test");
  assert.equal(requests.at(-1).state.recent_execution_observations[0].process_exit_code, 0);
  await assert.rejects(readFile(join(cwd, "verified")), { code: "ENOENT" });
  await writeFile(configPath, initial + adapter.replace('[harnesses.agent]\n', '[harnesses.agent]\nauto_verify = true\n'));
  await run();
  [record] = await client.runs({ limit: 1 });
  assert.equal(record.outcome, "success");
  assert.equal(record.outcome_evidence.source, "verification");
  assert.equal(record.lifecycle.state, "completed");
  assert.equal(record.feedback, undefined);
  await writeFile(join(cwd, "fail-check"), "fail");
  await assert.rejects(run());
  [record] = await client.runs({ limit: 1 });
  assert.equal(record.outcome, "failure");
  assert.equal(record.outcome_evidence.source, "verification");
  const before = await readFile(join(cwd, "verified"), "utf8");
  await writeFile(configPath, initial + adapter.replace('[harnesses.agent]\n', '[harnesses.agent]\nauto_verify = false\n'));
  await run();
  [record] = await client.runs({ limit: 1 });
  assert.equal(record.outcome_evidence.source, "process_exit");
  assert.equal(record.execution.verification, undefined);
  assert.equal(await readFile(join(cwd, "verified"), "utf8"), before);
  // Explicit verification wins even with a discoverable failing npm test.
  await writeFile(configPath, initial + adapter + `\n[harnesses.agent.verification]\ncommand = ${JSON.stringify(process.execPath)}\nargs = ["--version"]\n`);
  await run();
  [record] = await client.runs({ limit: 1 });
  assert.equal(record.outcome_evidence.source, "verification");
  assert.equal(record.outcome, "success");
  assert.equal(await readFile(join(cwd, "verified"), "utf8"), before);
});

for (const backend of ["jsonl", "sqlite", "postgres"]) {
  test(`native hooks record model changes without manual feedback (${backend})`, {
    skip: backend === "postgres" && !process.env.JEVIA_TEST_POSTGRES_URL
      && "requires isolated PostgreSQL test database",
  }, async (t) => {
    const { client, cwd, requests } = await fixture(t, { SDK_TEST_DB: process.env.JEVIA_TEST_POSTGRES_URL });
    if (backend !== "jsonl") await client.setupStorage(backend === "sqlite"
      ? { backend, path: ".jevia/observations.db" }
      : { backend, project: `hooks-${randomUUID()}`, urlEnv: "SDK_TEST_DB", allowInsecureLocalhost: true },
    { apply: true, confirmStopped: true });
    const configPath = join(cwd, ".jevia", "config.toml");
    const initial = await readFile(configPath, "utf8");
    await writeFile(configPath, initial + `\n[harnesses.agent]\ncommand = ${JSON.stringify(process.execPath)}\nargs = ["hooks.cjs", "{model}", "{task}"]\nobservations = "claude_hooks"\n[harnesses.agent.models]\nfast = "requested-alias"\nbalanced = "requested-alias"\nstrong = "requested-alias"\n`);
    // A compatible wrapper fixture executes the actual injected commands, not a
    // fabricated run record. No provider credentials or real agent are needed.
    await writeFile(join(cwd, "hooks.cjs"), `
      const assert = require('node:assert/strict');
      const { spawnSync } = require('node:child_process');
      const hooks = JSON.parse(process.argv[process.argv.indexOf('--settings') + 1]).hooks;
      const events = [
        { hook_event_name: 'SessionStart', model: 'model-a' },
        { hook_event_name: 'UserPromptSubmit', prompt: 'PRIVATE_PROMPT' },
        { hook_event_name: 'PostToolUseFailure', tool_name: 'Bash', tool_input: { command: 'PRIVATE_COMMAND' } },
        { hook_event_name: 'PostModelSwitch', from_model: 'model-a', to_model: 'model-b' },
        { hook_event_name: 'PostToolUse', tool_name: 'Edit', tool_response: 'PRIVATE_CONTENT' },
        { hook_event_name: 'TaskCompleted', task_subject: 'PRIVATE_TASK' },
        { hook_event_name: 'Stop', last_assistant_message: 'PRIVATE_OUTPUT' },
        { hook_event_name: 'SessionEnd' },
      ];
      for (const event of events) {
        const hook = hooks[event.hook_event_name][0].hooks[0];
        const result = spawnSync(hook.command, hook.args, { input: JSON.stringify({ ...event, session_id: 'private-session-id', transcript_path: '/PRIVATE_PATH' }), encoding: 'utf8' });
        assert.equal(result.status, 0); assert.equal(result.stdout, ''); assert.equal(result.stderr, '');
      }
      if (process.env.WAIT_FOR_CHECKPOINT === '1') {
        const hook = hooks.SessionStart[0].hooks[0];
        const id = /jevia-events-([0-9a-f-]{36})-/.exec(hook.args[2])[1];
        const deadline = Date.now() + 15000;
        let saved;
        do {
          const result = spawnSync(hook.command, ['runs', 'show', id], { encoding: 'utf8' });
          assert.equal(result.status, 0);
          saved = JSON.parse(result.stdout);
          if (saved.execution.observations.events.length === events.length) break;
          Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
        } while (Date.now() < deadline);
        assert.equal(saved.execution.observations.events.length, events.length, 'checkpoint visible while harness is alive');
        assert.equal(saved.lifecycle.state, 'running');
        assert.equal(saved.outcome, 'unknown');
      }
      process.stdout.write('unchanged harness output');
      process.exit(Number(process.env.FAKE_HARNESS_EXIT || 0));
    `);
    await client.route("later task");
    const run = (code) => execute(binary, ["run", "agent", "fix task"], { cwd, env: { ...client.env, FAKE_HARNESS_EXIT: String(code), WAIT_FOR_CHECKPOINT: code === 0 ? "1" : "0" }, timeout: 30000 });
    const { stdout } = await run(0);
    assert.equal(stdout, "unchanged harness output");
    const [record] = await client.runs({ limit: 1 });
    assert.equal(record.schema_version, 5);
    assert.equal(record.outcome, "unknown");
    assert.equal(record.feedback, undefined);
    assert.equal(record.execution.verification, undefined);
    assert.equal(record.execution.model, "requested-alias");
    const captured = record.execution.observations;
    assert.equal(captured.status, "recorded");
    assert.equal(captured.source, "claude_hooks");
    assert.equal(captured.events.length, 8);
    assert.equal(captured.events[3].model, "model-b");
    assert.equal(captured.events[3].previous_model, "model-a");
    assert.ok(!JSON.stringify(record).includes("PRIVATE_"));
    assert.equal((await client.route("later task")).source, "live");
    let state = requests.at(-1).state;
    assert.deepEqual(state.recent_completed_outcomes, []);
    const summary = state.recent_execution_observations[0].harness_observations;
    assert.deepEqual(summary.observed_models, ["model-a", "model-b"]);
    assert.equal(summary.event_counts.tool_failed, 1);
    assert.ok(!JSON.stringify(state).includes("private-session-id"));
    assert.equal((await client.route("later task")).source, "cache");
    await assert.rejects(run(7), { code: 7 });
    const [failed] = await client.runs({ limit: 1 });
    assert.equal(failed.outcome, "unknown");
    assert.equal(failed.execution.exit_code, 7);
    assert.equal(failed.execution.observations.events.length, 8);
    assert.ok(!(await readdir(join(cwd, ".jevia"))).some((name) => name.startsWith("jevia-events-")));
    await client.feedback(record.run_id, "success");
    const updated = await client.show(record.run_id);
    assert.deepEqual(updated.execution.observations, captured);
    await client.route("with optional feedback");
    state = requests.at(-1).state;
    assert.equal(state.recent_completed_outcomes.length, 1);
    assert.equal(state.recent_execution_observations.length, 1);
    assert.ok(!JSON.stringify(state).includes("private-session-id"));
    const exported = join(cwd, "observations-export.jsonl");
    await execute(binary, ["storage", "export", "--output", exported], { cwd, env: client.env });
    assert.ok((await readFile(exported, "utf8")).includes('"model_changed"'));
  });
}

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
