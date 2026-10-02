// Actual pinned CLIs, not wrapper scripts. All model responses are loopback fixtures.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

const exec = promisify(execFile);
const jevia = resolve(process.env.JEVIA_TEST_BINARY ?? "target/debug/jevia");
const tools = process.env.JEVIA_NATIVE_BIN_DIR;
const pins = { codex: "codex-cli 0.158.0-alpha.2", opencode: "1.18.33" };

async function fixture(t, harness) {
  const cwd = await mkdtemp(join(tmpdir(), "jevia-native-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const home = join(cwd, "isolated-home");
  const configHome = join(home, "config");
  const agentHome = join(home, "agent");
  await mkdir(agentHome, { recursive: true });
  const requests = [];
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    let input;
    try { input = JSON.parse(Buffer.concat(chunks).toString()); } catch { res.writeHead(400); res.end(); return; }
    requests.push({ path: req.url, input });
    if (req.url === "/v1/systemone") {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ model: "fixture", answers: { tier: {
        type: "choice", choice: "fast", confidence: 0.99,
        probabilities: { fast: 0.99, balanced: 0.005, strong: 0.005 },
      } } }));
    } else if (req.url === "/v1/chat/completions") {
      res.setHeader("content-type", "text/event-stream");
      const first = input.tools?.some((tool) => tool.function?.name === "bash") &&
        requests.filter((r) => r.path === "/v1/chat/completions" && r.input.tools?.length).length === 1;
      const chunks = first
        ? [[{ role: "assistant", tool_calls: [{ index: 0, id: "call_fixture", type: "function", function: { name: "bash", arguments: JSON.stringify({ command: "printf fixture-tool", description: "Print a fixed integration-test marker" }) } }] }, null], [{}, "tool_calls"]]
        : [[{ role: "assistant", content: "fixture response" }, null], [{}, "stop"]];
      for (const [delta, finish_reason] of chunks) {
        res.write(`data: ${JSON.stringify({ id: "chat_fixture", object: "chat.completion.chunk", created: 1, model: "fixture-model", choices: [{ index: 0, delta, finish_reason }] })}\n\n`);
      }
      res.end("data: [DONE]\n\n");
    } else if (req.url === "/v1/responses") {
      res.setHeader("content-type", "text/event-stream");
      const item = { type: "message", id: "msg_fixture", status: "completed", role: "assistant", content: [{ type: "output_text", text: "fixture response", annotations: [] }] };
      const events = [
        { type: "response.created", response: { id: "resp_fixture", status: "in_progress", output: [] } },
        { type: "response.output_item.added", output_index: 0, item: { ...item, status: "in_progress", content: [] } },
        { type: "response.output_text.delta", item_id: item.id, output_index: 0, content_index: 0, delta: "fixture response" },
        { type: "response.output_item.done", output_index: 0, item },
        { type: "response.completed", response: { id: "resp_fixture", status: "completed", output: [item], usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 } } },
      ];
      for (const event of events) res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
      res.end();
    } else { res.writeHead(404); res.end(); }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => new Promise((done) => { server.closeAllConnections(); server.close(done); }));
  const base = `http://127.0.0.1:${server.address().port}/v1`;
  // Allowlist environment: never inherit provider keys, authentication, plugins,
  // hook trust, proxy settings, or the developer's harness configuration.
  const env = {
    PATH: process.env.PATH, HOME: home, USERPROFILE: home,
    TMPDIR: tmpdir(), XDG_CONFIG_HOME: configHome,
    XDG_DATA_HOME: join(home, "data"), XDG_CACHE_HOME: join(home, "cache"),
    XDG_STATE_HOME: join(home, "state"), CODEX_HOME: agentHome,
    TYPESAFE_API_KEY: "loopback-only", TOKIO_WORKER_THREADS: "2",
    OPENCODE_DISABLE_DEFAULT_PLUGINS: "true", OPENCODE_DISABLE_MODELS_FETCH: "true",
    OPENCODE_DISABLE_AUTOUPDATE: "true", NO_COLOR: "1",
  };
  const binary = join(resolve(tools), harness);
  assert.equal((await exec(binary, ["--version"], { cwd, env, timeout: 30_000 })).stdout.trim(), pins[harness]);
  await exec(jevia, ["init"], { cwd, env });
  const config = join(cwd, ".jevia/config.toml");
  let source = (await readFile(config, "utf8")).replace(/base_url = "[^"]+"/, `base_url = "${base.slice(0, -3)}"`);
  const args = harness === "codex"
    ? ["exec", "--skip-git-repo-check", "--sandbox", "read-only", "--model", "{model}", "{task}"]
    : ["run", "--model", "{model}", "{task}"];
  const model = harness === "codex" ? "fixture-model" : "fixture/fixture-model";
  source += `\n[harnesses.${harness}]\ncommand = ${JSON.stringify(binary)}\nargs = ${JSON.stringify(args)}\n[harnesses.${harness}.models]\nfast = "${model}"\nbalanced = "${model}"\nstrong = "${model}"\n`;
  await writeFile(config, source);
  if (harness === "codex") {
    await writeFile(join(agentHome, "config.toml"), `model_provider = "fixture"\nweb_search = "disabled"\n[model_providers.fixture]\nname = "Loopback fixture"\nbase_url = "${base}"\nwire_api = "responses"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\n`);
  } else {
    await mkdir(join(configHome, "opencode"), { recursive: true });
    await writeFile(join(configHome, "opencode", "opencode.json"), JSON.stringify({
      enabled_providers: ["fixture"], autoupdate: false, snapshot: false,
      provider: { fixture: { npm: "@ai-sdk/openai-compatible", name: "Loopback fixture", options: { baseURL: base, apiKey: "loopback-only" }, models: { "fixture-model": { name: "Fixture", limit: { context: 10000, output: 100 } } } } },
    }));
  }
  const result = await exec(jevia, ["run", harness, "Print the fixture-tool marker and reply with fixture response.", "--non-interactive", "--timeout-seconds", "45"], { cwd, env, timeout: 60_000, maxBuffer: 2 * 1024 * 1024 });
  const records = JSON.parse((await exec(jevia, ["runs", "--json"], { cwd, env })).stdout);
  return { cwd, requests, result, record: records[0] };
}

