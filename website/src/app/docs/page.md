# Jevia documentation

Jevia is an adaptive, outcome-based router for coding harnesses. Start with the CLI, connect the models and agents you already use, then feed recorded observations and optional known outcomes into later routing decisions.

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

On native Windows, install the same checksum-verified CLI with PowerShell 5.1
or newer, then continue with the commands above:

```powershell
irm https://jevia.dev/install.ps1 | iex
jevia --version
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
- `run <harness> <task>` routes, launches, and records automatically; extra verification is opt-in.
- `runs [--json]` lists recent runs, lifecycle state, quality evidence, and execution observations.
- `runs show <run-id>` prints one complete versioned record.
- `stats [--limit N] [--json]` summarizes routing, trusted outcomes, feedback, and cache hits.

Add `--json` where supported for stable machine-readable output. Run `jevia <command> --help` for command-specific options.

## How adaptive routing works

1. **Route:** Jevia selects one configured capability tier for the current task.
2. **Execute:** Your chosen harness maps that tier to a concrete model and performs the work.
3. **Observe:** Record process facts and supported native activity automatically; keep uncertain outcomes unknown.
4. **Optional outcome:** Add feedback or extra verification only when wanted; neither is required for passive history.
5. **Adapt:** Use recorded observations and known outcomes as separate context for later routing decisions.

Jevia does not decide that its own output is good. A completed verifier or explicit application or human feedback can supply a known outcome. Without either, recorded activity still informs routing as passive context.

## Connect any harness

Map stable capability tiers to the model names your harness accepts:

```bash
jevia harness presets
jevia harness setup codex --preset codex \
  --model fast=your-fast-model \
  --model balanced=your-balanced-model \
  --model strong=your-strong-model
```

Built-in shell-free templates are available for Codex, Claude Code, OpenCode, and Gemini CLI. A preset supplies only the command and non-interactive argument shape; you still choose model IDs, credentials, permissions, and verification. The setup command only previews generated TOML.

```bash
# Review the generated TOML, then save it
jevia harness setup codex --preset codex \
  --model fast=your-fast-model \
  --model balanced=your-balanced-model \
  --model strong=your-strong-model --apply

# Validate templates and executable paths without launching anything
jevia harness check codex
jevia harness check codex --json

# Route and launch the configured harness
jevia run codex "investigate the failing integration test"
```

Changing an existing adapter also requires `--replace`. Extra harness arguments can follow `--` on `jevia run`. Jevia executes argument arrays directly without shell expansion.

Use explicit `--command` and repeated `--arg` values when a built-in template does not match the installed harness version or when connecting another agent.

The same pattern works with Claude Code, Codex, OpenCode, Gemini CLI, Cursor Agent, Copilot CLI, Aider, Goose, Amp, or a custom runner. Jevia learns from outcomes while the adapter owns the tier-to-model mapping.

### Automatic CLI pipeline

Configure your installed harness, credentials, and model mappings once. Then
`jevia run` routes the task, launches the agent, and records execution facts and
supported native events in your selected storage backend.
**No manual `feedback` or `runs complete` step is needed.** Recorded history informs
later routing automatically, even when task success is unknown.

```bash
jevia run codex "fix the failing test"
```

Unreleased: additional verification is opt-in. CLI 0.1.4–0.1.5 enabled test
discovery by default; existing explicit checks are preserved. Set `auto_verify = true`
only if you want root Rust/Node test discovery, or configure your own optional
verifier. Recording itself works with Python, Go, and mixed-language projects
without tests. Passing tests is evidence, not proof of every requirement.

Native event capture currently supports direct Claude Code 2.1.251+ launches.
Codex, OpenCode, Gemini, and other harnesses currently provide process-level
observations. Capture is best-effort: reported models, switches, and tool activity
do not prove which model solved a task. Raw prompts, tool contents, and transcripts
are not retained. Inspect `execution.observations` for coverage and recorded events.
Upgrade the CLI and SDK together for schema 5. See the
[capture contract and limits](https://github.com/assistant-ui/jevia/blob/main/docs/reference.md#native-harness-observations).

Optional verification runs when the agent process/session finishes, not after each internal
message or tool call. Jevia does not install the agent or test runner, supply
credentials, bypass agent permission prompts, or retry failed work. Prepare
project dependencies first. See the
[CLI setup and verification reference](https://github.com/assistant-ui/jevia/blob/main/docs/reference.md#automatic-cli-pipeline)
for supported tests, deadlines, and opt-in settings.

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
history, including CLI-verified results, plus a separate window of passive
execution observations. No manual cache clearing is needed:
evidence is part of the cache key.

Unknown outcomes are not quality labels. Finished process-only runs still supply
observations; active and routed-only runs do not. The SDK does not instrument agents
your application launches outside `jevia run`.
`[router].history_limit` bounds each history window: default 20, maximum 100, or 0 to
disable it. This is decision context, not model training or a guarantee of better
choices. Live requests send eligible historical task text, outcome metadata, and
bounded native summaries to Jev; feedback reasons, session IDs, and raw event lists
stay in storage. `[privacy].store_task_text = false`
omits task text from new records, not older history or the current routing request.

### Client methods

- `version(options?)` — Return the installed CLI version after validating its output.
- `route(task, options?)` — Choose a tier using recent eligible recorded outcomes automatically and return the complete typed route record.
- `feedback(runId, outcome, options?)` — Optionally record an application-reported outcome. No verifier is required; omitting feedback leaves the outcome unknown.
- `runs(options?)` — List recent route records with a configurable positive result limit.
- `show(runId, options?)` — Read one complete record, including lifecycle and evidence.
- `complete(runId, outcome, options)` — Finish externally executed work after confirming it has stopped; requires CLI 0.1.4 or newer.
- `setupStorage(target, options?)` — Preview SQLite or PostgreSQL setup and apply only with confirmation.
- `checkStorage(options?)` — Check selected storage or deeply validate it without a write probe.

### Configure storage from Node.js

**Node API availability:** these methods are available in `jevia@0.1.1`. They require CLI 0.1.2 or newer and an existing `jevia init` project. Creating a client never connects to a database or changes storage.

```typescript
// Relative paths resolve from the client cwd; absolute paths also work.
const target = { backend: "sqlite", path: ".data/jevia/history.db" } as const;

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
- `process_exit` — The harness exited. Useful passive context, but not proof of task success.
- `unknown` — No trusted result is known, so it stays out of the adaptive evidence set.

