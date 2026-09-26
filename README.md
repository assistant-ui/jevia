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
5. attach success or failure after the task finishes;
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

To try unreleased development changes instead:

```bash
cargo install --git https://github.com/assistant-ui/jevia --locked jevia
```

Record the real result after the task completes:

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
| `jevia route <task>` | Ask Jev for a tier and record the decision. |
| <code>jevia run &lt;harness&gt; &lt;task&gt;</code> | Route, launch a configured harness, and record its exit outcome. |
| `jevia runs` | Inspect recent records in the configured backend. |
| `jevia feedback <id> <outcome>` | Mark a run as `success`, `failure`, or `unknown`. |
| `jevia doctor` | Validate configuration, credentials, and configured storage. |
| `jevia storage init` | Explicitly initialize an opt-in database schema and project. |
| `jevia storage check` | Check storage without needing a Jev API key. |
| `jevia storage import-jsonl [--from <file>] [--apply]` | Preview/import local history into a database without changing the source. |
| `jevia storage export --output <file>` | Export history to a new JSONL snapshot; never overwrite a file. |
| `jevia check` | Validate configured storage and complete a live Jev routing round trip without storing a run. |
| `jevia cache status` | Inspect routing-cache settings and entry counts. |
| `jevia cache clear` | Remove cached decisions without touching run history. |

Run `jevia <command> --help` for command-specific options.

## Harness adapters

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
failure and skips verification. Without a configured verifier, a zero harness
exit records success for backward compatibility.

When <code>verification</code> is configured, Jevia runs it only after the
harness succeeds and uses its exit status as the final outcome. A verifier that
cannot start leaves the outcome unknown, preventing an environment problem
from incorrectly training the router. Verification arguments support the same
placeholders and are also launched directly without shell interpretation.

Completed harness runs also record the concrete model, harness name, duration,
process exit code, and verification evidence. This data appears in
<code>jevia runs --json</code> and is supplied with relevant outcomes on later
routing requests, so model changes do not erase which implementation actually
produced a verified result.

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

Without this flag, existing interactive terminal behavior remains unchanged.
This is not a sandbox: descendants that deliberately escape a process group/job,
SIGKILL of Jevia, and machine crashes cannot be handled reliably. Use explicit
recovery and inspect the workspace in those cases; no work is automatically retried.

## Outcome provenance

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
the selected backend. These options are unreleased and are not in v0.1.1.

### SQLite: local database, no server

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

### Guarantees and current limits

- Database writes are transactional. Feedback history and its current outcome
  change together; short project-scoped write locks prevent lost updates.
- Recent history and eligible evidence use indexed, bounded queries in append
  order. Complete versioned records preserve execution and feedback provenance.
- `storage check` verifies schema and CRUD permissions with a rolled-back probe;
  it does not insert fake evidence. `doctor`/`check` include this check. It is not
  a complete database integrity scan; SQLite/PostgreSQL maintenance remains the
  operator's responsibility.
- Database/record schema versions are checked; unknown versions are rejected.
  Driver errors are redacted and database operations have five-second timeouts.
  An outage never silently switches history back to local JSONL.
- Imports preview by default and commit all-or-nothing with `--apply`. Identical
  run IDs are skipped; conflicting records, duplicate source IDs, active runs,
  or malformed/unsupported records abort the import. Stop source writers first.
  Keep the unchanged source as your backup; no automatic bidirectional sync or
  background replication is provided.
- `jevia storage export --output .jevia/snapshot.jsonl` writes a new private
  snapshot. Protect and ignore exports; they may contain task text. Exports
  normalize records rather than preserving original JSON whitespace.
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
- `runs repair` and `runs archive` remain JSONL-only and refuse database mode.
  Use explicit exports and your database's backup/retention tooling instead.
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

### History maintenance

Both maintenance commands preview by default. Inspect the report before repeating
with `--apply`; `--json` provides counts and saved paths for automation.

```sh
jevia runs repair
jevia runs repair --apply
jevia runs archive --keep 1000
jevia runs archive --keep 1000 --apply
```

Repair handles an incomplete, unterminated final JSON line after an interrupted
write, or a valid final record missing its newline. It refuses malformed middle
lines, complete invalid records, unsupported schemas, and duplicate IDs. It does
not guess at missing fields or rewrite individual outcomes.

Archival keeps the most recently **appended** `--keep` terminal records (minimum
one), plus every active, routed/pending, or legacy-unknown record. Older terminal
records move to a separate JSONL archive; original record bytes and additive
metadata are preserved. Archived records no longer appear in `runs`, accept
feedback, or inform routing. Choose retention to preserve the evidence you need;
this is explicit maintenance, not automatic pruning.

Before applying either operation, Jevia saves the exact original file in
`.jevia/history-backups/`. Archival also writes `.jevia/history-archives/` before
atomically replacing active history. The history lock covers the entire operation
and the plan is recomputed on apply. Both directories are ignored local data;
files use private permissions on Unix and may contain sensitive prompts. Backups
and archives are never automatically removed, so total disk use can increase.
If restoring manually, first stop all Jevia writers and save the current history;
replacing it with an older backup would otherwise discard newer runs.

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
