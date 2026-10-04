// Opt-in, paid real-provider check. No synthetic events or model responses.
// Keeps the disposable project's real recording for inspection, even on failure.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const harness = process.argv[2];
assert.ok(["claude", "codex", "opencode"].includes(harness), "Usage: node scripts/live-harness-smoke.mjs claude|codex|opencode");
assert.equal(process.env.JEVIA_LIVE_TEST, "1", "Set JEVIA_LIVE_TEST=1 to authorize live, potentially paid API calls");
assert.ok(process.env.TYPESAFE_API_KEY, "TYPESAFE_API_KEY must be supplied through the environment");
const binary = resolve(process.env.JEVIA_TEST_BINARY ?? "target/release/jevia");
// Ignoring user config also ignores persisted hook trust. Use an explicitly
// selected profile so normal review works without loading the everyday profile.
const childEnv = { ...process.env };
if (harness === "codex") {
  assert.ok(process.env.JEVIA_LIVE_CODEX_PROFILE, "Set JEVIA_LIVE_CODEX_PROFILE to an isolated profile with your authorized authentication; review Jevia hooks there first (use --prepare)");
  childEnv.CODEX_HOME = resolve(process.env.JEVIA_LIVE_CODEX_PROFILE);
}
const model = harness === "claude"
  ? process.env.JEVIA_LIVE_CLAUDE_MODEL ?? "sonnet"
  : harness === "codex" ? process.env.JEVIA_LIVE_CODEX_MODEL : process.env.JEVIA_LIVE_OPENCODE_MODEL;
assert.ok(model, "Set JEVIA_LIVE_CODEX_MODEL or JEVIA_LIVE_OPENCODE_MODEL to an available model");
const cwd = mkdtempSync(join(tmpdir(), "jevia-live-smoke-"));
const snapshot = join(cwd, ".jevia", "runs.jsonl");
console.log(`Disposable project: ${cwd}`);
console.log(`Actual recording: ${snapshot}`);

function call(program, args, timeout = 150_000) {
  return spawnSync(program, args, { cwd, env: childEnv, encoding: "utf8", timeout, maxBuffer: 2 * 1024 * 1024 });
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
if (harness === "opencode") {
  writeFileSync(join(cwd, "opencode.json"), JSON.stringify({
    autoupdate: false, share: "disabled",
    permission: { "*": "deny", read: "allow", external_directory: "deny" },
  }));
}
const extra = harness === "claude"
  ? ["--allowedTools", "Read", "--max-budget-usd", "0.50", "--no-session-persistence"]
  : harness === "codex" ? ["--sandbox", "read-only", "--ephemeral"] : [];
if (process.argv.includes("--prepare")) {
  console.log("Prepared only: no Jev request or model task was submitted.");
  if (harness === "codex") {
    console.log("In this project, use the selected isolated profile to run `jevia harness review codex --launch`. Review only Jevia's capture-event hooks, then quit and rerun this smoke test without --prepare using the same profile and Jevia executable.");
  }
  process.exit(0);
}
// Deliberately do not bypass hook trust. Missing events must fail this check.
const result = call(binary, ["run", harness,
  "Read input.txt using your file-reading tool. Return the checksum label and arithmetic result. Do not modify files, inspect other directories, use the network, or spawn agents.",
  "--non-interactive", "--timeout-seconds", "90", "--no-cache", "--explain", "--", ...extra]);
// Jevia's own redacted diagnostics explain setup failures without echoing
// arbitrary provider logs, shell commands, or local configuration.
for (const line of (result.stderr ?? "").split("\n")) {
  if (/^jevia: (native |observations=|explain )/.test(line)) console.log(line);
}
assert.ok(existsSync(snapshot), `No run was persisted; routing/setup failed (exit ${result.status}, error ${result.error?.code ?? "none"}). No native-capture success is claimed.`);
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
checked(binary, ["harness", "health", harness, "--require-events", "--json"]);
// A second real classification must automatically reuse the recorded facts;
// do not add feedback or a verifier to make the history eligible.
const followup = call(binary, ["route", "Read input.txt and check its arithmetic again", "--no-cache", "--explain", "--json"]);
assert.equal(followup.status, 0, "Live follow-up routing failed");
assert.match(followup.stderr, /known_outcomes=0 passive_observations=1/);
const followupRecord = JSON.parse(followup.stdout);
assert.equal(followupRecord.source, "live");
assert.notEqual(followupRecord.run_id, record.run_id);
console.log(`Follow-up routing: ${followupRecord.run_id}; automatically loaded one passive observation and zero inferred outcomes.`);
console.log("PASS: live routing, real tool/turn events, persisted recording, harness health, and automatic history reuse verified.");
