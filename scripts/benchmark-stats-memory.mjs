// Synthetic local stats benchmark; no provider requests or credentials needed.
// Usage: node scripts/benchmark-stats-memory.mjs BEFORE_BINARY AFTER_BINARY
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

assert.ok(["darwin", "linux"].includes(process.platform), "Requires /usr/bin/time on macOS or Linux");
assert.equal(process.argv.length, 4, "Supply before and after CLI binaries");
const binaries = process.argv.slice(2).map((path) => resolve(path));
const root = await mkdtemp(join(tmpdir(), "jevia-stats-benchmark-"));
const env = { ...process.env, TOKIO_WORKER_THREADS: "2" };
delete env.TYPESAFE_API_KEY;
const exec = promisify(execFile);
const records = 200;
const taskBytes = 512 * 1024;
const results = [];
for (const backend of ["jsonl", "sqlite"]) {
  const cwd = join(root, backend);
  await mkdir(cwd);
  const call = (args) => exec(binaries[0], args, { cwd, env, timeout: 60_000 });
  await call(["init"]);
  if (backend === "sqlite") {
    const config = join(cwd, ".jevia/config.toml");
    await writeFile(config, (await readFile(config, "utf8")) + '\n[storage]\nbackend = "sqlite"\nurl = "sqlite://.jevia/runs.db"\n');
  }
  const original = Array.from({ length: records }, (_, i) => JSON.stringify({
    schema_version: 6, run_id: `stats-${i}`, tier: "fast", suggested_tier: "fast", confidence: 0.9,
    probabilities: {}, fallback_applied: false, jev_model: "fixture", created_at_ms: 1, source: "live",
    task: "x".repeat(taskBytes), outcome: "success", outcome_evidence: { source: "manual", recorded_at_ms: 2 },
    lifecycle: { state: "completed", started_at_ms: null, finished_at_ms: 2 },
  })).join("\n") + "\n";
  await writeFile(join(cwd, ".jevia/runs.jsonl"), original);
  if (backend === "sqlite") {
    await call(["storage", "init"]);
    await call(["storage", "import-jsonl", "--apply"]);
  }
  for (const limit of [20, records]) {
    for (const binary of binaries) {
      const started = performance.now();
      const { stdout, stderr } = await exec("/usr/bin/time", [process.platform === "darwin" ? "-l" : "-v", binary, "stats", "--json", "--limit", String(limit)], { cwd, env, timeout: 60_000 });
      const peakRssBytes = process.platform === "darwin"
        ? Number(stderr.match(/(\d+)\s+maximum resident set size/)[1])
        : Number(stderr.match(/Maximum resident set size \(kbytes\):\s+(\d+)/)[1]) * 1024;
      results.push({ backend, limit, binary, peakRssBytes, elapsedMs: Math.round(performance.now() - started), stats: JSON.parse(stdout) });
    }
    assert.deepEqual(results.at(-1).stats, results.at(-2).stats, "Stats output must be unchanged");
  }
  assert.equal(await readFile(join(cwd, ".jevia/runs.jsonl"), "utf8"), original);
}
const report = { root, synthetic: true, records, taskBytes, results };
await writeFile(join(root, "results.json"), JSON.stringify(report, null, 2));
console.log(JSON.stringify({ ...report, results: results.map(({ stats, ...measurement }) => measurement) }, null, 2));
