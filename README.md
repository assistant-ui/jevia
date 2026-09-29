# Jevia

Jevia is an outcome-aware model router for coding agents. It asks Jev for a
typed routing decision, applies a deterministic safety policy, and records the
eventual result so later decisions can use evidence from earlier runs.

> Jevia is experimental. The current milestones establish the CLI, routing
> contract, local outcome store, Jev integration, and generic harness
> execution. The managed control plane remains separate.

## Why Jevia

Most model routers classify a task and immediately forget what happened.
Jevia closes that loop:

1. describe stable capability tiers rather than hard-coding model names;
2. ask Jev which tier should handle the current task;
3. fall back to a configured safe tier when confidence is low;
4. record the routing decision in the configured history store;
5. automatically record execution and verification outcomes for `jevia run`;
6. include recent outcomes as evidence in future routing decisions.

The application owns the policy. Jev supplies a structured decision signal.

## Quick start

Use the copyable `/install.sh` command on the
[Jevia landing page](https://jevia.vercel.app). It downloads a
checksum-verified binary for macOS or Linux on Intel or ARM; Rust and Cargo are
not required.

```bash
curl -fsSL https://jevia.vercel.app/install.sh | sh
jevia --version
jevia init
export TYPESAFE_API_KEY="your-key"
jevia check
jevia route "investigate an intermittent distributed-lock failure"
```

`jevia check` makes one live Jev request and requires a valid API key. Use
`jevia doctor` for configuration/storage checks without a Jev API request.
PostgreSQL storage checks do connect to the configured database.

To install the exact crates.io release with Rust 1.92 or newer:

```bash
cargo install jevia --version 0.1.3 --locked
```

To try unreleased development changes instead:

```bash
cargo install --git https://github.com/assistant-ui/jevia --locked jevia
```

### Run an agent: automatic outcome recording

After [configuring your installed harness once](#harness-adapters), use
`jevia run` for the end-to-end flow:

```bash
jevia run codex "fix the failing test"
jevia runs
```

Jevia routes the task, launches the agent, runs verification after a successful
agent exit, and saves the outcome automatically. **No manual `feedback` or
`runs complete` step is needed.** Verifier-backed outcomes become evidence for
later routing decisions. The name `codex` must match your configured adapter;
Jevia does not install or authenticate the agent for you.

**Unreleased:** automatic discovery of existing Rust/Node tests is pending the
next CLI release after 0.1.3. The installer above still installs 0.1.3, which
needs a configured verifier to verify runs automatically. See
[automatic CLI pipeline](#automatic-cli-pipeline-unreleased) for detection,
overrides, and verification limits.

### Route only: your integration owns execution

`jevia route` (used in the connection quickstart above) selects a tier but does
not execute an agent or verify a task. Only if you run and verify the work
outside `jevia run` do you need to report its actual result yourself:

```bash
jevia feedback <run-id> success
jevia runs
```

Machine-readable output is available for integrations:

```bash
jevia route --json "fix a typo in the README"
jevia runs --json
```

## Commands

| Command | Purpose |
| --- | --- |
| `jevia init` | Create `.jevia/config.toml` and local store rules. |
| `jevia harness setup <name>` | Preview an explicit harness template; back up and save only with `--apply`. |
| `jevia harness check <name> [--json]` | Inspect configuration and local executable candidates without launching programs or calling APIs. |
| `jevia route <task>` | Ask Jev for a tier and record the decision. |
| <code>jevia run &lt;harness&gt; &lt;task&gt;</code> | Route, launch, verify, and record automatically; test auto-detection is unreleased. |
| `jevia runs` | Inspect recent records in the configured backend. |
| `jevia stats [--limit <records>] [--json]` | Summarize recent routing decisions, verified outcomes, manual feedback, and cache hits. |
| `jevia feedback <id> <outcome>` | Report externally verified work or manually correct an outcome; not required after `run`. |
| `jevia runs complete <id> <outcome> --confirm-stopped` | Explicitly finish external work with manual evidence (unreleased). |
| `jevia doctor` | Validate configuration, credentials, and configured storage. |
| `jevia storage setup <sqlite\|postgres>` | Preview database setup; explicitly apply after validation, backup, and optional JSONL import. |
| `jevia storage init` | Explicitly initialize an opt-in database schema and project. |
| `jevia storage check` | Check storage without needing a Jev API key. |
| `jevia storage check --deep` | Inspect all history records and SQL routing/order metadata without a write probe or repair. |
| `jevia storage import-jsonl [--from <file>] [--apply]` | Preview/import local history into a database without changing the source. |
| `jevia storage export --output <file>` | Export history to a new JSONL snapshot; never overwrite a file. |
| `jevia check` | Validate configured storage and complete a live Jev routing round trip without storing a run. |
| `jevia cache status` | Inspect routing-cache settings and entry counts. |
| `jevia cache clear` | Remove cached decisions without touching run history. |

Run `jevia <command> --help` for command-specific options.

## Routing insights

```sh
jevia stats
jevia stats --limit 500
jevia stats --json
```

`stats` reads the configured JSONL, SQLite, or PostgreSQL history without calling
Jev, launching a harness, changing outcomes, or reading the decision-cache file.
It needs no Jev API key; PostgreSQL still requires its configured database
connection. Initialize an opted-in database with `jevia storage init` first.
This command requires v0.1.2 or newer.

The default window is the **latest 1,000 records in append order**, not a date
range or an all-time total. `--limit` accepts 1–100,000. The report says when older
records were excluded; archived records are not included. SQL reads are bounded
and project-scoped. JSONL still scans and validates the full file under its shared
history lock, but retains only the requested tail in memory.

Totals and per-tier groups use the **selected tier recorded at routing time**,
including tiers since removed from configuration. Tiers are not concrete model
identities: changing a harness's model mapping does not split historical groups.
Each record counts once, using its current outcome and provenance:

- **Verified success rate:** verifier-backed successes divided by verifier-backed
  successes plus failures. Manual feedback, process-exit-only results, active
  runs, unknown outcomes, and legacy outcomes without provenance are excluded.
  A configured verifier's result is not a guarantee of task correctness.
- **Manual feedback:** separate success/failure counts. Correcting a verified
  outcome with manual feedback moves that record to the manual group; prior
  feedback events and the old verifier result are not counted again.
- **Learning evidence:** known, non-active verifier-backed or manual outcomes,
  using the same eligibility rule as routing. This is the eligible count in the
  stats window, not necessarily the smaller evidence window sent to Jev.
- **Cache hits:** recorded cached decisions divided by all records in the window,
  not current cache occupancy or a count of API calls. Bypassed/disabled-cache
  decisions still count in the denominator. Legacy records without a decision
  source use the existing `live` default.
- **Other outcomes:** process-exit-only and unattributed known outcomes, plus
  active and unknown counts. Active takes precedence over unknown, so the outcome
  groups partition the records without double-counting.

Rates with no eligible observations display `n/a`, not 0%. The JSON report uses
`schema_version: 1`, `storage`, `window` (`limit`, `order`, `has_older_records`),
`totals`, and a tier-keyed `tiers` map. Rates are fractions from 0 to 1, or `null`
when their denominator is zero. `verified`, `manual`, `process_exit`, and
`unattributed` each contain `successes` and `failures`; `active` and `unknown`
are separate counts. Output excludes task text, run IDs, feedback notes,
execution commands, and connection URLs; historical tier labels are included.

These are descriptive, potentially small or biased samples—not a model ranking,
a controlled benchmark, or proof that adaptive routing improves results. No
cost savings are estimated because token/cost telemetry is not recorded.

## Harness adapters

### Preview-first setup

`jevia init` leaves harness selection to you. Configure your installed agent with
an explicit argument template and one model mapping for every configured tier:

```sh
jevia harness setup agent --command my-agent \
  --arg=run --arg=--model --arg='{model}' --arg='{task}' \
  --model fast=provider/small \
  --model balanced=provider/standard \
  --model strong=provider/frontier
```

This is a generic example, not a provider preset: substitute your agent's actual
executable, argument syntax, and accessible model IDs. Repeat the same command
with `--apply` after reviewing its TOML preview. Setup never launches either
program, calls Jev, opens storage, or checks provider credentials/model access.
It works offline, including when a configured database is unavailable. There
is no interactive prompt or automatic agent installation. Quote placeholders as
shown and use `--arg=--flag` / `--verify-arg=--flag` for leading-hyphen arguments.

- Preview changes no files. Apply shares a configuration lock with database setup,
  saves a private exact-byte backup in ignored `.jevia/config-backups/`, then
  atomically replaces the config after checking for concurrent edits. It preserves
  unrelated settings, harnesses, and comments, and rejects symlink configs. Stop
  concurrent manual config editing; the lock only coordinates Jevia setup commands.
- Changing an existing harness also requires `--replace`. Reapplying identical
  settings does not rewrite the config or make another backup (apply may create
  the config lock sidecar). The selected harness entry is rewritten when changed.
- Automatic test detection and persistent opt-out behavior are **unreleased**
  (after 0.1.3).
  Omitted verifier options preserve an existing verifier and automatic-detection
  setting. New adapters default to automatic project tests. Use `--no-verification`
  to disable both explicit and automatic checks, or `--auto-verification` to remove
  a custom verifier and re-enable detection. Supplying `--verify-command` replaces
  its whole command and argument list (for example, `--verify-command cargo
  --verify-arg=test`). Without verification, process success alone is not eligible
  learning evidence. Missing/duplicate/unknown tier mappings and unsupported
  template placeholders are rejected before config replacement.
- Commands, arguments, and model IDs appear in previews and committed config.
  Never put credentials in these flags or templates; let the agent inherit its
  credentials from the environment. Protect retained config backups, especially
  with appropriate directory ACLs on Windows. Nothing is shell-expanded by Jevia.

### Local preflight

```sh
jevia harness check agent
jevia harness check agent --json
```

Preflight validates the project config and selected adapter, renders its templates
for every configured tier, rejects NUL arguments, and checks file candidates for
the agent and configured verifier (or automatically detected verifier in the
unreleased CLI). It requires no Jev/provider key or database
connection, does not read history/cache, and creates no files or locks. It never
executes even a `--version` probe. Missing executables, model mappings, or valid
templates produce a failing exit status. An absent verifier is a warning, not a
failure; process success alone still is not learning evidence.

The human report escapes the requested harness name. JSON reports use
`schema_version: 1`, `harness`, `scope: "static"`, `ok`, `checks` (stable `id`,
`status`, `code`, and explanatory `message`), and `limitations`. Once a project
is found, failed checks also produce JSON and exit nonzero. CLI argument errors
and missing projects retain normal CLI diagnostics. Reports omit configured
command paths, arguments, model IDs, credentials, and raw parse/driver errors.

Lookup checks explicit paths or the inherited `PATH`; it does not expand shell
aliases, variables, `~`, or `PATHEXT`. Unix relative paths/PATH entries are checked
against the project root, with regular-file and execute-bit checks. On Windows,
bare names may omit `.exe`; non-`.exe` extensions must be explicit. Preflight
requires absolute explicit paths and absolute PATH entries on Windows, and warns
that it does not search extra system/application directories. This intentionally
conservative check is not a complete reproduction of OS executable resolution;
use an absolute path if lookup is ambiguous. Windows batch wrappers get a warning
because [Rust launches them through `cmd.exe`](https://doc.rust-lang.org/std/process/index.html#windows-argument-splitting).

An `ok` result means **static checks passed**, not that an agent is authenticated
or will successfully launch. Binary format, interpreters, mount/ACL restrictions,
agent-specific flags, provider model access, and task correctness are not tested;
files and environment may change afterward. Preflight does not change `jevia run`
or the existing `jevia check` live routing probe.

### Manual configuration

Harness adapters are shell-free process templates. Add a harness to
<code>.jevia/config.toml</code> and map every capability tier to a concrete
model:

~~~toml
[harnesses.agent]
command = "my-agent"
args = ["run", "--model", "{model}", "{task}"]

[harnesses.agent.models]
fast = "provider/small"
balanced = "provider/standard"
strong = "provider/frontier"

[harnesses.agent.verification]
command = "cargo"
args = ["test", "--workspace", "--all-features"]
~~~

A complete ready-to-copy configuration is available at
[examples/jevia.toml](https://github.com/assistant-ui/jevia/blob/main/crates/jevia-cli/examples/jevia.toml).

Then route and run a task through that adapter:

~~~bash
jevia run agent "investigate the failing integration test"
~~~

Extra harness arguments must follow <code>--</code> and are appended without
shell interpretation:

~~~bash
jevia run agent "update the parser" -- --verbose
~~~

Templates support <code>{task}</code>, <code>{model}</code>,
<code>{tier}</code>, and <code>{run_id}</code>. Jevia requires the task and
model placeholders, rejects unknown placeholders, launches the configured
executable directly, and mirrors its exit code. A non-zero harness exit records
failure and skips verification. If neither an explicit nor an automatically
detected verifier is available, a zero harness exit records process-only success
for backward compatibility, not verified task success or learning evidence.

Jevia runs the configured or detected verifier only after the harness succeeds
and uses its exit status as the final outcome. A verifier that
cannot start leaves the outcome unknown, preventing an environment problem
from incorrectly training the router. Verification arguments support the same
placeholders and are also launched directly without shell interpretation.

Completed harness runs also record the concrete model, harness name, duration,
process exit code, and verification evidence. This data appears in
<code>jevia runs --json</code> and is supplied with relevant outcomes on later
routing requests, so model changes do not erase which implementation actually
produced a verified result.

## Node.js API

Use the typed `jevia` npm package when a Node application or agent framework
owns harness execution. It calls the CLI's shell-free JSON interface, keeping
routing policy, storage, caching, and outcome handling in one implementation:

~~~bash
npm install jevia
~~~

~~~ts
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd() });
const task = "investigate the failing integration test";
const route = await jevia.route(task);

const model = {
  fast: "provider/small",
  balanced: "provider/standard",
  strong: "provider/frontier",
}[route.tier];

const result = await runYourHarness({ task, model, runId: route.run_id });
const verified = await verifyResult(result);
await jevia.feedback(route.run_id, verified ? "success" : "failure");
~~~

The adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor Agent,
Copilot CLI, Aider, Goose, Amp, or a custom harness. Jevia returns a capability
tier; the application maps that tier to a harness-specific model. Feedback stays
explicit—a successful process or function return is not automatically proof of
task success. The CLI must already be installed and available on `PATH`; npm
installation does not run a binary downloader.

## Automatic CLI pipeline (unreleased)

After configuring your harness once, use the normal command:

```sh
jevia run codex "fix the failing test"
```

Jevia routes the task, launches the agent, runs verification after a successful
agent exit, and records the lifecycle and result in your selected storage backend.
Verified results are available to subsequent routing automatically. **Do not run
`feedback` or `runs complete` afterward** for the normal supervised CLI flow.
Those APIs are for manual corrections and externally executed work, respectively.

Verification runs once when the launched agent process/session finishes
successfully, not after every message or tool call inside an interactive agent.
Task execution may still require the agent's normal permission prompts. Installing
the agent, supplying credentials/model mappings, and preparing project dependencies
are one-time prerequisites, not per-run feedback tasks. Jevia does not retry or
repair failed work automatically.

If the adapter has no explicit verifier, `auto_verify = true` (the default)
detects existing tests at the Jevia project root:

- Rust: `cargo test --workspace` for a root Cargo package/workspace.
- Node: the existing `test` script, using `packageManager`, then an unambiguous
  lockfile, then npm. npm, pnpm, Yarn, and Bun are supported. Empty scripts and
  common placeholder/no-op scripts are not selected.

Automatic checks run with `CI=true`, no stdin, owned process-tree cleanup, and a
five-minute deadline even if the agent was interactive. To override the deadline,
use `--non-interactive --verification-timeout-seconds <seconds>`. Jevia does not
install a test runner or package manager. The existing Cargo/package-manager test
command may build/download dependencies as it normally does; use trusted projects.

An explicit `[harnesses.<name>.verification]` always wins. For mixed Rust/Node
roots, unsupported projects, invalid manifests, or conflicting lockfiles, Jevia
reports that automatic verification is unavailable instead of guessing. The run
is still recorded, but process-only results are not verified learning evidence.
Configure a suitable check once for such projects. `harness check <name>` previews
detection and executable availability without running tests. Passing tests means
the selected checks passed—not a guarantee that every requirement was satisfied.

This behavior is pending the next CLI release after 0.1.3. Existing adapters
without a verifier also gain detection. To preserve process-only behavior, set
`auto_verify = false` in that adapter; the older `--no-verification` removed the
verifier without storing an explicit opt-out. The new flag persists that opt-out.
The Node SDK's `route`, `feedback`, and `complete` remain explicit and flexible;
they never launch an agent or run project tests automatically.

## Run lifecycle

Execution progress is separate from task outcome. New records track `routed`,
`running`, `verifying`, `completed`, `launch_failed`, or `interrupted`, with start
and finish timestamps. A completed process may still have a failed task outcome.

```sh
jevia runs show <run-id>
jevia runs recover <run-id>
```

`show` prints the complete record as JSON. `recover` explicitly marks a formerly
running/verifying execution as interrupted only if no Jevia supervisor holds its
per-run lease. It leaves the task outcome unknown and never reruns or terminates
processes. After a supervisor crash, inspect any surviving child processes and
workspace changes before starting new work. Legacy unknown outcomes are not
assumed to represent interrupted executions. Feedback on active runs is refused.

New and updated records use schema version 3; versions 1 and 2 remain readable.
Older CLI versions refuse version 3 rather than silently discard new metadata.
The ignored `run-leases/` sidecars are retained so concurrent processes always
coordinate on the same lock file.

### Complete work from an external harness (unreleased)

When you use `route` and run your own agent, ordinary `feedback` updates the
outcome but deliberately leaves the run pending. After **all external work and
verification have stopped**, explicitly finish it:

```sh
jevia runs complete <run-id> success --confirm-stopped
# A changed known outcome requires an explanation:
jevia runs complete <run-id> failure --confirm-stopped --reason "Tests still fail"
```

This command is pending the next CLI release after 0.1.3. It atomically records
manual feedback and a completed lifecycle in JSONL, SQLite, or PostgreSQL. The
finish timestamp is when completion was recorded; start time and harness evidence
remain absent because Jevia did not observe execution. `unknown` is also accepted
when work stopped but the result is inconclusive; it does not become learning evidence.

Only pending external records qualify. Active/terminal runs, supervised evidence,
held execution leases, and SQL supervisor ownership are refused. This command
does not stop processes, verify work, retry tasks, or bypass recovery. Repeated
completion is refused; after an uncertain response, inspect `runs show` before
retrying. Completed records become eligible for normal preview-first archival;
other pending and active records remain protected.

### Bounded non-interactive execution

For a headless harness, opt into owned process-tree supervision:

```sh
jevia run agent "fix the parser" --non-interactive --timeout-seconds 300 --verification-timeout-seconds 120
```

Each deadline applies only to its execution phase, not the Jev routing request.
Both flags require `--non-interactive` and accept 1–86400 seconds. Unspecified
deadlines are unlimited. This mode disables stdin and uses a Unix process group
or Windows job object; stdout/stderr still stream normally. Ctrl-C (and SIGTERM
on Unix) stops the owned group/job and records `cancelled`; a deadline records
`timed_out`. Outcomes remain unknown, failed/cancelled harnesses skip verification,
and verification cancellation preserves the harness evidence. Exit codes are 130
for cancellation and 124 for timeout. Cleanup errors record `interrupted` instead
of claiming the process tree was stopped.

The main command exiting does not finish a supervised phase while background
processes remain in its group/job. Jevia waits for them before starting verification
or recording completion, and the phase deadline and cancellation remain active.
The main command's exit code is preserved; background commands must report their
own failures to the main command (or the verifier) if they should affect the outcome.

Without this flag, existing interactive terminal behavior remains unchanged.
This is not a sandbox: descendants that deliberately escape a process group/job,
SIGKILL of Jevia, and machine crashes cannot be handled reliably. Use explicit
recovery and inspect the workspace in those cases; no work is automatically retried.

## Outcome provenance

`jevia run` saves this evidence automatically. The manual feedback examples below
are for externally verified work, corrections, or legacy records—not a required
step after each CLI run.

Run records distinguish `process_exit`, `verification`, and `manual` evidence.
Process-only success/failure remains visible, but only known outcomes from a
completed verifier or explicit human feedback are supplied to Jev as learning
evidence. Legacy outcomes without provenance are not silently promoted; confirm
them with `feedback` if you want them used in routing. `runs` reports the source
and whether the result is eligible for learning.

```sh
jevia feedback <run-id> success
jevia feedback <run-id> failure --reason "The integration test still fails"
jevia runs show <run-id>
```

Changing an already-known outcome requires a nonempty `--reason`. Every feedback
operation retains the prior outcome/source, timestamp, and optional reason in a
audit trail in the selected backend; original execution evidence is preserved. Reasons are limited
to 4096 bytes and are never sent to Jev. Setting the outcome to `unknown` removes
it from learning evidence. These records are not a tamper-proof audit log.

## Routing cache

Jevia caches equivalent routing decisions locally so repeated work does not
always require another network request. The default policy keeps up to 256
decisions for 15 minutes:

~~~toml
[cache]
enabled = true
ttl_seconds = 900
max_entries = 256
~~~

A cache key is a SHA-256 fingerprint over the exact task, Jev endpoint and
model, routing policy, tier definitions, selected harness mapping, and the
recent completed evidence actually sent to Jev. A new success, failure,
verification result, policy change, model change, or harness change therefore
produces a miss automatically. Pending outcomes do not invalidate an otherwise
equivalent decision.

Cache files contain the fingerprint and decision signal, not task text. Every
hit receives a fresh run ID and timestamp, and run records expose
<code>source=live</code> or <code>source=cache</code>. Use
<code>--no-cache</code> on <code>jevia route</code> or
<code>jevia run</code> when a forced live decision is needed.

Cache errors never block routing: Jevia reports the problem and falls through
to a live request. API errors are never cached, expired decisions are never
used as an offline fallback, and <code>jevia cache clear</code> provides an
explicit recovery path for a damaged cache.

Concurrent equivalent cache misses normally share one live routing request.
Waiters recheck both the cache and current learning evidence before using a
decision; they still receive independent run IDs. Coordination uses up to 256
stable lock stripes in the ignored `cache-leases/` directory, so lock files do
not grow per task. Unrelated requests can occasionally share a stripe and wait.
No global history/cache lock is held during network calls. An OS lease is
released if its owner exits; failures are not cached, so another caller can try.

Waiting is bounded to the configured Jev timeout plus one second (at most 30
seconds). On expiry or a coordination error, routing proceeds live; duplicate
requests are possible in that fallback. `--no-cache` skips coordination too.

## Storage

JSONL remains the default. SQLite and PostgreSQL are opt-in alternatives; changing
the backend does **not** synchronize or automatically move existing history.
The normal `route`, `run`, `runs`, `feedback`, `doctor`, and `check` commands use
the selected backend. These options require v0.1.2 or newer.

### SQLite: local database, no server

The guided CLI path avoids editing TOML by hand (run `jevia init` first):

```sh
jevia storage setup sqlite --import-jsonl
# Stop all Jevia writers/supervisors using this workspace, then:
jevia storage setup sqlite --import-jsonl --apply --confirm-stopped
jevia storage check
jevia stats
```

The first command previews without creating files or contacting a database.
`--path .jevia/custom.db` chooses another local file; paths are resolved from the
discovered project root even when invoked from a subdirectory. Protect and ignore
custom paths outside `.jevia` yourself. An empty JSONL project can omit
`--import-jsonl`; a nonempty one must include it to avoid silently abandoning
existing evidence. The source JSONL file is never deleted or rewritten.

Or configure manually:

Add to `.jevia/config.toml`:

```toml
[storage]
backend = "sqlite"
url = "sqlite://.jevia/jevia.db"
```

Then explicitly initialize and optionally import your old JSONL history:

```sh
jevia storage init
jevia storage check
jevia storage import-jsonl
jevia storage import-jsonl --apply
jevia runs
```

Relative file paths are resolved from the project root, not the current working
directory. SQLite is bundled into the binary. It uses WAL, full synchronous writes,
a five-second busy timeout, and private file permissions on Unix for new databases.
Use a local disk, not a shared/network filesystem. Default `.db` files, WAL/SHM
sidecars, and run locks under `.jevia` are ignored after `init`/`storage init`.
For custom paths/extensions, protect the directory and add your own ignore rules;
on Windows, protect the directory with the appropriate filesystem ACLs.

### PostgreSQL: bring your own database

Provision a dedicated PostgreSQL database and put its connection URL in your
secret manager or environment as `JEVIA_DATABASE_URL`. Do not put passwords in
the project config or CLI arguments.

```sh
jevia storage setup postgres --project my-project --import-jsonl
# Stop all Jevia writers/supervisors using this workspace, then:
jevia storage setup postgres --project my-project --import-jsonl --apply --confirm-stopped
jevia storage check
```

`--url-env MY_DATABASE_URL` selects a different environment variable **name**, not
a URL value. Preview does not resolve that variable or test connectivity. On apply,
the existing TLS and timeout rules apply; `--allow-insecure-localhost` is available
only for loopback development databases. The command creates the Jevia schema and
project, not a PostgreSQL server, database, or user.

The equivalent manual configuration is:

```toml
[storage]
backend = "postgres"
url_env = "JEVIA_DATABASE_URL"
project = "my-project"
```

Run `jevia storage init` once with schema-creation permissions, then
`jevia storage check`. Normal operation needs read/write access to the
`jevia_projects` and `jevia_runs` tables and read access to `jevia_schema`,
not permission to create databases. Remote connections require certificate and
hostname verification (`sslmode=verify-full`); `sslrootcert`, `sslcert`, `sslkey`,
and `application_name` URL options are supported. Only local development can opt
into plaintext using `allow_insecure_localhost = true` and a loopback host.

Use a **direct or session-pooled connection**, not a transaction-mode pooler:
execution guards use PostgreSQL session advisory locks. Each CLI invocation uses
a small pool; running a harness also holds a dedicated guard connection.

Use the same `project` value across trusted workspaces to share evidence. This
namespace is **not authorization or tenant isolation**: anyone with access to the
database tables can access other projects. Separate database roles/databases or a
future authenticated managed API are needed for mutually untrusted users.

### Setup safety and recovery

`storage setup` is preview-first. Applying requires both `--apply` and
`--confirm-stopped`: stop all source writers and supervisors, including scheduled
jobs and other workspaces using the source file. Recover any active run records
before retrying; this command does not stop processes or recover runs for you.

Apply backs up the exact old config to a new `.jevia/config-backups/config-*.toml`
file, initializes/checks the destination, imports requested history transactionally,
then atomically replaces config **last**. Routing, privacy, cache, harness settings,
and unrelated TOML comments are preserved. Config permissions are retained;
new backups are private on Unix and added to local ignore rules. Backups are not
automatically removed. On Windows, protect the directory with appropriate ACLs.

Active, duplicate, malformed, unsupported, or conflicting imported records cause
failure. Identical destination records are skipped for safe retries. Setup does
not replace config on connection, schema, permission, or import failure. It
serializes other setup invocations and checks for changes to the source/config
before switching, but cannot prevent an editor or already-running supervisor
from writing: the stop-writers requirement is not optional.

The database and filesystem are **not one atomic transaction**. A failed setup
can leave an initialized database, a config backup, or (if the final config save
fails) imported records. Inspect config, keep the original JSONL and backup, and
retry only after reconciling concurrent changes. A crash after replacement may
mean config was already switched; verify with `jevia storage check`. Never blindly
restore old config once new work has written to the database, since histories can
diverge. No automatic synchronization, rollback deletion, or backend fallback is
performed.

Once configured, repeat the same setup command **without `--import-jsonl`** to
initialize/check the same target without rewriting config or adding another backup.
Old JSONL files may be stale, so importing them from an already-SQL project needs
the separate explicit `storage import-jsonl` command. Changing between SQL targets
or back to JSONL is not supported by guided setup; use explicit export/import and
review the configuration change yourself.
Changing the value of the PostgreSQL URL environment variable can independently
change the destination; setup cannot detect which database it previously named.

### Guarantees and current limits

- Database writes are transactional. Feedback history and its current outcome
  change together; project-scoped write locks prevent lost updates. Retention
  holds the same lock while saving recovery files; schedule it during a quiet period.
- Recent history and eligible evidence use indexed, bounded queries in append
  order. Complete versioned records preserve execution and feedback provenance.
- `storage check` verifies schema and CRUD permissions with a rolled-back probe;
  it does not insert fake evidence. `doctor`/`check` include this access check.
  SQL records are not individually decoded by the default access check.
- `storage check --deep` instead inspects the complete selected history without
  a write probe, automatic repair, Jev request, or cache access. JSONL validates
  record schemas and nonempty/unique run IDs under the shared history lock,
  streaming records while retaining an ID set (memory grows with the IDs).
  SQL uses 200-row pages in one consistent read snapshot to validate record
  schemas, indexed run IDs, learning flags against current evidence rules, and
  positive append ordinals bounded by the project's append counter. Gaps left
  by retention are valid; the counter need not equal the newest retained ordinal.
  Only the configured PostgreSQL project is inspected. Ordinary SQL writers can
  continue, though a long scan may delay database cleanup/WAL recycling.
- A deep-check failure exits nonzero and identifies the line or append-position
  where possible, without printing task text, IDs, or database credentials. Keep
  the current history and backups and investigate locally before making changes;
  no records or indexes are silently rewritten. Success means the inspected
  snapshot passed these logical checks, not that write permissions, physical
  database integrity, or task-outcome correctness were verified. Run the default
  access check separately when needed; database-native integrity checks and
  backups remain the operator's responsibility. Missing storage is not initialized.
- Database/record schema versions are checked; unknown versions are rejected.
  Driver errors are redacted and database operations have five-second timeouts.
  An outage never silently switches history back to local JSONL.
- Imports preview by default and commit all-or-nothing with `--apply`. Identical
  run IDs are skipped; conflicting records, duplicate source IDs, active runs,
  or malformed/unsupported records abort the import. Stop source writers first.
  Keep the unchanged source as your backup; no automatic bidirectional sync or
  background replication is provided.
- `storage import-jsonl` validates and streams the source into a private unnamed
  temporary file before taking a database write lock. It holds the source's shared
  JSONL lock during capture, then imports only the captured records in one database
  transaction. Later changes to the source are not included; preview and apply
  capture independently. Both modes need temporary disk space for the normalized
  history in the OS temporary directory (use protected directory ACLs on Windows).
  Memory grows with the run-ID set plus the largest record, not all task bodies;
  the ID set is released before database writes. Snapshot failures prevent writes;
  later read errors or conflicts roll back all inserts and their ordering counter.
  The temporary file is removed on close/process exit, not kept as a backup.
  Imports still normalize known fields and omit unknown additive fields. Large
  imports hold the project write lock until commit/rollback (SQLite serializes
  all database writers); this is not a resumable or chunk-committed import. Guided
  `storage setup --import-jsonl` still captures its migration source in memory.
- `jevia storage export --output .jevia/snapshot.jsonl` writes a new private
  snapshot in append order. It streams one JSONL record or a bounded SQL page at
  a time, rather than loading the entire history. JSONL holds its shared history
  lock; SQL uses one consistent read snapshot across all pages (PostgreSQL
  repeatable-read/read-only, SQLite WAL snapshot) without taking the project write
  lock. SQL changes committed after the snapshot begins appear in a later export,
  not partway through this one. Very large records still require memory, and a
  long SQL snapshot can delay database cleanup/WAL recycling.
- Export writes to a private temporary file beside the destination, validates all
  records, flushes/syncs the file, and publishes without overwriting any existing
  path, including symlinks. Handled read/write failures do not publish a partial
  destination. The parent directory is synced on Unix; if that final sync fails,
  the error says the complete file was already created. A process crash can leave
  a private `.jevia-export-*.tmp` file. Protect and ignore exports and their output
  directory; they may contain task text. On Windows, use appropriate directory
  ACLs. Exports normalize known record fields, omit unknown additive fields, and
  are logical record snapshots, not exact-byte or physical database backups.
- The decision cache and cache-miss coordination remain local. Every routing
  attempt fetches current eligible evidence before computing its cache key, so
  new shared outcomes invalidate affected decisions. Cross-machine request
  deduplication and offline write queues are not included.
- SQLite run locks sit beside the canonical database path. PostgreSQL guards
  block recovery while the supervisor's database session is alive. Because a
  lost session does not prove its subprocess stopped, PostgreSQL recovery also
  requires `jevia runs recover <id> --confirm-stopped` after inspecting and
  stopping the original supervisor and any surviving processes. Recovery is
  terminal and fences late writes from that supervisor; it never reruns work.
  There is no automatic lease expiration or remote process termination.
- `runs archive` supports all three backends with preview-first, backup-first
  retention (see below). `runs repair` remains JSONL-only: database integrity
  repair and physical backups require database-native tooling.
- Managed hosting, managed credentials, and a dashboard are not part of this
  integration. Data is stored locally or in the user's own database.

### Default JSONL data

Project configuration lives in `.jevia/config.toml` and is intended to be
reviewed and committed. Run history lives in `.jevia/runs.jsonl` and is ignored
by the project-local `.jevia/.gitignore` because prompts and outcomes may be
sensitive. Jevia coordinates concurrent readers and writers through the
ignored `.jevia/runs.lock` sidecar so parallel agents cannot overwrite one
another's evidence.

Routing decisions live in the ignored `.jevia/cache.jsonl` file and use the
same locking and atomic-replacement guarantees through `.jevia/cache.lock`.

By default Jevia stores task text in the selected backend so it can supply useful examples to
future decisions. Set `store_task_text = false` under `[privacy]` to retain only
routing metadata.

In JSONL mode, `jevia doctor` validates the complete history and reports malformed records
without deleting or rewriting them.

Malformed configuration, history, and cache diagnostics omit raw values, including
parser error chains. They report line/column positions where available and safe
configuration guidance. This does not redact deliberately requested run/task output
or stdout/stderr streamed by your configured harness and verifier.

Provider and transport failures also omit raw response values and endpoint URLs,
including underlying error chains. Diagnostics retain safe failure categories,
JSON positions where available, and HTTP status codes.

### History maintenance

Both maintenance commands preview by default. Inspect the report before repeating
with `--apply`; `--json` provides counts and saved paths for automation.

```sh
jevia runs repair
jevia runs repair --apply
jevia runs archive --keep 1000
jevia runs archive --keep 1000 --apply
```

Repair is JSONL-only. It handles an incomplete, unterminated final JSON line after
an interrupted write, or a valid final record missing its newline. It refuses malformed middle
lines, complete invalid records, unsupported schemas, and duplicate IDs. It does
not guess at missing fields or rewrite individual outcomes.

Archival works with JSONL, SQLite, and PostgreSQL. It keeps the most recently
**appended** `--keep` eligible terminal records (minimum one), plus every active,
routed/pending, or legacy-unknown record. SQL also retains any record with an
execution owner, even if its recorded lifecycle appears terminal. This command
does not recover runs, stop processes, or infer that old work has finished.

Older eligible records move to a separate JSONL archive. They no longer appear in
`runs` or `stats`, accept feedback, or inform routing. Routing fetches current
evidence before cache lookup, so removing relevant evidence changes the cache key;
archival itself does not clear or rewrite the local decision cache. Choose a
retention window large enough for the evidence you need. This is explicit
maintenance, not automatic pruning.

All backends save recovery files under the invoking project's
`.jevia/history-backups/` and `.jevia/history-archives/` **on the CLI machine**,
including when PostgreSQL is remote. Apply recomputes its plan under the history
lock. Preview (and a no-op apply) removes no records and creates no recovery files.

- **JSONL:** repair and archival back up the exact original file before atomic
  replacement. Archived record bytes, including additive metadata, are preserved.
- **SQL:** archival holds the selected project's write lock, validates all records
  in bounded pages, and saves a full project-record snapshot plus an archive of
  the selected rows before any deletion. A private temporary journal bounds
  memory while deletion batches commit in one transaction. Saved JSON preserves
  additive fields but flattens physical line breaks into JSONL; it is not an
  exact-byte or physical database backup. SQL project/ordinal/owner metadata and
  other projects are not included. There is no database schema change.

SQL rejects malformed/unsupported records and mismatched indexed IDs, rechecks
the snapshot against its plan, and conditions deletions on the saved row values.
Jevia writers serialize with retention; long operations
can make other commands hit their database timeout. Coordinate external SQL
writers too, since they may bypass Jevia's project lock. A snapshot failure
prevents deletion. A deletion failure rolls back all batches and leaves any
finalized recovery files. If a connection fails while committing, the result may
be ambiguous: inspect active history and the reported files before retrying or
restoring. Never assume a failed response means nothing committed.

These directories are ignored local data and may contain sensitive prompts.
New snapshot files/directories use private permissions on Unix; on Windows,
protect the project directory with appropriate filesystem ACLs. Jevia syncs files
before deleting, and also syncs snapshot directories on Unix. Backups and archives
are never overwritten or automatically removed. Total disk use can increase;
SQL archival does not vacuum or compact the database.

For SQL restoration, first stop writers, export the current state to a new file,
and review the chosen archive before explicitly importing it:

```sh
jevia storage export --output .jevia/before-restore.jsonl
# Replace sql-runs-UUID.jsonl with the archive path printed by `runs archive`.
jevia storage import-jsonl --from .jevia/history-archives/sql-runs-UUID.jsonl
jevia storage import-jsonl --from .jevia/history-archives/sql-runs-UUID.jsonl --apply
```

Import keeps run IDs, outcomes, and known provenance, skips identical records,
and aborts on conflicts. It **appends restored records as newest**, not at their
original SQL positions, which can change the routing evidence window. Unknown
additive JSON fields remain in the archive but are not retained by typed import.
A full pre-archive snapshot can contain active records and is not suitable for
blind import. For JSONL restoration, stop writers and save current history before
manual replacement; an older backup would otherwise discard newer runs. SQL
snapshots do not replace a database-native disaster-recovery backup strategy.

## Repository structure

- `crates/jevia-core` contains configuration, typed API contracts, policy, and
  outcome records.
- `crates/jevia-cli` contains JSONL/SQLite/PostgreSQL persistence and terminal commands.
- `website` contains the Farm.js product site and getting-started guide.

Dashboard code does not belong in this repository. The managed dashboard is a
separate private project with a separate security boundary.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Run the website separately:

```bash
cd website
pnpm install
pnpm dev
```

The site serves `/install.sh` and uses the current page's origin in its copyable
install command: localhost during development and the deployed domain in
production. The script downloads the pinned GitHub release binary, verifies its
SHA-256 checksum, and installs it to `~/.local/bin` by default. It does not need
Rust or Cargo and does not modify shell configuration. Run `pnpm test` in
`website` to check the installer without installing anything.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution and commit
conventions and [docs/architecture.md](docs/architecture.md) for component
boundaries and routing invariants.

## License

MIT
