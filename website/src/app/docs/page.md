# Jevia for Node.js

Use Jevia from Node.js to route tasks, inspect run records, and report verified outcomes while keeping local policy, storage, and learning behavior in one place.

## Install

Install the Node.js package after installing and configuring the Jevia CLI:

```bash
npm install jevia
```

The package uses the local `jevia` executable. It does not run an installer during `npm install`.

## Create a client and route

```typescript
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd() });
const route = await jevia.route("fix the flaky integration test");

console.log(route.tier, route.confidence, route.run_id);
```

A route returns the selected tier, confidence, probabilities, cache source, and a traceable run ID. Routing chooses capability; it does not claim the task succeeded.

## Client methods

- `version(options?)` — Return the installed Jevia CLI version after validating its output.
- `route(task, options?)` — Choose a capability tier and return the complete typed route record.
- `feedback(runId, outcome, options?)` — Record an explicit success, failure, or unknown outcome for a run.
- `runs(options?)` — List recent route records with a configurable positive result limit.
- `show(runId, options?)` — Read one complete route record, including lifecycle and outcome evidence.

## Open any harness

Map Jevia's tier to the model names your chosen harness accepts:

```typescript
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
await jevia.feedback(
  route.run_id,
  verified ? "success" : "failure",
);
```

Your adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor Agent, Copilot CLI, Aider, Goose, Amp, or a custom runner. Submit feedback only after your verifier determines the actual outcome.

## Cancellation and errors

```typescript
import { JeviaClient, JeviaCommandError } from "jevia";

try {
  await jevia.route(task, { signal: controller.signal });
} catch (error) {
  if (error instanceof JeviaCommandError) {
    console.error(error.message, error.exitCode, error.signal);
  }
}
```

Log only the message, exit code, and signal. Raw command, stdout, and stderr can contain sensitive data and are for private debugging only. Invalid JSON or records raise `JeviaProtocolError`.

## Source

[View the Node.js package source](https://github.com/assistant-ui/jevia/tree/main/packages/jevia-node).