test("real OpenCode loads the injected plugin and records passive model facts", { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 90_000 }, async (t) => {
  const { cwd, requests, result, record } = await fixture(t, "opencode");
  assert.match(result.stdout + result.stderr, /fixture response/);
  assert.ok(requests.some((r) => r.path === "/v1/chat/completions"));
  assert.equal(record.lifecycle.state, "completed");
  assert.equal(record.outcome, "unknown");
  assert.equal(record.execution.verification, undefined);
  const observations = record.execution.observations;
  assert.equal(observations.source, "opencode_plugin");
  assert.equal(observations.status, "recorded");
  assert.ok(observations.events.some((e) => e.kind === "turn_started"));
  assert.ok(observations.events.some((e) => e.kind === "model_observed" && e.model === "fixture/fixture-model"));
  assert.ok(observations.events.some((e) => e.kind === "tool_completed" && e.tool_name === "bash"));
  assert.ok(!observations.events.some((e) => e.kind === "tool_succeeded"));
  assert.ok(!JSON.stringify(observations).includes("fixture response"));
  assert.ok(!(await readdir(join(cwd, ".jevia"))).some((name) => name.startsWith("jevia-events-")));
});

test("real Codex accepts injected hooks but preserves normal trust review", { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 90_000 }, async (t) => {
  const { requests, result, record } = await fixture(t, "codex");
  assert.ok(requests.some((r) => r.path === "/v1/responses"));
  assert.match(result.stdout + result.stderr, /fixture response/);
  assert.equal(record.execution.exit_code, 0);
  assert.equal(record.outcome, "unknown");
  assert.equal(record.execution.verification, undefined);
  assert.equal(record.execution.observations.source, "codex_hooks");
  assert.equal(record.execution.observations.status, "no_events");
  assert.deepEqual(record.execution.observations.events, []);
  assert.match(result.stderr, /observations=no_events events=0/);
  assert.match(result.stderr, /no native events received/);
  assert.doesNotMatch(result.stderr, /observations=recorded/);
  // This is a negative coverage test, not proof that trusted hooks dispatch.
  // scripts/live-harness-smoke.mjs requires real tool and turn events to pass.
});
