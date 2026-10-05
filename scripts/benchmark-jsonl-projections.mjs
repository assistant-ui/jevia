// Synthetic loopback benchmark, not model-quality or live-provider evidence.
// Build both binaries in release mode. Alternates measurements to reduce warmup/order bias.
// Usage: node scripts/benchmark-jsonl-projections.mjs BEFORE_BINARY AFTER_BINARY
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

assert.equal(process.argv.length, 4, "Supply before and after release binaries");
const binaries = process.argv.slice(2).map((path) => resolve(path));
const root = await mkdtemp(join(tmpdir(), "jevia-jsonl-projections-"));
const exec = promisify(execFile);
const env = { ...process.env, TYPESAFE_API_KEY: "synthetic-loopback-only", TOKIO_WORKER_THREADS: "2" };
let requestHash;
const server = createServer(async (request, response) => {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  requestHash = createHash("sha256").update(Buffer.concat(chunks)).digest("hex");
  response.end(JSON.stringify({ model: "fixture", answers: { tier: { type: "choice", choice: "fast", confidence: 0.99 } } }));
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const call = (binary, cwd, args) => exec(binary, args, { cwd, env, timeout: 60_000 });
const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
const cases = [];
try {
  for (const count of [20, 10_000]) {
    const rows = Array.from({ length: count }, (_, i) => ({
      schema_version: 6, run_id: `fixture-${i}`, tier: "fast", suggested_tier: "fast", confidence: 0.9,
      probabilities: {}, fallback_applied: false, jev_model: "fixture", created_at_ms: i, source: "live",
      task: "x".repeat(1024), outcome: i % 2 ? "unknown" : "success",
      ...(i % 2 ? {} : { outcome_evidence: { source: "manual", recorded_at_ms: i } }),
      lifecycle: { state: "completed", started_at_ms: i, finished_at_ms: i },
      execution: { harness: "fixture", model: "requested", duration_ms: 1, exit_code: 0,
        observations: { source: "application", status: "recorded",
          events: Array.from({ length: 32 }, (_, n) => ({ kind: "tool_completed", recorded_at_ms: n, model: `m${n % 4}` })),
          totals: { event_counts: { tool_completed: 64 },
            models: Object.fromEntries(Array.from({ length: 4 }, (_, n) => [`m${n}`, { tool_completed: 16 }])),
            unattributed_event_counts: {}, omitted_model_event_counts: {}, models_truncated: false, discarded_inputs: 0 },
        },
      },
    }));
    const original = rows.map((row) => JSON.stringify(row)).join("\n") + "\n";
    const projects = [];
    for (const [index, binary] of binaries.entries()) {
      const cwd = join(root, `${count}-${index}`);
      await mkdir(cwd);
      await call(binary, cwd, ["init"]);
      const path = join(cwd, ".jevia/config.toml");
      const config = (await readFile(path, "utf8"))
        .replace(/base_url = "[^"]+"/, `base_url = "http://127.0.0.1:${server.address().port}"`)
        .replace(/history_limit = \d+/, "history_limit = 20");
      await writeFile(path, config);
      await writeFile(join(cwd, ".jevia/runs.jsonl"), original);
      projects.push(cwd);
    }
    const timings = [[], []];
    let expectedHash;
    for (let round = 0; round < 7; round++) {
      for (const index of round % 2 ? [1, 0] : [0, 1]) {
        const started = performance.now();
        await call(binaries[index], projects[index], ["route", "Synthetic projection probe", "--no-cache", "--json"]);
        if (round > 1) timings[index].push(Number((performance.now() - started).toFixed(2)));
        expectedHash ??= requestHash;
        assert.equal(requestHash, expectedHash, "Provider request bytes changed");
      }
    }
    for (const cwd of projects) {
      assert.ok((await readFile(join(cwd, ".jevia/runs.jsonl"), "utf8")).startsWith(original), "Source history changed");
    }
    cases.push({ records: count, historyBytes: Buffer.byteLength(original), historyLimit: 20, requestHash: expectedHash,
      beforeMs: timings[0], afterMs: timings[1], beforeMedianMs: median(timings[0]), afterMedianMs: median(timings[1]) });
  }
} finally { server.closeAllConnections(); server.close(); }
const report = { synthetic: true, root, binaries, cases };
await writeFile(join(root, "results.json"), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
