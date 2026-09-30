import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { inspect, promisify } from "node:util";
import { JeviaClient, JeviaCommandError } from "../dist/index.js";

const execute = promisify(execFile);
const binary = process.env.JEVIA_TEST_BINARY ?? fileURLToPath(new URL(
  `../../../target/debug/jevia${process.platform === "win32" ? ".exe" : ""}`, import.meta.url,
));
const record = {
  schema_version: 3, run_id: "storage-test", tier: "fast", suggested_tier: "fast",
  confidence: 0.9, probabilities: { fast: 0.9 }, fallback_applied: false, jev_model: "test",
  created_at_ms: 1, source: "live", task: "test task", outcome: "unknown",
};

async function fixture(t, env = {}) {
  const cwd = await mkdtemp(join(tmpdir(), "jevia-node-storage-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const client = new JeviaClient({ binary, cwd, env: { TOKIO_WORKER_THREADS: "2", ...env } });
  assert.deepEqual(await readdir(cwd), [], "constructor must not create project files");
  await execute(binary, ["init"], { cwd, timeout: 10_000 });
  const configPath = join(cwd, ".jevia/config.toml");
  const config = await readFile(configPath, "utf8");
  return { cwd, client, configPath, config };
}

async function migrateAndVerify(t, target, env) {
  const { cwd, client, configPath, config } = await fixture(t, env);
  const historyPath = join(cwd, ".jevia/runs.jsonl");
  const history = `${JSON.stringify(record)}\n`;
  await writeFile(historyPath, history);
  const before = await readdir(join(cwd, ".jevia"));
  const preview = await client.setupStorage(target, { importJsonl: true });
  assert.match(preview, /No files changed or database connection attempted/);
  assert.equal(await readFile(configPath, "utf8"), config);
  assert.deepEqual(await readdir(join(cwd, ".jevia")), before);
  await assert.rejects(client.setupStorage(target, { apply: true, confirmStopped: true }), JeviaCommandError);
  assert.equal(await readFile(configPath, "utf8"), config);
  const report = await client.setupStorage(target, { apply: true, confirmStopped: true, importJsonl: true });
  assert.match(report, /1 imported/);
  assert.notEqual(await readFile(configPath, "utf8"), config);
  assert.equal(await readFile(historyPath, "utf8"), history);
  assert.equal((await client.runs()).length, 1);
  assert.equal((await client.show(record.run_id)).task, record.task);
  assert.equal((await client.feedback(record.run_id, "success")).outcome, "success");
  const done = await client.complete(record.run_id, "success", { confirmStopped: true });
  assert.equal(done.lifecycle.state, "completed");
  assert.equal(done.outcome_evidence.source, "manual");
  assert.equal(await readFile(historyPath, "utf8"), history, "feedback must use SQL, not retained JSONL");
  assert.match(await client.checkStorage(), new RegExp(`backend=${target.backend}, records=1`));
  assert.match(await client.checkStorage({ deep: true }), /check=deep/);
}

test("SQLite setup previews, imports explicitly, and uses selected storage for SDK calls", async (t) => {
  await migrateAndVerify(t, { backend: "sqlite", path: ".jevia/sdk history.db" });
});

test("failed setup preserves config and does not expose database credentials", async (t) => {
  const secret = "PRIVATE_DATABASE_SENTINEL";
  const { client, config, configPath } = await fixture(t, { SDK_TEST_DB: `invalid://${secret}` });
  const target = { backend: "postgres", project: "sdk-test", urlEnv: "SDK_TEST_DB" };
  assert.match(await client.setupStorage(target), /No files changed or database connection attempted/);
  await assert.rejects(client.setupStorage(target, { apply: true, confirmStopped: true }), (error) => {
    assert.ok(error instanceof JeviaCommandError);
    assert.ok(!inspect(error).includes(secret));
    assert.ok(!error.stderr.includes(secret));
    assert.ok(!error.command.join(" ").includes(secret));
    return true;
  });
  assert.equal(await readFile(configPath, "utf8"), config);
});

test("checks never initialize a missing SQLite database", async (t) => {
  const { cwd, client, config, configPath } = await fixture(t);
  await writeFile(configPath, `${config}\n[storage]\nbackend = "sqlite"\nurl = "sqlite://.jevia/missing.db"\n`);
  assert.notEqual(await readFile(configPath, "utf8"), config);
  await assert.rejects(client.checkStorage(), JeviaCommandError);
  await assert.rejects(client.checkStorage({ deep: true }), JeviaCommandError);
  await assert.rejects(readFile(join(cwd, ".jevia/missing.db")), { code: "ENOENT" });
});

test("PostgreSQL setup imports and checks a project through the SDK", {
  skip: !process.env.JEVIA_TEST_POSTGRES_URL && "requires isolated PostgreSQL test database",
}, async (t) => {
  await migrateAndVerify(t, { backend: "postgres", project: `sdk-${randomUUID()}`, urlEnv: "SDK_TEST_DB", allowInsecureLocalhost: true }, {
    SDK_TEST_DB: process.env.JEVIA_TEST_POSTGRES_URL,
  });
});
