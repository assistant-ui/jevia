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

## Automatic CLI or explicit SDK?

For end-to-end execution, configure your installed harness once and use
`jevia run codex "fix the failing test"`. Jevia launches the agent, runs its
verifier after a successful exit, and saves the result automatically. **No manual
`feedback` or `complete` call is needed after `jevia run`.** Verification happens
when the launched process/session ends, not after each internal agent message.

Automatic detection of existing root Rust/Node tests is **unreleased**, pending
the next CLI release after 0.1.3. With CLI 0.1.3, configure a verifier once. If no
usable verifier is available, Jevia records an unverified, process-only result;
it does not treat the agent exiting as proof of task success. See the
[CLI pipeline documentation](https://github.com/assistant-ui/jevia#automatic-cli-pipeline-unreleased).

Use the SDK when your application should control how agents run and how their
work is checked. SDK routing does not launch a harness or run project tests.
Your integration can automatically call feedback after its own verifier; no
human feedback prompt is required, but the SDK will not infer task success.

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

// Prior recorded outcomes are included automatically; no history argument needed.
const next = await jevia.route("fix another parser regression");
```

`runYourHarness` can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor
Agent, Copilot CLI, Aider, Goose, Amp, or a custom agent. Jevia selects a
capability tier; your adapter owns the tier-to-model mapping and harness API.

Feedback is explicit. A successful function return is not automatically treated
as proof that the task succeeded.

### Recorded outcomes inform the next route automatically

The SDK does not require Jevia's built-in verifier. Your application can use
tests, an acceptance check, or a user-approved result, then record `success` or
`failure` with `feedback`. The source is labeled `manual` (application-reported),
not `verification`; it does not mean a person must type the feedback. Record
`unknown` when you cannot establish the outcome.

By default, each `route()` call loads recent eligible outcomes from that project's
selected JSONL, SQLite, or PostgreSQL storage before choosing a tier. There is no
need to fetch history with `runs()` or pass it back to `route()`. CLI-verified
results and SDK-reported results in the same history can both contribute. The
unreleased `complete()` method also records an eligible outcome while closing an
external run, but completion is not required merely to reuse `feedback`.

- Only known successes/failures backed by verification or explicit feedback are
  included. Pending/unknown, active, and process-exit-only records are excluded.
- `[router].history_limit` bounds the recent evidence (default 20, maximum 100;
  0 disables history input). This is context for a decision, not model training
  or a guarantee that future decisions improve.
- The evidence is part of the routing-cache key. A changed evidence payload
  prevents reuse of a decision based on older outcomes; unchanged inputs can
  still use the cache. No cache clearing or `noCache` flag is needed after feedback.
- On a live request, eligible historical task text and outcome metadata are
  supplied to Jev. Feedback reasons stay in storage and are not sent to Jev.
  `[privacy].store_task_text = false` omits task text from newly recorded history;
  it does not erase older records or hide the current task sent for routing.

### Optional history per route (unreleased)

```ts
// Default: use eligible recorded outcomes, within the project's history limit.
const informed = await jevia.route(task);

// Decide without prior outcomes for this call only.
const independent = await jevia.route(task, { useHistory: false });
```

`useHistory` defaults to `true`. Setting it to `false` excludes recorded outcomes
from both the decision input and its cache key. It does not delete history,
change project config, disable storage, or stop the new route from being recorded.
History-enabled and history-disabled decisions have separate cache entries, and
`noCache: true` can be combined with either mode. Explicit `useHistory: true`
still respects `[router].history_limit`, including a project-wide limit of 0.

This option is pending the next npm release. Disabling history requires the
next CLI release after 0.1.3 (`jevia route --no-history`). An older CLI rejects the
unsupported flag; the SDK will not retry with history enabled. Verification and
feedback remain application-controlled regardless of this option.

## API

```ts
const version = await jevia.version();
const route = await jevia.route(task, { noCache: true, signal });
const independent = await jevia.route(task, { useHistory: false, signal }); // Unreleased.
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

## Error categories (unreleased)

In the next npm release, `JeviaCommandError.kind` provides an allowlisted category
without requiring access to private stderr, arguments, or raw system errors:

| Kind | Meaning |
| --- | --- |
| `not_found` | Executable or working-directory path missing/not a directory. |
| `permission_denied` | Operating system refused access or execution. |
| `timeout` | SDK deadline elapsed. |
| `aborted` | Caller cancelled through an AbortSignal. |
| `output_limit` | Captured stdout or stderr exceeded its byte limit. |
| `invalid_options` | Node rejected process options. |
| `exit` | CLI exited unsuccessfully; inspect the safe `exitCode`. |
| `signal` | CLI terminated by a signal; inspect the safe `signal`. |
| `spawn_failed` | Other/unrecognized process failure. |

`JeviaProtocolError.kind` is `protocol`. Existing error classes, private diagnostic
getters, `exitCode`, and `signal` remain available. Invalid SDK method arguments
still throw `TypeError`. Raw error codes, paths, messages, and causes are never
copied into the public category. The OS cannot reliably distinguish a missing
binary from a missing `cwd`, so `not_found` intentionally covers both.

```ts
import { JeviaCommandError } from "jevia";

try {
  await jevia.route(task);
} catch (error) {
  if (error instanceof JeviaCommandError) {
    console.error({ kind: error.kind, exitCode: error.exitCode, signal: error.signal });
  }
  throw error;
}
```

Categories do not imply retry safety. After a timeout, abort, or failed mutation,
inspect the stored state before retrying; cancellation is not a rollback guarantee.

## External completion (unreleased)

`complete` requires the next CLI release after 0.1.3 and the next npm release;
it is not available in `jevia@0.1.0`. After your harness and verifier have both
stopped, use it instead of `feedback` when you also want to finish the run:

```ts
const done = await jevia.complete(route.run_id, verified ? "success" : "failure", {
  confirmStopped: true,
  reason: "External verification finished",
});
```

This atomically records manual feedback and `lifecycle.state = "completed"`,
making the record eligible for explicit archival. It never manufactures a start
time, process exit, or verifier evidence. Pass `unknown` if work stopped without
a conclusive result. Changing a known outcome requires a reason. Pending legacy
records without execution evidence are supported; active, terminal, or supervised
runs are refused. It does not stop external processes. Repeated completion is
refused; after a timeout or lost response, inspect `show(runId)` before retrying.
Ordinary `feedback` keeps its existing behavior and does not close a run.

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
