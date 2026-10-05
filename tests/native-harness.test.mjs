// Actual pinned CLIs, not wrapper scripts. All model responses are loopback fixtures.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { once } from "node:events";
import { cp, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

const exec = promisify(execFile);
const jevia = resolve(process.env.JEVIA_TEST_BINARY ?? "target/debug/jevia");
const tools = process.env.JEVIA_NATIVE_BIN_DIR;
const pins = { codex: "codex-cli 0.158.0-alpha.2", opencode: "1.18.33" };

async function fixture(t, harness, trusted = false, scenario = "complete", task = "- Print the fixture-tool marker and reply with fixture response.") {
  const cwd = await mkdtemp(join(tmpdir(), "jevia-native-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const home = join(cwd, "isolated-home");
  const configHome = join(home, "config");
  const agentHome = join(home, "agent");
  await mkdir(agentHome, { recursive: true });
  const requests = [];
  let activeRun;
  let cancellationSent = false;
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
    } else if (scenario === "cancel") {
      // Reach the actual provider boundary before interrupting Jevia. This is
      // not a synthetic recording event or a timing guess during startup.
      res.writeHead(200, { "content-type": "text/event-stream" });
      res.write(": waiting for a response\n\n");
      // Title and main-model requests may race. Simulate one user interrupt.
      if (!cancellationSent) {
        cancellationSent = true;
        activeRun.kill("SIGINT");
      }
    } else if (scenario === "provider_error") {
      res.writeHead(401, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { message: "fixture-provider-rejected", type: "authentication_error", code: "invalid_api_key" } }));
    } else if (req.url === "/v1/chat/completions") {
      res.setHeader("content-type", "text/event-stream");
      const first = input.tools?.some((tool) => tool.function?.name === "bash") &&
        requests.filter((r) => r.path === "/v1/chat/completions" && r.input.tools?.length).length === 1;
      const tool = scenario === "subagent"
        ? { name: "task", arguments: JSON.stringify({ description: "Return a fixed marker", prompt: "Reply with fixture child response.", subagent_type: "fixture-child" }) }
        : { name: "bash", arguments: JSON.stringify({ command: "printf fixture-tool", description: "Print a fixed integration-test marker" }) };
      const chunks = first
        ? [[{ role: "assistant", tool_calls: [{ index: 0, id: "call_fixture", type: "function", function: tool }] }, null], [{}, "tool_calls"]]
        : [[{ role: "assistant", content: input.model === "fixture-child" ? "fixture child response" : "fixture response" }, null], [{}, "stop"]];
      for (const [delta, finish_reason] of chunks) {
        res.write(`data: ${JSON.stringify({ id: "chat_fixture", object: "chat.completion.chunk", created: 1, model: input.model, choices: [{ index: 0, delta, finish_reason }] })}\n\n`);
      }
      res.end("data: [DONE]\n\n");
    } else if (req.url === "/v1/responses") {
      res.setHeader("content-type", "text/event-stream");
      const first = trusted && requests.filter((r) => r.path === "/v1/responses").length === 1;
      const item = first
        ? { type: "function_call", id: "fc_fixture", call_id: "call_fixture", name: "exec_command", arguments: JSON.stringify({ cmd: "printf fixture-tool", max_output_tokens: 20 }), status: "completed" }
        : { type: "message", id: "msg_fixture", status: "completed", role: "assistant", content: [{ type: "output_text", text: "fixture response", annotations: [] }] };
      const events = [
        { type: "response.created", response: { id: "resp_fixture", status: "in_progress", output: [] } },
        { type: "response.output_item.added", output_index: 0, item: { ...item, status: "in_progress", content: [] } },
        ...(first ? [] : [{ type: "response.output_text.delta", item_id: item.id, output_index: 0, content_index: 0, delta: "fixture response" }]),
        { type: "response.output_item.done", output_index: 0, item },
        { type: "response.completed", response: { id: "resp_fixture", status: "completed", output: [item], usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 } } },
      ];
      for (const event of events) res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
      res.end();
    } else { res.writeHead(404); res.end(); }
  });
  server.listen(0, "127.0.0.1");
  // A cancelled test must not keep the Node worker alive solely via a listener.
  server.unref();
  await once(server, "listening");
  t.after(() => new Promise((done) => { server.close(done); server.closeAllConnections(); }));
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
  // Keep npm setup isolated too: no developer registry credentials or config.
  for (const key of ["npm_config_userconfig", "npm_config_globalconfig"]) {
    env[key] = join(home, key);
    await writeFile(env[key], "");
  }
  const binary = join(resolve(tools), harness);
  assert.equal((await exec(binary, ["--version"], { cwd, env, timeout: 30_000 })).stdout.trim(), pins[harness]);
  await exec(jevia, ["init"], { cwd, env });
  const config = join(cwd, ".jevia/config.toml");
  let source = (await readFile(config, "utf8")).replace(/base_url = "[^"]+"/, `base_url = "${base.slice(0, -3)}"`);
  const args = harness === "codex"
    ? ["exec", "--model", "{model}", "{task}"]
    : ["run", "--model", "{model}", "{task}"];
  const model = harness === "codex" ? "fixture-model" : "fixture/fixture-model";
  source += `\n[harnesses.${harness}]\ncommand = ${JSON.stringify(binary)}\nargs = ${JSON.stringify(args)}\n[harnesses.${harness}.models]\nfast = "${model}"\nbalanced = "${model}"\nstrong = "${model}"\n`;
  await writeFile(config, source);
  if (harness === "codex") {
    // Keep unrelated marketplace initialization out of this loopback test.
    // Disable plugins only in this disposable contract-test profile;
    // inline Jevia hooks still require the ordinary native trust review below.
    await writeFile(join(agentHome, "config.toml"), `model = "fixture-model"\nmodel_provider = "fixture"\nsandbox_mode = "read-only"\nfeatures.plugins = false\ncheck_for_update_on_startup = false\nweb_search = "disabled"\n[model_providers.fixture]\nname = "Loopback fixture"\nbase_url = "${base}"\nwire_api = "responses"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\n`);
  } else {
    await mkdir(join(configHome, "opencode"), { recursive: true });
    await writeFile(join(configHome, "opencode", "opencode.json"), JSON.stringify({
      enabled_providers: ["fixture"], autoupdate: false, snapshot: false,
      provider: { fixture: { npm: "@ai-sdk/openai-compatible", name: "Loopback fixture", options: { baseURL: base, apiKey: "loopback-only" }, models: Object.fromEntries(["fixture-model", "fixture-next", "fixture-child"].map((id) => [id, { name: id, limit: { context: 10000, output: 100 } }])) } },
      ...(scenario === "subagent" ? {
        agent: { "fixture-child": { mode: "subagent", description: "Return a fixed integration marker", model: "fixture/fixture-child", permission: { "*": "deny" } } },
        permission: { task: { "*": "deny", "fixture-child": "allow" } },
      } : {}),
    }));
    // OpenCode installs this SDK even with default plugins disabled. Resolve it
    // before the measured harness launch, not inside its recording deadline.
    const dependencyHome = join(configHome, "opencode");
    const setupStarted = performance.now();
    try {
      if (process.env.JEVIA_NATIVE_PLUGIN_DIR) {
        // Reuse installed SDK files, never a harness profile, authentication,
        // database, or node_modules shared writable between test cases.
        const source = resolve(process.env.JEVIA_NATIVE_PLUGIN_DIR);
        for (const name of ["package.json", "package-lock.json", "node_modules"]) {
          await cp(join(source, name), join(dependencyHome, name), { recursive: true });
        }
      } else {
        await exec("npm", ["install", "--ignore-scripts", "--no-audit", "--no-fund", "--save-exact", `@opencode-ai/plugin@${pins.opencode}`], {
          cwd: dependencyHome,
          env: { ...env, npm_config_fetch_retries: "0", npm_config_fetch_timeout: "20000" },
          timeout: 60_000, killSignal: "SIGKILL", maxBuffer: 1024 * 1024,
        });
      }
    } catch (cause) {
      throw new Error("Native fixture dependency setup failed before any task was run", { cause });
    }
    const installed = JSON.parse(await readFile(join(dependencyHome, "node_modules/@opencode-ai/plugin/package.json"), "utf8"));
    assert.equal(installed.version, pins.opencode);
    const manifest = JSON.parse(await readFile(join(dependencyHome, "package.json"), "utf8"));
    const lock = JSON.parse(await readFile(join(dependencyHome, "package-lock.json"), "utf8"));
    assert.equal(manifest.dependencies["@opencode-ai/plugin"], pins.opencode);
    assert.equal(lock.packages[""].dependencies["@opencode-ai/plugin"], pins.opencode);
    t.diagnostic(`OpenCode dependency setup: ${Math.round(performance.now() - setupStarted)} ms (not harness execution)`);
    // Any missed dependency must fail without fetching during the contract run.
    env.npm_config_offline = "true";
    env.npm_config_audit = "false";
    assert.equal(requests.length, 0, "Dependency setup must not route or submit a model task");
  }
  if (trusted) {
    await exec("python3", [resolve("tests/helpers/review-native-hooks.py"), jevia], { cwd, env, timeout: 50_000 });
    assert.equal(requests.length, 0, "Trust review must not route or submit a model task");
  }
  const extra = harness === "codex" ? ["--", "--skip-git-repo-check", "--sandbox", "read-only"] : [];
  const run = (task, extra) => exec(jevia, ["run", harness, `--task=${task}`, "--non-interactive", "--timeout-seconds", "45", ...extra], { cwd, env, timeout: 60_000, maxBuffer: 2 * 1024 * 1024 });
  const pending = run(task, extra);
  activeRun = pending.child;
  const result = await pending.catch((error) => {
    if (!["cancel", "provider_error"].includes(scenario) || error.signal || typeof error.code !== "number") throw error;
    return { stdout: error.stdout, stderr: error.stderr, code: error.code };
  });
  const records = JSON.parse((await exec(jevia, ["runs", "--json"], { cwd, env })).stdout);
  assert.equal(records.length, 1, "One execution must produce exactly one saved run");
  t.diagnostic(`${harness} execution: ${records[0].execution.duration_ms} ms; capture=${records[0].execution.observations.status}; events=${records[0].execution.observations.events.length}`);
  assert.equal(records[0].task, task);
  assert.ok(requests.some((r) => r.path !== "/v1/systemone" && JSON.stringify(r.input).includes(task)), "native provider receives the literal task");
  // Read the next real routing request, not just the saved file or explain text.
  // No feedback/verification call is made between execution and this decision.
  const followup = await exec(jevia, ["route", "Check the fixture marker again", "--no-cache", "--explain", "--json"], { cwd, env, timeout: 15_000 });
  assert.match(followup.stderr, /known_outcomes=0 passive_observations=1/);
  const routes = requests.filter((request) => request.path === "/v1/systemone");
  assert.equal(routes.length, 2);
  assert.deepEqual(routes[0].input.state.recent_execution_observations, []);
  assert.deepEqual(routes[1].input.state.recent_completed_outcomes, []);
  const [history] = routes[1].input.state.recent_execution_observations;
  assert.equal(routes[1].input.state.recent_execution_observations.length, 1);
  assert.equal(history.state, records[0].lifecycle.state);
  assert.equal(history.requested_model, model);
  assert.equal(history.process_exit_code, records[0].execution.exit_code ?? null);
  assert.equal(history.harness_observations.source, records[0].execution.observations.source);
  assert.equal(history.harness_observations.status, records[0].execution.observations.status);
  assert.equal(history.harness_observations.sampled_events, records[0].execution.observations.events.length);
  return { cwd, env, config, model, run, requests, result, record: records[0] };
}

