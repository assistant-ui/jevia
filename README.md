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

```bash
cargo install --git https://github.com/assistant-ui/jevia jevia
jevia init
export TYPESAFE_API_KEY="your-key"
jevia check
jevia route "investigate an intermittent distributed-lock failure"
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
[examples/jevia.toml](examples/jevia.toml).

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
production. The script builds the CLI from this repository with locked
dependencies; Rust 1.92+ and Cargo must already be installed. It does not install
Rust or modify shell configuration. Run `pnpm test` in `website` to check the
installer without installing anything.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution and commit
conventions and [docs/architecture.md](docs/architecture.md) for component
boundaries and routing invariants.

## License

MIT
