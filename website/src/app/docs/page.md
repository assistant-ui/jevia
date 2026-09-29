# Jevia documentation

Jevia is an adaptive, outcome-based router for coding harnesses. Start with the CLI, connect the models and agents you already use, then feed verified results back into later routing decisions.

## Install and validate

Install the checksum-verified CLI, create local policy, and verify one live Jev request:

```bash
curl -fsSL https://jevia.dev/install.sh | sh
jevia --version
jevia init
export TYPESAFE_API_KEY="your-key"
jevia check
jevia route "fix the flaky integration test"
```

`jevia doctor` checks local configuration and storage without calling Jev. `jevia check` performs one live routing round trip without saving a synthetic run.

## Route, run, and inspect

Use routing on its own, or let Jevia supervise the complete harness workflow:

```bash
jevia route "investigate the failing integration test"
jevia route --json "investigate the failing integration test"
jevia run agent "investigate the failing integration test"
jevia runs
jevia runs show <run-id>
jevia stats --json
```

- `route <task>` selects a capability tier and saves the routing decision as a new run.
- `run <harness> <task>` routes the task, launches a configured harness, verifies it, and records the result.
- `runs [--json]` lists recent records, lifecycle state, outcome source, and learning eligibility.
- `runs show <run-id>` prints one complete versioned record.
- `stats [--limit N] [--json]` summarizes routing, trusted outcomes, feedback, and cache hits.

Add `--json` where supported for stable machine-readable output. Run `jevia <command> --help` for command-specific options.

## How adaptive routing works

1. **Route:** Jevia selects one configured capability tier for the current task.
2. **Execute:** Your chosen harness maps that tier to a concrete model and performs the work.
3. **Verify:** Tests, review, or another trusted evaluator decides whether the task succeeded.
4. **Record:** The verified outcome is attached to the run with its evidence source.
5. **Adapt:** Eligible outcomes become evidence for later routing decisions.

Jevia does not decide that its own output is good. A completed verifier or explicit application or human feedback supplies the outcome used as learning evidence.

## Connect any harness

Map stable capability tiers to the model names your harness accepts:

```bash
jevia harness setup agent --command my-agent \
  --arg=run --arg=--model --arg='{model}' --arg='{task}' \
  --model fast=provider/small \
  --model balanced=provider/standard \
  --model strong=provider/frontier \
  --verify-command cargo --verify-arg=test
```

This is a generic template, not a provider preset. Replace the command, arguments, and model IDs with values your harness accepts. The first command only previews generated TOML.

```bash
# Review the generated TOML, then save it
jevia harness setup agent --command my-agent \
  --arg=run --arg=--model --arg='{model}' --arg='{task}' \
  --model fast=provider/small \
  --model balanced=provider/standard \
  --model strong=provider/frontier \
  --verify-command cargo --verify-arg=test --apply

# Validate templates and executable paths without launching anything
jevia harness check agent
jevia harness check agent --json

# Route and launch the configured harness
jevia run agent "investigate the failing integration test"
```

Changing an existing adapter also requires `--replace`. Extra harness arguments can follow `--` on `jevia run`. Jevia executes argument arrays directly without shell expansion.