for (const harness of ["opencode", "codex"]) {
  test(`real ${harness} resumes a session with a different model and fresh recording`, { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 180_000 }, async (t) => {
    const f = await fixture(t, harness, harness === "codex");
    const session = f.record.execution.observations.events.find((event) => event.session_id)?.session_id;
    assert.ok(session, "The first launch must report a native session ID");
    const nextModel = harness === "codex" ? "fixture-next" : "fixture/fixture-next";
    let source = (await readFile(f.config, "utf8")).replaceAll(f.model, nextModel);
    if (harness === "codex") {
      // Resume is a custom argv template. Supply its literal-task delimiter
      // explicitly; keep the same reviewed hooks and read-only profile.
      source = source.replace('args = ["exec","--model","{model}","{task}"]',
        `args = ${JSON.stringify(["exec", "resume", "--skip-git-repo-check", "--model", "{model}", session, "--", "{task}"])}`);
    }
    await writeFile(f.config, source);
    const task = "Continue the existing session with the new fixture model.";
    const offset = f.requests.length;
    const result = await f.run(task, harness === "opencode" ? ["--", "--session", session] : []);
    assert.match(result.stdout + result.stderr, /fixture response/);
    const runs = JSON.parse((await exec(jevia, ["runs", "--json"], { cwd: f.cwd, env: f.env })).stdout).filter((run) => run.execution);
    assert.equal(runs.length, 2);
    const resumed = runs.find((run) => run.run_id !== f.record.run_id);
    assert.equal(resumed.task, task);
    assert.equal(resumed.execution.model, nextModel);
    assert.equal(resumed.execution.exit_code, 0);
    assert.equal(resumed.execution.verification, undefined);
    assert.equal(resumed.outcome, "unknown");
    assert.equal(resumed.lifecycle.state, "completed");
    const events = resumed.execution.observations.events;
    assert.equal(resumed.execution.observations.status, "recorded");
    assert.ok(events.some((event) => event.session_id === session && event.kind === "turn_completed"));
    assert.ok(events.every((event) => !event.session_id || event.session_id === session), "Resume must not silently start another session");
    assert.ok(events.some((event) => event.model === nextModel), "Native events must identify the new model");
    assert.ok(!events.some((event) => event.kind === "tool_completed"), "The resumed response must not replay the first turn's tool event");
    assert.ok(f.requests.slice(offset).some((request) => request.input.model === "fixture-next" && JSON.stringify(request.input).includes(task)), "The new model receives the resumed task");
    const followup = await exec(jevia, ["route", "Inspect both fixture turns", "--no-cache", "--json", "--explain"], { cwd: f.cwd, env: f.env });
    assert.match(followup.stderr, /known_outcomes=0 passive_observations=2/);
    const history = f.requests.filter((request) => request.path === "/v1/systemone").at(-1).input.state;
    assert.deepEqual(history.recent_completed_outcomes, []);
    assert.deepEqual(new Set(history.recent_execution_observations.map((run) => run.requested_model)), new Set([f.model, nextModel]));
    assert.equal(history.recent_execution_observations.length, 2);
    for (const saved of runs) {
      const summary = history.recent_execution_observations.find((run) => run.requested_model === saved.execution.model);
      assert.equal(summary.harness_observations.sampled_events, saved.execution.observations.events.length);
      assert.equal(summary.harness_observations.status, "recorded");
    }
    assert.deepEqual(runs.find((run) => run.run_id === f.record.run_id), f.record, "Resume must not rewrite the original run");
    assert.ok(!(await readdir(join(f.cwd, ".jevia"))).some((name) => name.startsWith("jevia-events-")));
    t.diagnostic(`${harness} resumed: ${events.length} fresh events; both turns present in routing history`);
  });
}

