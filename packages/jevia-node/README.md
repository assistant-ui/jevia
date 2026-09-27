# Jevia for Node.js

Typed, shell-free access to Jevia from Node.js. The package invokes the Jevia
CLI's JSON interface so routing policy, local storage, caching, privacy, and
outcome eligibility stay consistent with the Rust implementation.

## Install

Install the Jevia CLI first, then add the Node package:

```sh
curl -fsSL https://jevia.vercel.app/install.sh | sh
npm install jevia
```

The npm package does not download or execute an installer during `npm install`.
By default it finds `jevia` on `PATH`; pass `binary` when the executable lives
elsewhere.

## Route from any harness

```ts
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd() });
const task = "fix the flaky integration test";
const route = await jevia.route(task);

const models: Record<string, string> = {
  fast: "provider/small",
  balanced: "provider/standard",
  strong: "provider/frontier",
};

const result = await runYourHarness({
  task,
  model: models[route.tier],
  runId: route.run_id,
});

const verified = await verifyResult(result);
await jevia.feedback(route.run_id, verified ? "success" : "failure");
```

`runYourHarness` can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor
Agent, Copilot CLI, Aider, Goose, Amp, or a custom agent. Jevia selects a
capability tier; your adapter owns the tier-to-model mapping and harness API.

Feedback is explicit. A successful function return is not automatically treated
as proof that the task succeeded.

## API

```ts
const version = await jevia.version();
const route = await jevia.route(task, { noCache: true, signal });
const runs = await jevia.runs({ limit: 20, signal });
const run = await jevia.show(route.run_id, { signal });
const updated = await jevia.feedback(route.run_id, "success", { signal });
```

All child processes use `execFile` without a shell. `JeviaCommandError` exposes
the exit code, signal, stdout, and stderr for failed CLI calls.
