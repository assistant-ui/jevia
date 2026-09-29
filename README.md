# Jevia

Jevia is an open-source, local-first, outcome-based model router for coding
harnesses. It chooses a capability tier for each task, applies your safety
policy, and uses verified outcomes as evidence for future routes.

Use it with Claude Code, Codex, OpenCode, Gemini CLI, Cursor Agent, or a custom
harness. Jevia uses automatically verified CLI outcomes or results your application
reports; it is not tied to one model provider or agent runtime.

> Jevia is experimental. The CLI, typed routing contract, local outcome store,
> harness adapters, and Node.js SDK are available today. A managed control plane
> is not part of this repository.

## Why Jevia

Most routers stop after choosing a model. Jevia closes the loop:

```text
task -> route -> harness -> verification -> outcome -> future evidence
```

- **Adaptive:** verified outcomes inform later routing decisions.
- **Model-agnostic:** stable tiers map to whichever models your harness exposes.
- **Harness-agnostic:** use the CLI directly or embed the typed Node.js SDK.
- **Local-first:** policy, cache, and history stay in your project or database.
- **Explicit:** your verifier or feedback decides success; Jevia does not grade
  its own work.

## Quick start

Install the checksum-verified macOS or Linux binary without Rust or Cargo:

```bash
curl -fsSL https://jevia.dev/install.sh | sh
```

Initialize a project and check the full routing path:

```bash
jevia init
export TYPESAFE_API_KEY="your-key"
jevia check
jevia route "investigate the failing integration test"
```

For route-only integrations, optionally report a known result after executing the
work externally. This is not a required step after `jevia run`:

```bash
jevia feedback <run-id> success
jevia runs
jevia stats
```

`jevia doctor` checks local configuration and storage without making a Jev API
request. `jevia check` performs one live routing round trip without saving a
synthetic run.

## Run any harness

Jevia returns a capability tier. Your adapter maps that tier to a concrete model
and command for the harness you already use.

```bash
jevia harness setup agent --command my-agent \
  --arg=run --arg=--model --arg='{model}' --arg='{task}' \
  --model fast=provider/small \
  --model balanced=provider/standard \
  --model strong=provider/frontier \
  --verify-command cargo --verify-arg=test
```

The command previews the configuration first. Review it, repeat with `--apply`,
then run a routed task:

```bash
jevia run agent "fix the flaky integration test"
```

Jevia routes, executes, verifies, and records this run automatically. No manual
`feedback` or `runs complete` step is needed. CLI 0.1.4 can automatically discover
existing root Rust or Node tests when no verifier is configured. See the
[automatic CLI pipeline](docs/reference.md#automatic-cli-pipeline)
for detection, opt-out settings, and verification limits.

Harness and verifier processes are launched directly without shell
interpolation. Credentials remain in the environment instead of the project
configuration.

## Node.js SDK

Use the typed `jevia` package when your application owns harness execution:

```bash
npm install jevia
```

```ts
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd() });
const route = await jevia.route("fix the flaky integration test");

const result = await runYourHarness({ tier: route.tier });
// Optional: report a known outcome from your own adapter; no verifier required.
if (result.outcome === "success" || result.outcome === "failure") {
  await jevia.feedback(route.run_id, result.outcome);
}

// Eligible recorded outcomes inform the next decision automatically.
const next = await jevia.route("investigate another integration failure");
```

The SDK calls Jevia's shell-free JSON interface, so CLI and programmatic usage
share the same policy, storage, caching, and outcome rules. The CLI must already
be installed and available on `PATH`. Feedback and verification are optional:
skipping feedback leaves the outcome unknown, while future routes still use
other eligible recorded outcomes. No history argument or manual fetch is needed.

## Documentation

- [Product documentation](https://jevia.dev/docs) — install, adaptive routing,
  harness integration, SDK usage, outcomes, and operations.
- [Complete CLI and operations reference](docs/reference.md) — command details,
  storage migration, recovery, caching, and retention behavior.
- [Architecture](docs/architecture.md) — component boundaries and routing
  invariants.
- [Example configuration](crates/jevia-cli/examples/jevia.toml) — tiers, cache,
  harness mapping, and verification.
- [Node.js package reference](packages/jevia-node/README.md) — typed client API.

Run `jevia <command> --help` for command-specific options.

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
pnpm test
pnpm dev
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution and commit conventions.

## License

MIT