test("real OpenCode subagent events retain distinct session and model identities", { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 180_000 }, async (t) => {
  const { cwd, record, requests } = await fixture(t, "opencode", false, "subagent");
  assert.equal(record.outcome, "unknown");
  assert.equal(record.execution.exit_code, 0);
  assert.equal(record.execution.verification, undefined);
  assert.equal(record.execution.observations.status, "recorded");
  const events = record.execution.observations.events;
  const parent = events.find((event) => event.kind === "session_started");
  const child = events.find((event) => event.kind === "subagent_started");
  assert.ok(parent?.session_id);
  assert.ok(child?.agent_id);
  assert.notEqual(child.session_id, parent.session_id);
  assert.equal(child.agent_id, child.session_id);
  assert.ok(events.some((event) => event.kind === "model_observed" && event.session_id === child.session_id && event.model === "fixture/fixture-child"));
  assert.ok(events.some((event) => event.kind === "turn_completed" && event.session_id === child.session_id));
  assert.ok(events.some((event) => event.kind === "tool_completed" && event.session_id === parent.session_id && event.tool_name === "task"));
  assert.ok(!events.some((event) => event.kind === "tool_succeeded"));
  assert.ok(requests.some((request) => request.input.model === "fixture-child"));
  assert.ok(requests.some((request) => request.input.messages?.some((message) => message.role === "tool" && JSON.stringify(message.content).includes("fixture child response"))), "The real child result must return to its parent");
  assert.ok(!JSON.stringify(record.execution.observations).includes("fixture child response"));
  assert.ok(!(await readdir(join(cwd, ".jevia"))).some((name) => name.startsWith("jevia-events-")));
});

