// Opt-in, paid real-provider check. No synthetic events or model responses.
// Keeps the disposable project's real recording for inspection, even on failure.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const harness = process.argv[2];
assert.ok(["claude", "codex"].includes(harness), "Usage: node scripts/live-harness-smoke.mjs claude|codex");
assert.equal(process.env.JEVIA_LIVE_TEST, "1", "Set JEVIA_LIVE_TEST=1 to authorize live, potentially paid API calls");
assert.ok(process.env.TYPESAFE_API_KEY, "TYPESAFE_API_KEY must be supplied through the environment");
const binary = resolve(process.env.JEVIA_TEST_BINARY ?? "target/release/jevia");
const model = harness === "claude"
  ? process.env.JEVIA_LIVE_CLAUDE_MODEL ?? "sonnet"
  : process.env.JEVIA_LIVE_CODEX_MODEL;
assert.ok(model, "Set JEVIA_LIVE_CODEX_MODEL to a model available to your Codex account");
const cwd = mkdtempSync(join(tmpdir(), "jevia-live-smoke-"));
const snapshot = join(cwd, ".jevia", "runs.jsonl");
console.log(`Disposable project: ${cwd}`);
console.log(`Actual recording: ${snapshot}`);

function call(program, args, timeout = 150_000) {
  return spawnSync(program, args, { cwd, env: process.env, encoding: "utf8", timeout, maxBuffer: 2 * 1024 * 1024 });
}
function checked(program, args) {
  const result = call(program, args);
  // Do not echo arbitrary provider stderr, credentials, or local configuration.
  assert.equal(result.status, 0, `Setup failed (exit ${result.status}, error ${result.error?.code ?? "none"})`);
}
checked("git", ["init", "-q"]);
checked(binary, ["init"]);
checked(binary, ["harness", "setup", harness, "--preset", harness, "--apply",
  ...["fast", "balanced", "strong"].flatMap((tier) => ["--model", `${tier}=${model}`])]);
writeFileSync(join(cwd, "input.txt"), "The checksum label is JEVIA-LIVE-37. Compute 17 + 25.\n");
const extra = harness === "claude"
  ? ["--allowedTools", "Read", "--max-budget-usd", "0.50", "--no-session-persistence"]
  : ["--sandbox", "read-only", "--ignore-user-config", "--ephemeral"];
// Deliberately do not bypass hook trust. Missing events must fail this check.
const result = call(binary, ["run", harness,
  "Read input.txt using your file-reading tool. Return the checksum label and arithmetic result. Do not modify files, inspect other directories, use the network, or spawn agents.",
  "--non-interactive", "--timeout-seconds", "90", "--no-cache", "--explain", "--", ...extra]);
const records = readFileSync(snapshot, "utf8").trim().split("\n").map(JSON.parse);
const record = records.at(-1);
const observations = record.execution?.observations;
console.log(JSON.stringify({ run_id: record.run_id, exit_code: record.execution?.exit_code,
  outcome: record.outcome, observations }, null, 2));
assert.equal(result.status, 0, "Real harness did not exit successfully");
assert.equal(record.source, "live", "Routing must use the live provider");
assert.equal(record.lifecycle?.state, "completed");
assert.equal(record.outcome, "unknown", "Normal process exit must not infer task success");
assert.equal(observations?.status, "recorded", "Native capture is not active; inspect harness health and hook trust");
assert.ok(observations.events.some((event) => ["tool_succeeded", "tool_completed"].includes(event.kind)),
  "The recording must contain a real tool event, not only session events");
assert.ok(observations.events.some((event) => event.kind === "turn_completed"), "Missing turn completion");
assert.match(result.stdout, /JEVIA-LIVE-37/);
assert.match(result.stdout, /\b42\b/);
console.log("PASS: live routing, real tool event, turn completion, and persisted recording verified.");
