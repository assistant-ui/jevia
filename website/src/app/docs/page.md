# Jevia for Node.js

Use Jevia from Node.js to route tasks, inspect run records, and report verified outcomes while keeping local policy, storage, and learning behavior in one place.

Using `jevia run` instead? The CLI handles execution, verification, and outcome
recording automatically. No manual feedback step is needed. See
[CLI vs. SDK execution](#open-any-harness) below for setup, release availability,
and verification limits.

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
- `setupStorage(target, options?)` — Unreleased: preview opt-in SQLite or PostgreSQL setup; apply only with confirmation.
- `checkStorage(options?)` — Unreleased: check selected storage, or deeply validate it without a write probe.

## Opt-in storage

**Unreleased Node API:** these methods are pending the next npm release and are not
in `jevia@0.1.0`. They require CLI 0.1.2 or newer and an existing `jevia init` project.

JSONL remains the default. Creating a client never connects to a database or changes
storage. All methods use the backend selected in `.jevia/config.toml`.

```typescript
const target = { backend: "sqlite", path: ".jevia/jevia.db" } as const;

// Preview only: no database connection or file changes.
console.log(await jevia.setupStorage(target, { importJsonl: true }));

// After review, stop all project writers and supervisors before applying.
await jevia.setupStorage(target, {
  apply: true,
  confirmStopped: true,
  importJsonl: true,
});
console.log(await jevia.checkStorage());
console.log(await jevia.checkStorage({ deep: true }));
```

Paths are relative to the project root, or absolute. Applying switches config last.
Nonempty JSONL history requires explicit import. Source JSONL remains intact and is
not continuously synchronized. Routes, feedback, and run queries then use SQL.

For PostgreSQL, provision a database and set its URL in the environment before
creating the client. Never put credentials in arguments or committed config:

```typescript
const target = {
  backend: "postgres",
  project: "my-app",
  urlEnv: "JEVIA_DATABASE_URL",
} as const;

console.log(await jevia.setupStorage(target)); // Preview first.
// Apply with the same explicit confirmation and import options above.
```

`urlEnv` is the variable name, not the URL. Environment overrides can also be supplied
through `new JeviaClient({ env: { JEVIA_DATABASE_URL: secretFromYourVault } })`.
PostgreSQL requires verified TLS by default. `allowInsecureLocalhost: true` is only
for loopback development. Project names scope history, not database permissions.

Both methods return human-readable CLI reports, not stable JSON. Normal checks use
a rollback-only SQL write probe; deep checks validate records without a write probe
or repairs. Neither initializes missing storage. Both support `signal`; raise the
client's `timeoutMs` for large imports. After failures or cancellation, inspect config
and destination before retrying: database changes may remain. This is not SQL-to-SQL
migration, database provisioning, a hosted service, or a no-storage mode. Cache stays local.

## Open any harness

### Automatic CLI pipeline

Configure your installed harness, credentials, and model mappings once. Then
`jevia run` routes the task, launches the agent, runs verification after a
successful exit, and records the outcome in your selected storage backend.
**No manual `feedback` or `runs complete` step is needed.** Verifier-backed
outcomes become evidence for later routing decisions automatically.

```bash
jevia run codex "fix the failing test"
```

Automatic test detection is **unreleased**, pending the next CLI release after
0.1.3. The current installer still installs 0.1.3: configure a verifier once for
that version. The upcoming CLI detects root Rust workspace tests or Node test
scripts; an explicit verifier takes precedence. Missing or ambiguous checks stay
unverified and are excluded from learning. Passing tests is evidence, not proof
of every requirement.

Verification runs when the agent process/session finishes, not after each internal
message or tool call. Jevia does not install the agent or test runner, supply
credentials, bypass agent permission prompts, or retry failed work. Prepare
project dependencies first. See the
[CLI setup and verification reference](https://github.com/assistant-ui/jevia#automatic-cli-pipeline-unreleased)
for supported tests, deadlines, and opt-out settings.

### SDK-controlled execution

Map Jevia's tier to your harness model and let your application run and verify
the work. SDK routing does not launch an agent or run tests; your integration
explicitly reports the result. That call can be automatic in your application—no
human feedback prompt is required:

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

Your adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor Agent,
Copilot CLI, Aider, Goose, Amp, or a custom runner. Submit feedback only after your
verifier determines the actual outcome of this SDK-controlled work, not again
after a supervised CLI run.

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