test("real OpenCode loads the injected plugin and records passive model facts", { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 150_000 }, async (t) => {
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

for (const task of ["attach", "serve", "web"]) {
  test(`real OpenCode records the literal task ${task}`, { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 150_000 }, async (t) => {
    const { record } = await fixture(t, "opencode", false, "complete", task);
    assert.equal(record.execution.exit_code, 0);
    assert.equal(record.outcome, "unknown");
    assert.equal(record.execution.observations.status, "recorded");
    assert.ok(record.execution.observations.events.some((e) => e.kind === "tool_completed"));
  });
}

test("real trusted hooks record a tool call and turn without inferring success", { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 180_000 }, async (t) => {
  const { requests, record } = await fixture(t, "codex", true);
  assert.equal(record.execution.exit_code, 0);
  assert.equal(record.lifecycle.state, "completed");
  assert.equal(record.outcome, "unknown");
  assert.equal(record.execution.verification, undefined);
  const observations = record.execution.observations;
  assert.equal(observations.source, "codex_hooks");
  assert.equal(observations.status, "recorded");
  assert.ok(observations.events.some((e) => e.kind === "tool_completed" && e.tool_name === "Bash"));
  assert.ok(observations.events.some((e) => e.kind === "turn_completed"));
  const toolOutputs = requests.filter((r) => r.path === "/v1/responses")
    .flatMap((r) => r.input.input ?? []).filter((item) => item.type === "function_call_output");
  assert.ok(toolOutputs.some((item) => typeof item.output === "string" &&
    /Process exited with code 0[\s\S]*Output:\s*fixture-tool\s*$/.test(item.output)),
    `Real tool result reaches the next provider turn: ${JSON.stringify(toolOutputs).slice(0, 4096)}`);
  assert.ok(!JSON.stringify(observations).includes("fixture-tool"));
});

test("real Codex accepts injected hooks but preserves normal trust review", { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 150_000 }, async (t) => {
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

for (const harness of ["opencode", "codex"]) {
  for (const scenario of ["cancel", "provider_error"]) {
    test(`real ${harness} ${scenario} preserves unknown outcome and automatic history`, { skip: !tools && "set JEVIA_NATIVE_BIN_DIR to pinned CLI executables", timeout: 180_000 }, async (t) => {
      const { cwd, result, record } = await fixture(t, harness, harness === "codex", scenario);
      assert.equal(record.outcome, "unknown");
      assert.equal(record.execution.verification, undefined);
      assert.equal(record.execution.observations.source, harness === "codex" ? "codex_hooks" : "opencode_plugin");
      assert.equal(record.execution.observations.status, "recorded");
      assert.ok(record.execution.observations.events.length > 0, "Keep the native facts received before cancellation or rejection");
      if (scenario === "cancel") {
        assert.equal(record.lifecycle.state, "cancelled");
        assert.equal(result.code, 130);
      } else {
        assert.match(result.stdout + result.stderr, /fixture-provider-rejected/);
        assert.equal(record.lifecycle.state, "completed");
        assert.ok(Number.isInteger(record.execution.exit_code));
      }
      assert.ok(!JSON.stringify(record.execution.observations).includes("fixture-provider-rejected"), "Native facts must not retain provider error text");
      assert.ok(!(await readdir(join(cwd, ".jevia"))).some((name) => name.startsWith("jevia-events-")), "Terminal runs clean up their native journals");
    });
  }
}
