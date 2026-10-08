// Synthetic cleanup-preview benchmark. No harnesses, providers, or cleanup moves.
// Usage: node scripts/benchmark-recording-cleanup-memory.mjs BEFORE_BINARY AFTER_BINARY
// Build both binaries with identical profiles. Results are not a CI RSS threshold.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { appendFile, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

assert.ok(["darwin", "linux"].includes(process.platform), "Requires /usr/bin/time on macOS or Linux");
assert.equal(process.argv.length, 4, "Supply before and after CLI binaries");
const binaries = process.argv.slice(2).map((path) => resolve(path));
const root = await mkdtemp(join(tmpdir(), "jevia-cleanup-memory-"));
const exec = promisify(execFile);
const env = { ...process.env, TOKIO_WORKER_THREADS: "2" };
delete env.TYPESAFE_API_KEY;
const taskBytes = 2 * 1024 * 1024;
const results = [];
for (const records of [1, 32]) {
  const cwd = join(root, String(records));
  await mkdir(cwd);
  await exec(binaries[0], ["init"], { cwd, env, timeout: 30_000 });
  const history = join(cwd, ".jevia/runs.jsonl");
  const observations = { source: "claude_hooks", status: "no_events", events: [] };
  const checkpoint = JSON.stringify({ type: "snapshot", event: observations });
  const artifacts = [];
  for (let i = 0; i < records; i++) {
    const id = randomUUID();
    await appendFile(history, JSON.stringify({
      schema_version: 6, run_id: id, tier: "fast", suggested_tier: "fast", confidence: 0.9,
      probabilities: {}, fallback_applied: false, jev_model: "fixture", created_at_ms: 1,
      source: "live", task: "x".repeat(taskBytes), outcome: "unknown",
      lifecycle: { state: "completed", started_at_ms: 1, finished_at_ms: 2 },
      execution: { harness: "fixture", model: "requested", duration_ms: 1, exit_code: 0, observations },
    }) + "\n");
    const artifact = join(cwd, `.jevia/.jevia-event-checkpoint-${id}-fixture`);
    await writeFile(artifact, checkpoint);
    artifacts.push(artifact);
  }
  const original = await readFile(history);
  // Prime stable lock sidecars before comparing inventory counts, and warm both
  // binaries equally. The measured commands still run in fresh processes.
  for (const binary of binaries) {
    await exec(binary, ["recordings", "cleanup", "--json"], { cwd, env, timeout: 60_000 });
  }
  let expected;
  for (let round = 0; round < 3; round++) {
    for (const index of round % 2 ? [1, 0] : [0, 1]) {
      const started = performance.now();
      const { stdout, stderr } = await exec("/usr/bin/time", [
        process.platform === "darwin" ? "-l" : "-v", binaries[index], "recordings", "cleanup", "--json",
      ], { cwd, env, timeout: 60_000 });
      const report = JSON.parse(stdout);
      assert.equal(report.eligible, records);
      assert.equal(report.moved, 0);
      expected ??= report;
      assert.deepEqual(report, expected, "Cleanup decisions must be unchanged");
      const peakRssBytes = process.platform === "darwin"
        ? Number(stderr.match(/(\d+)\s+maximum resident set size/)[1])
        : Number(stderr.match(/Maximum resident set size \(kbytes\):\s+(\d+)/)[1]) * 1024;
      results.push({ records, binary: index === 0 ? "before" : "after", round, peakRssBytes,
        elapsedMs: Math.round(performance.now() - started) });
    }
  }
  assert.deepEqual(await readFile(history), original, "Source history must be unchanged");
  for (const artifact of artifacts) assert.equal(await readFile(artifact, "utf8"), checkpoint);
}
const report = { root, synthetic: true, binaries, taskBytes, results };
await writeFile(join(root, "results.json"), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
