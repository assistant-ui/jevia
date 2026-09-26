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
cargo install --path crates/jevia-cli
jevia init
export TYPESAFE_API_KEY="your-key"
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
executable directly, and mirrors its exit code. A zero exit records success; a
non-zero exit records failure. If the process cannot start, the run remains
unknown so an environment problem does not incorrectly train the router.

Completed harness runs also record the concrete model, harness name, duration,
and process exit code. This evidence appears in <code>jevia runs --json</code>
and is supplied with relevant outcomes on later routing requests, so model
changes do not erase which implementation actually produced a result.

## Local data

Project configuration lives in `.jevia/config.toml` and is intended to be
reviewed and committed. Run history lives in `.jevia/runs.jsonl` and is ignored
by the project-local `.jevia/.gitignore` because prompts and outcomes may be
sensitive. Jevia coordinates concurrent readers and writers through the
ignored `.jevia/runs.lock` sidecar so parallel agents cannot overwrite one
another's evidence.

By default Jevia stores task text locally so it can supply useful examples to
future decisions. Set `store_task_text = false` under `[privacy]` to retain only
routing metadata.

`jevia doctor` validates the complete history and reports malformed records
without deleting or rewriting them.

## Repository structure

- `crates/jevia-core` contains configuration, typed API contracts, policy, and
  outcome records.
- `crates/jevia-cli` contains filesystem persistence and terminal commands.

Dashboard code does not belong in this repository. The managed dashboard is a
separate private project with a separate security boundary.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution and commit
conventions and [docs/architecture.md](docs/architecture.md) for component
boundaries and routing invariants.

## License

MIT
