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
4. record the routing decision locally;
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
`jevia doctor` for local checks without an API request.

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
| `jevia runs` | Inspect recent local routing records. |
| `jevia feedback <id> <outcome>` | Mark a run as `success`, `failure`, or `unknown`. |
| `jevia doctor` | Validate configuration, credentials, and local storage. |
| `jevia check` | Validate local state and complete a live Jev routing round trip without storing a run. |
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
local audit trail; original execution evidence is preserved. Reasons are limited
to 4096 bytes and are never sent to Jev. Setting the outcome to `unknown` removes
it from learning evidence. These local records are not a tamper-proof audit log.

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

## Local data

Project configuration lives in `.jevia/config.toml` and is intended to be
reviewed and committed. Run history lives in `.jevia/runs.jsonl` and is ignored
by the project-local `.jevia/.gitignore` because prompts and outcomes may be
sensitive. Jevia coordinates concurrent readers and writers through the
ignored `.jevia/runs.lock` sidecar so parallel agents cannot overwrite one
another's evidence.

Routing decisions live in the ignored `.jevia/cache.jsonl` file and use the
same locking and atomic-replacement guarantees through `.jevia/cache.lock`.

By default Jevia stores task text locally so it can supply useful examples to
future decisions. Set `store_task_text = false` under `[privacy]` to retain only
routing metadata.

`jevia doctor` validates the complete history and reports malformed records
without deleting or rewriting them.

## Repository structure

- `crates/jevia-core` contains configuration, typed API contracts, policy, and
  outcome records.
- `crates/jevia-cli` contains filesystem persistence and terminal commands.
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
