import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { JeviaClient, JeviaCommandError } from "../dist/index.js";

// Keep fixture startup/OS cleanup on real time while controlling only the SDK
// timers. Capture the function before MockTimers replaces the global binding.
const realSetTimeout = globalThis.setTimeout;
const delay = (ms) => new Promise(resolve => realSetTimeout(resolve, ms));

async function until(check, timeout = 10000) {
  const start = Date.now();
  while (!(await check())) {
    assert.ok(Date.now() - start < timeout, "condition did not become true");
    await delay(20);
  }
}

async function stubborn(t, timeoutMs, startupDelayMs = 0) {
  const dir = await mkdtemp(join(tmpdir(), "jevia-deadline-"));
  const pids = join(dir, "pids");
  const heartbeat = join(dir, "heartbeat");
  const descendant = `
    const fs = require('node:fs');
    process.on('SIGTERM', () => {});
    fs.appendFileSync(${JSON.stringify(pids)}, process.pid + String.fromCharCode(10));
    setInterval(() => fs.writeFileSync(${JSON.stringify(heartbeat)}, String(Date.now())), 20);
    setTimeout(() => process.exit(0), 15000);
  `;
  const script = `
    require('node:fs').appendFileSync(${JSON.stringify(pids)}, process.pid + String.fromCharCode(10));
    process.on('SIGTERM', () => {});
    setTimeout(() => require('node:child_process').spawn(process.execPath, ['-e', ${JSON.stringify(descendant)}], { stdio: 'ignore' }), ${startupDelayMs});
    setTimeout(() => process.exit(0), 15000);
  `;
  t.after(async () => {
    const owned = await readFile(pids, "utf8").catch(() => "");
    for (const pid of owned.trim().split("\n").map(Number).filter(n => n > 0)) {
      try { process.kill(pid, "SIGKILL"); } catch { /* already reaped */ }
    }
    await rm(dir, { recursive: true, force: true });
  });
  return {
    client: new JeviaClient({ binary: process.execPath, binaryArgs: ["-e", script, "--"], timeoutMs }),
    ready: async () => {
      await until(async () => (await readFile(heartbeat, "utf8").catch(() => "")) !== "");
      assert.equal((await readFile(pids, "utf8")).trim().split("\n").length, 2, "both processes must exist before testing tree cleanup");
    },
    stopped: async () => {
      await delay(350);
      const before = await readFile(heartbeat, "utf8");
      await delay(150);
      assert.equal(await readFile(heartbeat, "utf8"), before, "descendant survived cleanup");
    },
  };
}

test("timeout rejects on deadline and cleans a SIGTERM-resistant process tree", async (t) => {
  // Deliberately start the descendant later than the SDK deadline: startup
  // speed must not decide whether this process-tree regression can run.
  const f = await stubborn(t, 1000, 1200);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let settled = false;
  const failed = assert.rejects(f.client.route("private task"), error => {
    settled = true;
    assert.ok(error instanceof JeviaCommandError);
    assert.equal(error.message, "Jevia command timed out");
    return true;
  });
  await f.ready();
  // Real startup time cannot consume the simulated deadline. Exercise both
  // sides of the deadline after proving the resistant descendant is running.
  t.mock.timers.tick(999);
  await Promise.resolve();
  assert.equal(settled, false);
  const started = performance.now();
  t.mock.timers.tick(1);
  await failed;
  assert.ok(performance.now() - started < 500, "waited for wrapper exit instead of deadline");
  t.mock.timers.tick(100); // Exercise real SIGKILL escalation on Unix.
  t.mock.timers.reset();
  await f.stopped();
});

test("a real startup deadline rejects without requiring descendant readiness", async (t) => {
  const f = await stubborn(t, 100, 1500);
  const started = performance.now();
  await assert.rejects(f.client.route("task"), error => {
    assert.ok(error instanceof JeviaCommandError);
    assert.equal(error.kind, "timeout");
    return true;
  });
  assert.ok(performance.now() - started < 2500, "startup must remain deadline-bounded");
});

test("abort rejects promptly and cleans the process tree", async (t) => {
  const f = await stubborn(t, 10000);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const controller = new AbortController();
  const failed = assert.rejects(f.client.route("task", { signal: controller.signal }), /Jevia command aborted/);
  await f.ready();
  const started = performance.now();
  controller.abort("private reason");
  await failed;
  assert.ok(performance.now() - started < 500);
  t.mock.timers.tick(100);
  t.mock.timers.reset();
  await f.stopped();
});

test("pre-aborted requests do not attempt to spawn even an invalid binary", async () => {
  const client = new JeviaClient({ binary: "missing-jevia-binary" });
  await assert.rejects(client.version({ signal: AbortSignal.abort("private reason") }), /Jevia command aborted/);
});

test("rejects timeout values that Node would silently clamp to one millisecond", () => {
  for (const timeoutMs of [0, -1, 1.1, Infinity, 2147483648]) {
    assert.throws(() => new JeviaClient({ timeoutMs }), TypeError);
  }
});

test("output limits are byte-based and apply independently to stdout and stderr", async () => {
  for (const stream of ["stdout", "stderr"]) {
    const client = new JeviaClient({
      binary: process.execPath,
      binaryArgs: ["-e", `process.${stream}.write('é'.repeat(10));`, "--"],
      maxBufferBytes: 10,
    });
    await assert.rejects(client.version(), error => {
      assert.ok(error instanceof JeviaCommandError);
      assert.equal(error[stream], "é".repeat(5));
      return true;
    });
  }
});
