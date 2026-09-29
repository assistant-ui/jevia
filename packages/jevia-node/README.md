# Jevia for Node.js

Typed, shell-free access to Jevia from Node.js. The package invokes the Jevia
CLI's JSON interface so routing policy, local storage, caching, privacy, and
outcome eligibility stay consistent with the Rust implementation.

## Install

Install the Jevia CLI first, then add the Node package:

```sh
curl -fsSL https://jevia.dev/install.sh | sh
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

All CLI processes use `spawn` without a shell. `JeviaCommandError` has a
generic message, exit code, and signal safe for ordinary error logging. The raw
`command`, `stdout`, and `stderr` remain available through explicit getters for
private debugging, but are excluded from normal inspection and serialization.
These getters may contain tasks, credentials, and other sensitive data: do not
send them to logs or telemetry. Raw process and JSON parser causes are not retained.
Tasks, run IDs, and feedback reasons are passed as literal values, including
text starting with `-` or `--`.

`timeoutMs` bounds each SDK call (default 30 seconds; maximum 2147483647 ms).
Timeouts and aborts reject without waiting for the CLI callback. On POSIX, the
SDK sends SIGTERM to the call's isolated process group, then SIGKILL after a
100 ms grace period; on Windows it requests native process-tree termination.
Already-aborted signals never launch a process. Timeout/abort diagnostics may
be empty because rejection does not wait for output collection. Cancellation
cannot undo completed writes; inspect storage before retrying a mutation.

Record responses are validated at runtime, including schema versions 1–3,
finite probabilities in `[0, 1]`, safe-integer timestamps, and optional lifecycle,
execution, verification, outcome evidence, and feedback. Malformed or unsupported
responses raise `JeviaProtocolError` without their contents. Legacy records can
omit optional evidence; the SDK never invents it. Additive unknown fields remain
compatible within a supported schema.

## Development tests

See [opt-in storage](#opt-in-storage-unreleased) below for the new storage methods.

Run `pnpm test` for the SDK unit tests. To check argument handling against the
real Rust CLI, build it from the repository root with `cargo build --locked -p jevia`,
then run `pnpm --dir packages/jevia-node test:cli`. The integration tests use
temporary projects and a local mock routing server; no API credentials are needed.
Set `JEVIA_TEST_BINARY` to an absolute executable path to test another CLI build.
Set `JEVIA_TEST_POSTGRES_URL` only to an isolated test database to include the
PostgreSQL SDK integration test. It creates a uniquely named project and records;
the disposable CI service is discarded afterward.

## Opt-in storage (unreleased)

The following methods are pending the next npm release; they are not in `jevia@0.1.0`.
They require Jevia CLI 0.1.2 or newer and an existing `jevia init` project.

JSONL remains the default. Every client method reads the project's existing
`.jevia/config.toml`; constructing a client never initializes, connects to, or
migrates a database. Keep ordinary routing calls separate from administrative setup.

```ts
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd(), timeoutMs: 120_000 });
const target = { backend: "sqlite", path: ".jevia/jevia.db" } as const;

// Preview only: no database connection or file changes.
console.log(await jevia.setupStorage(target, { importJsonl: true }));

// After inspecting the preview and stopping ALL project writers/supervisors:
console.log(await jevia.setupStorage(target, {
  apply: true,
  confirmStopped: true,
  importJsonl: true,
}));
console.log(await jevia.checkStorage());
console.log(await jevia.checkStorage({ deep: true }));
```

SQLite paths are relative to the project root (an absolute path is also allowed).
For PostgreSQL, provision a database and set its connection URL in the process
environment or a secret manager before constructing the client:

```ts
const target = {
  backend: "postgres",
  project: "my-app",
  urlEnv: "JEVIA_DATABASE_URL",
} as const;

console.log(await jevia.setupStorage(target)); // Preview; credentials not required yet.
// Use the same explicit apply/confirmStopped/importJsonl options after review.
```

`urlEnv` is the variable **name**, not the URL. The client also accepts environment
overrides via `new JeviaClient({ env: { JEVIA_DATABASE_URL: secretFromYourVault } })`.
Never put a database URL in arguments or committed configuration. PostgreSQL uses
certificate-verified TLS by default; `allowInsecureLocalhost: true` permits only
loopback development connections, not insecure remote databases. `project` scopes
history inside a shared database; it is not an authorization boundary.

- `setupStorage(target, options?)` returns a human-readable CLI report. Preview is
  the default; `apply: true` requires `confirmStopped: true` at both type and runtime levels.
- `importJsonl: true` explicitly preserves existing JSONL records in the database;
  it is required on apply if the current JSONL history is nonempty. Source JSONL
  remains unchanged and is not continuously synchronized. Config is switched last.
- Failed setup leaves the original config selected, but destination schema/project
  or imported records may remain. Inspect both before retrying. A client timeout
  or cancellation is not a transaction rollback guarantee; verify state afterward.
- `checkStorage()` checks access using the CLI's rollback-only write probe for SQL.
  `{ deep: true }` validates records and metadata without a write probe or repairs.
  Neither initializes missing storage. These reports are not stable JSON APIs.
- Both methods accept `signal`; client timeout/buffer limits apply. Increase the
  timeout explicitly for large imports. Routes, feedback, and run queries then use
  the selected backend with no new per-request option.
- This is explicit JSONL-to-database setup, not SQL-to-SQL migration, database
  provisioning, a hosted service, or a no-storage mode. Cache remains project-local.
