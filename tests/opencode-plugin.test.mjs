import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdtemp, writeFile, readFile, lstat, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import test from "node:test";

const exec = promisify(execFile);
const jevia = resolve(process.env.JEVIA_TEST_BINARY ?? `target/debug/jevia${process.platform === "win32" ? ".exe" : ""}`);
const runner = fileURLToPath(new URL("./fixtures/opencode-plugin-runner.mjs", import.meta.url));
const initial = { source: "opencode_plugin", status: "no_events", events: [] };

async function fixture(t, mode) {
  const cwd = await mkdtemp(join(tmpdir(), "jevia-plugin-loss-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const id = randomUUID();
  const journal = join(cwd, ".jevia", `jevia-events-${id}-fixture.jsonl`);
  const marker = journal.replace(/\.jsonl$/, ".loss");
  const env = {
    PATH: process.env.PATH, SystemRoot: process.env.SystemRoot,
    TOKIO_WORKER_THREADS: "2", JEVIA_TEST_BINARY: jevia, FIXTURE_MODE: mode,
    JEVIA_OBSERVATION_EXECUTABLE: mode === "missing" ? join(cwd, "missing-collector") : process.execPath,
    JEVIA_OBSERVATION_JOURNAL: journal,
  };
  await exec(jevia, ["init"], { cwd, env });
  await writeFile(journal, JSON.stringify({ type: "snapshot", event: initial }) + "\n");
  await writeFile(join(cwd, ".jevia/runs.jsonl"), JSON.stringify({
    schema_version: 6, run_id: id, tier: "fast", suggested_tier: "fast", confidence: 0.9,
    probabilities: {}, fallback_applied: false, jev_model: "fixture", created_at_ms: 1,
    source: "live", outcome: "unknown", lifecycle: { state: "running", started_at_ms: 1, finished_at_ms: null },
    execution: { harness: "opencode", model: "fixture", duration_ms: 1, exit_code: null, observations: initial },
  }) + "\n");
  // The plugin launches `node capture-event ...`; this disposable fixture fails
  // model metadata only, while the other events use the real Rust collector.
  await writeFile(join(cwd, "capture-event"), `
const { readFileSync, writeFileSync } = require('node:fs');
const { spawnSync } = require('node:child_process');
const input = readFileSync(0, 'utf8');
const mode = process.env.FIXTURE_MODE;
if (JSON.parse(input).hook_event_name === 'ModelObserved' && mode !== 'success') {
  if (mode === 'timeout') {
    process.on('SIGTERM', () => {});
    writeFileSync('collector.pid', String(process.pid));
    setInterval(() => {}, 1000);
  } else if (mode === 'signal') process.kill(process.pid, 'SIGTERM');
  else process.exit(7);
} else {
  const result = spawnSync(process.env.JEVIA_TEST_BINARY, ['capture-event', ...process.argv.slice(2)], { input, env: process.env });
  process.exit(result.status ?? 1);
}
`);
  const run = () => exec(process.execPath, [runner], { cwd, env, timeout: 12_000 });
  const recover = async () => JSON.parse((await exec(jevia, ["runs", "recover", id, "--confirm-stopped"], { cwd, env, timeout: 10_000 })).stdout);
  return { cwd, journal, marker, run, recover };
}

for (const mode of ["exit", "missing", "signal", "timeout"]) {
  test(`collector ${mode} marks capture partial without failing the harness`, { timeout: 20_000 }, async (t) => {
    const f = await fixture(t, mode);
    const result = await f.run();
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "");
    assert.equal((await lstat(f.marker)).size, 0);
    if (process.platform !== "win32") assert.equal((await lstat(f.marker)).mode & 0o777, 0o600);
    if (mode === "timeout") {
      const pid = Number(await readFile(join(f.cwd, "collector.pid"), "utf8"));
      await delay(100);
      assert.throws(() => process.kill(pid, 0), { code: "ESRCH" });
    }
    const saved = await f.recover();
    const observations = saved.execution.observations;
    assert.equal(observations.status, "partial");
    // Several missing-process errors share one lower-bound marker, not an exact count.
    assert.equal(observations.totals.discarded_inputs, 1);
    if (mode !== "missing") {
      assert.equal(observations.totals.event_counts.session_started, 1);
      assert.equal(observations.totals.event_counts.turn_completed, 1);
    }
    assert.equal(saved.outcome, "unknown");
  });
}

test("successful collection stays recorded without a loss marker", async (t) => {
  const f = await fixture(t, "success");
  await f.run();
  await assert.rejects(lstat(f.marker), { code: "ENOENT" });
  const saved = await f.recover();
  assert.equal(saved.execution.observations.status, "recorded");
  assert.equal(saved.execution.observations.totals.discarded_inputs, 0);
  assert.equal(saved.execution.observations.totals.event_counts.model_observed, 1);
});

for (const kind of ["missing", "foreign", "invalid", "oversized", "invalid-marker", "symlink"]) {
  test(`loss reporting refuses ${kind} files and emits only one redacted warning`, { skip: kind === "symlink" && process.platform === "win32" }, async (t) => {
    const f = await fixture(t, "missing");
    if (kind === "missing") await rm(f.journal);
    if (kind === "foreign") await writeFile(f.journal, JSON.stringify({ type: "snapshot", event: { ...initial, source: "claude_hooks" } }));
    if (kind === "invalid") await writeFile(f.journal, "PRIVATE invalid JSON");
    if (kind === "oversized") await writeFile(f.journal, "PRIVATE" + "x".repeat(512 * 1024));
    if (kind === "invalid-marker") await writeFile(f.marker, "PRIVATE marker");
    if (kind === "symlink") {
      const target = join(f.cwd, "private.jsonl");
      await writeFile(target, await readFile(f.journal));
      await rm(f.journal);
      await symlink(target, f.journal);
    }
    const result = await f.run();
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "jevia: native observation recording could not be confirmed (details redacted)\n");
    if (kind === "invalid-marker") assert.equal(await readFile(f.marker, "utf8"), "PRIVATE marker");
    else await assert.rejects(lstat(f.marker), { code: "ENOENT" });
  });
}