Changing a known outcome requires `--reason`. Jevia retains the prior value in feedback history. Setting `unknown` removes the run from learning without erasing execution evidence. Feedback on active runs is rejected.

## Choose and configure storage

- **JSONL · default:** No setup required. Local history lives in `.jevia/runs.jsonl` and is ignored by Git.
- **SQLite · local database:** Defaults to `.jevia/jevia.db`; choose another project-relative or absolute file with `--path`.
- **PostgreSQL · shared evidence:** Records live in your configured database under the selected project namespace.

### What a stored run looks like

Every backend preserves the same logical record. JSONL writes one compact JSON object per line to `.jevia/runs.jsonl`; this example uses explicitly enabled verification and is expanded for readability. SQLite and PostgreSQL store the equivalent fields while keeping the same lifecycle and outcome-evidence semantics.

```json
{
  "schema_version": 5,
  "run_id": "7b65a69a-0a6f-4a89-bd73-88f090954dd9",
  "tier": "balanced",
  "suggested_tier": "balanced",
  "confidence": 0.84,
  "probabilities": {
    "fast": 0.1,
    "balanced": 0.84,
    "strong": 0.06
  },
  "fallback_applied": false,
  "jev_model": "jev-latest",
  "created_at_ms": 1790700000000,
  "source": "live",
  "task": "fix the flaky integration test",
  "outcome": "success",
  "execution": {
    "harness": "codex",
    "model": "provider/standard",
    "duration_ms": 48231,
    "exit_code": 0,
    "observations": { "source": null, "status": "unsupported", "events": [] },
    "verification": {
      "command": "pnpm",
      "launched": true,
      "duration_ms": 6842,
      "exit_code": 0
    }
  },
  "lifecycle": {
    "state": "completed",
    "started_at_ms": 1790700001120,
    "finished_at_ms": 1790700056193
  },
  "outcome_evidence": {
    "source": "verification",
    "recorded_at_ms": 1790700056193
  }
}
```

Jev selects the tier, but it does not declare its own work successful. In this record, the verifier exited successfully, so `outcome_evidence.source` is `verification`. Set `privacy.store_task_text = false` to persist `task: null` instead of the raw task.

### Move a project to SQLite

```bash
# Choose a project-relative or absolute SQLite file; preview writes nothing
jevia storage setup sqlite \
  --path .data/jevia/history.db \
  --import-jsonl

# Stop Jevia writers and supervisors, then apply the same reviewed path
jevia storage setup sqlite \
  --path .data/jevia/history.db \
  --import-jsonl --apply --confirm-stopped
jevia storage check
jevia stats
```

Without `--path`, SQLite uses `.jevia/jevia.db`. Relative paths resolve from the discovered project root, even when the command runs in a subdirectory; absolute paths are also accepted. Omit `--import-jsonl` when the current JSONL history is empty. The source file is never deleted or rewritten.

The selected location is saved in `.jevia/config.toml`:

```toml
[storage]
backend = "sqlite"
url = "sqlite://.data/jevia/history.db"
```

Files under `.jevia` are ignored automatically. If you choose a path elsewhere, add the database, its WAL/SHM sidecars, and run-lock files to your ignore rules and protect the containing directory.

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
