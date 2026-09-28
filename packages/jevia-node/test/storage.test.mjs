import assert from "node:assert/strict";
import test from "node:test";
import { JeviaClient, JeviaCommandError } from "../dist/index.js";

const client = new JeviaClient({
  binary: process.execPath,
  binaryArgs: ["-e", "console.log(JSON.stringify(process.argv.slice(1)))", "--"],
  env: { JEVIA_DATABASE_URL: "postgres://PRIVATE_DATABASE_SENTINEL" },
});
const args = async (promise) => JSON.parse(await promise);

test("storage setup previews by default and applies only with confirmation", async () => {
  assert.deepEqual(await args(client.setupStorage({ backend: "sqlite" })), ["storage", "setup", "sqlite"]);
  assert.deepEqual(await args(client.setupStorage({ backend: "sqlite", path: "--help with spaces.db" }, {
    apply: true, confirmStopped: true, importJsonl: true,
  })), ["storage", "setup", "sqlite", "--path=--help with spaces.db", "--import-jsonl", "--apply", "--confirm-stopped"]);
  for (const options of [{ apply: true }, { confirmStopped: true }, { apply: true, confirmStopped: false }, { apply: "true" }, { importJsonl: "true" }]) {
    await assert.rejects(client.setupStorage({ backend: "sqlite" }, options), TypeError);
  }
});

test("PostgreSQL arguments contain only an environment name and project scope", async () => {
  assert.deepEqual(await args(client.setupStorage({ backend: "postgres", project: "app" })), [
    "storage", "setup", "postgres", "--project=app", "--url-env=JEVIA_DATABASE_URL",
  ]);
  assert.deepEqual(await args(client.setupStorage({ backend: "postgres", project: "--help", urlEnv: "APP_DB", allowInsecureLocalhost: true })), [
    "storage", "setup", "postgres", "--project=--help", "--url-env=APP_DB", "--allow-insecure-localhost",
  ]);
});

test("storage options fail closed without echoing credentials", async () => {
  const secret = "postgres://PRIVATE_DATABASE_SENTINEL";
  for (const target of [
    null, [], {}, { backend: "jsonl" }, { backend: "sqlite", path: "" },
    { backend: "sqlite", path: "a\0b" }, { backend: "sqlite", path: ":memory:" },
    { backend: "sqlite", path: "db?mode=memory" },
    { backend: "postgres", project: secret }, { backend: "postgres", project: "" },
    { backend: "postgres", project: "app", urlEnv: secret },
    { backend: "postgres", project: "app", url: secret },
    { backend: "postgres", project: "app", allowInsecureLocalhost: "true" },
  ]) {
    await assert.rejects(client.setupStorage(target), (error) => {
      assert.ok(error instanceof TypeError);
      assert.ok(!String(error).includes(secret));
      return true;
    });
  }
});

test("storage checks are explicit and support deep validation and cancellation", async () => {
  assert.deepEqual(await args(client.checkStorage()), ["storage", "check"]);
  assert.deepEqual(await args(client.checkStorage({ deep: true })), ["storage", "check", "--deep"]);
  await assert.rejects(client.checkStorage({ deep: "true" }), TypeError);
  const signal = AbortSignal.abort();
  await assert.rejects(client.checkStorage({ signal }), JeviaCommandError);
  await assert.rejects(client.setupStorage({ backend: "sqlite" }, { signal }), JeviaCommandError);
});