The same pattern works with Claude Code, Codex, OpenCode, Gemini CLI, Cursor Agent, Copilot CLI, Aider, Goose, Amp, or a custom runner. Jevia learns from outcomes while the adapter owns the tier-to-model mapping.

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
[CLI setup and verification reference](https://github.com/assistant-ui/jevia/blob/main/docs/reference.md#automatic-cli-pipeline-unreleased)
for supported tests, deadlines, and opt-out settings.

## Use the Node.js SDK

Map Jevia's tier to your harness model and let your application run the work.
Feedback and verification are optional. SDK routing does not launch an agent or
run tests. If your application knows the result, it can report it without a
verifier; no human feedback prompt is required. Skipping feedback still records
the route with an unknown outcome and does not block later routing:

Install the typed package after installing the Jevia CLI:

```bash
npm install jevia
```

```typescript
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd() });
const task = "fix the flaky integration test";
const route = await jevia.route(task);

const result = await runYourHarness({
  task,
  tier: route.tier,
  runId: route.run_id,
});

// Optional: your adapter may expose a known outcome; no verifier is required.
if (result.outcome === "success" || result.outcome === "failure") {
  await jevia.feedback(route.run_id, result.outcome);
}

// Recorded outcomes are included automatically in the next decision.
const next = await jevia.route("fix another parser regression");
```

The SDK invokes the local CLI through its shell-free JSON interface. The CLI must already be available on `PATH`; npm installation never runs a binary downloader. Pass an `AbortSignal` in method options when the caller needs cancellation.

Your adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor Agent,
Copilot CLI, Aider, Goose, Amp, or a custom runner. The optional `result.outcome`
field above comes from your own adapter; Jevia does not infer it from a successful
function return. Report only known outcomes, and do not submit feedback again
after a supervised CLI run.

### Recorded outcomes inform the next route

The SDK does not require Jevia's built-in verifier. Your application can use
tests, acceptance checks, or a user-approved result and record success or failure.
This is labeled `manual` evidence (application-reported), not CLI verification.
Use `unknown` when the result is uncertain. The next `route()` automatically
includes eligible recorded outcomes from the same JSONL, SQLite, or PostgreSQL
history, including CLI-verified results. No manual cache clearing is needed:
evidence is part of the cache key.

Unknown, active, and process-exit-only records are excluded.
`[router].history_limit` bounds recent evidence: default 20, maximum 100, or 0 to
disable it. This is decision context, not model training or a guarantee of better
choices. Live requests send eligible historical task text and outcome metadata
to Jev; feedback reasons stay in storage. `[privacy].store_task_text = false`
omits task text from new records, not older history or the current routing request.

### Client methods

- `version(options?)` — Return the installed CLI version after validating its output.
- `route(task, options?)` — Choose a tier using recent eligible recorded outcomes automatically and return the complete typed route record.
- `feedback(runId, outcome, options?)` — Optionally record an application-reported outcome. No verifier is required; omitting feedback leaves the outcome unknown.
- `runs(options?)` — List recent route records with a configurable positive result limit.
- `show(runId, options?)` — Read one complete record, including lifecycle and evidence.
- `setupStorage(target, options?)` — Unreleased: preview SQLite or PostgreSQL setup and apply only with confirmation.
- `checkStorage(options?)` — Unreleased: check selected storage or deeply validate it without a write probe.

### Configure storage from Node.js

**Unreleased Node API:** these methods are pending the next npm release and are not in `jevia@0.1.0`. They require CLI 0.1.2 or newer and an existing `jevia init` project. Creating a client never connects to a database or changes storage.

```typescript
const target = { backend: "sqlite", path: ".jevia/jevia.db" } as const;

// Preview only: no database connection or file changes.
console.log(await jevia.setupStorage(target, { importJsonl: true }));

// Stop project writers and supervisors before applying.
await jevia.setupStorage(target, {
  apply: true,
  confirmStopped: true,
  importJsonl: true,
});
console.log(await jevia.checkStorage());
console.log(await jevia.checkStorage({ deep: true }));
```

For PostgreSQL, keep the URL in the environment and pass only its variable name:

```typescript
// Set JEVIA_DATABASE_URL before creating the client.
const target = {
  backend: "postgres",
  project: "my-app",
  urlEnv: "JEVIA_DATABASE_URL",
} as const;

console.log(await jevia.setupStorage(target)); // Preview first.
```

Setup stays preview-first and returns human-readable reports. Use `signal` for cancellation and raise the client's `timeoutMs` for large imports. After cancellation or failure, inspect the config and destination before retrying.

### Cancellation and errors

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

Log the message, exit code, and signal. Raw command, stdout, and stderr can contain sensitive values and are for private debugging only. Invalid JSON or invalid complete records raise `JeviaProtocolError`.

## Verify and report outcomes

These commands are for externally executed work or corrections. A supervised
`jevia run` records its outcome automatically; no manual feedback step is needed
afterward.

```bash
jevia feedback <run-id> success
jevia feedback <run-id> failure --reason "The integration test still fails"
jevia feedback <run-id> unknown --reason "No reliable verification result"
jevia runs show <run-id>
```

- `verification` — A configured or detected verifier produced a known result. Eligible for learning.
- `manual` — An application or person explicitly recorded success or failure. Eligible for learning.
- `process_exit` — The harness exited. Visible for diagnosis, but not learning evidence by itself.
- `unknown` — No trusted result is known, so it stays out of the adaptive evidence set.

Changing a known outcome requires `--reason`. Jevia retains the prior value in feedback history. Setting `unknown` removes the run from learning without erasing execution evidence. Feedback on active runs is rejected.

## Choose and configure storage

- **JSONL · default:** No setup required. Local history lives in `.jevia/runs.jsonl` and is ignored by Git.
- **SQLite · local database:** A bundled, server-free database for larger local histories and indexed queries.
- **PostgreSQL · shared evidence:** Bring a direct or session-pooled database for trusted workspaces that share one project.

### Move a project to SQLite

```bash
# Preview the migration; this writes nothing
jevia storage setup sqlite --import-jsonl

# Stop Jevia writers and supervisors, then apply the reviewed plan
jevia storage setup sqlite --import-jsonl --apply --confirm-stopped
jevia storage check
jevia stats
```

Omit `--import-jsonl` when the current JSONL history is empty. The source file is never deleted or rewritten. Relative paths resolve from the project root.

### Share evidence with PostgreSQL

```bash
export JEVIA_DATABASE_URL="postgresql://..."

# Preview first; the URL stays in the environment
jevia storage setup postgres --project my-project --import-jsonl

# Stop writers and supervisors before applying
jevia storage setup postgres --project my-project --import-jsonl --apply --confirm-stopped
jevia storage check
```

Keep credentials in the environment, never in config or CLI arguments. `--url-env MY_DATABASE_URL` names a different variable. Use a direct or session-pooled connection, not a transaction-mode pooler.

### Check, export, and import

```bash
jevia storage check
jevia storage check --deep
jevia storage export --output .jevia/snapshot.jsonl
jevia storage import-jsonl --from <file>
jevia storage import-jsonl --from <file> --apply
```

Setup and import are preview-first. A deep check reads complete selected history without a write probe or automatic repair. Stop writers before migration and keep the source as a recovery copy.

## Cache, diagnostics, and recovery

### Routing cache

```bash
jevia cache status
jevia cache clear
jevia route --no-cache "force one live routing decision"
```

The default cache holds 256 decisions for 15 minutes. Each hit gets a fresh run ID. New outcomes and policy, model, or harness changes produce new cache keys. Cache errors fall through to a live request.

### Run lifecycle and recovery

Lifecycle phases are `routed`, `running`, `verifying`, `completed`, `launch_failed`, and `interrupted`. Execution state stays separate from task outcome.

`jevia runs recover <run-id>` marks an abandoned active execution interrupted only when no supervisor still owns its lease. It does not rerun work or terminate a process. Inspect the workspace and any surviving processes first.

For bounded headless execution:

```bash
jevia run agent "fix the parser" --non-interactive --timeout-seconds 300 --verification-timeout-seconds 120
```

Timeout flags require `--non-interactive` and apply to their individual execution phases.

### History maintenance

```bash
# Preview and repair a recoverable JSONL tail
jevia runs repair
jevia runs repair --apply

# Preview and archive older eligible terminal records
jevia runs archive --keep 1000
jevia runs archive --keep 1000 --apply
```

Repair is JSONL-only. Archive works with every backend and writes recovery files before removing eligible old records. Neither command applies changes until you repeat the reviewed command with `--apply`.

Use the raw Markdown endpoint at [`/api/docs-markdown`](/api/docs-markdown) when you need a portable, agent-readable version of the guide. The repository source also includes `docs/reference.md` for exhaustive guarantees and edge cases.

## Source

- [Jevia repository](https://github.com/assistant-ui/jevia)
- [Node.js package source](https://github.com/assistant-ui/jevia/tree/main/packages/jevia-node)
- [Architecture](https://github.com/assistant-ui/jevia/blob/main/docs/architecture.md)
