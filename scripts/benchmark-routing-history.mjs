// Synthetic loopback benchmark, NOT live-provider or model-quality evidence.
// Usage: node scripts/benchmark-routing-history.mjs BEFORE_BINARY AFTER_BINARY
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

assert.ok(["darwin", "linux"].includes(process.platform), "Requires /usr/bin/time on macOS or Linux");
assert.equal(process.argv.length, 4, "Supply before and after CLI binaries");
const binaries = process.argv.slice(2).map((path) => resolve(path));
const root = await mkdtemp(join(tmpdir(), "jevia-history-benchmark-"));
const env = { ...process.env, TYPESAFE_API_KEY: "synthetic-loopback-only", TOKIO_WORKER_THREADS: "2" };
const exec = promisify(execFile);
const requests = [];
const server = createServer(async (request, response) => {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  const bytes = Buffer.concat(chunks);
  requests.push({ bytes: bytes.length, hash: createHash("sha256").update(bytes).digest("hex"), budget: JSON.parse(bytes).state.history_budget });
  response.end(JSON.stringify({ model: "fixture", answers: { tier: { type: "choice", choice: "fast", confidence: 0.99 } } }));
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const results = [];
try {
  for (const backend of ["jsonl", "sqlite"]) {
    for (const [index, binary] of binaries.entries()) {
      const cwd = join(root, `${backend}-${index}`);
      await mkdir(cwd);
      const call = (args) => exec(binary, args, { cwd, env, timeout: 60_000 });
      await call(["init"]);
      const configPath = join(cwd, ".jevia/config.toml");
      let config = (await readFile(configPath, "utf8"))
        .replace(/base_url = "[^"]+"/, `base_url = "http://127.0.0.1:${server.address().port}"`)
        .replace(/history_limit = \d+/, "history_limit = 100");
      if (backend === "sqlite") config += '\n[storage]\nbackend = "sqlite"\nurl = "sqlite://.jevia/runs.db"\n';
      await writeFile(configPath, config);
      const rows = Array.from({ length: 100 }, (_, i) => ({
        schema_version: 6, run_id: `history-${i}`, tier: "fast", suggested_tier: "fast", confidence: 0.9,
        probabilities: {}, fallback_applied: false, jev_model: "fixture", created_at_ms: 1, source: "live",
        task: "x".repeat(512 * 1024), outcome: "success", outcome_evidence: { source: "manual", recorded_at_ms: 2 },
        lifecycle: { state: "completed", started_at_ms: null, finished_at_ms: 2 },
      }));
      const original = rows.map((r) => JSON.stringify(r)).join("\n") + "\n";
      await writeFile(join(cwd, ".jevia/runs.jsonl"), original);
      if (backend === "sqlite") {
        await call(["storage", "init"]);
        await call(["storage", "import-jsonl", "--apply"]);
      }
      const { stderr } = await exec("/usr/bin/time", [process.platform === "darwin" ? "-l" : "-v", binary, "route", "Small task", "--json", "--no-cache"], { cwd, env, timeout: 60_000 });
      const rss = process.platform === "darwin" ? Number(stderr.match(/(\d+)\s+maximum resident set size/)[1])
        : Number(stderr.match(/Maximum resident set size \(kbytes\):\s+(\d+)/)[1]) * 1024;
      assert.ok((await readFile(join(cwd, ".jevia/runs.jsonl"), "utf8")).startsWith(original), "Source history must not be truncated");
      results.push({ backend, binary, peakRssBytes: rss, request: requests.at(-1) });
    }
    assert.deepEqual(results.at(-1).request, results.at(-2).request, "Optimization must preserve the entire provider request");
  }
} finally { server.closeAllConnections(); server.close(); }
const report = { root, synthetic: true, records: 100, taskBytes: 512 * 1024, results };
await writeFile(join(root, "results.json"), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
