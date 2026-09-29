# Jevia

Jevia is an open-source, local-first, outcome-based model router for coding
harnesses. It chooses a capability tier for each task, applies your safety
policy, and uses recorded execution history and optional outcome feedback for future routes.

Use it with Claude Code, Codex, OpenCode, Gemini CLI, Cursor Agent, or a custom
harness. Recording does not require extra tests or manual feedback; it is not tied
to one programming language, model provider, or agent runtime.

> The passive-recording workflow below is unreleased. CLI 0.1.4–0.1.5 enabled
> extra test discovery by default. The next CLI/SDK pair makes verification opt-in
> and adds schema-4 observations; upgrade all clients sharing a store together.

> Jevia is experimental. The CLI, typed routing contract, local outcome store,
> harness adapters, and Node.js SDK are available today. A managed control plane
> is not part of this repository.

## Why Jevia

Most routers stop after choosing a model. Jevia closes the loop:

```text
task -> route -> harness -> automatic observations -> future routing context
                            + optional feedback / verification -> known outcomes
```

- **Adaptive:** passive history and optional known outcomes inform later routing.
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

On Windows, use PowerShell 5.1 or newer:

```powershell
irm https://jevia.dev/install.ps1 | iex
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
and command for the harness you already use. Start from a built-in shell-free
template for Codex, Claude Code, OpenCode, or Gemini CLI:

```bash
jevia harness presets
jevia harness setup codex --preset codex \
  --model fast=your-fast-model \
  --model balanced=your-balanced-model \
  --model strong=your-strong-model
```

Presets supply only the executable and argument shape; model IDs, credentials,
permissions, and verification remain yours. For any other harness, use explicit
`--command` and repeated `--arg` values as shown in the
[adapter reference](docs/reference.md#preview-first-setup).

Setup previews the configuration first. Review it, repeat with `--apply`, then
run a routed task:

```bash
jevia run codex "fix the flaky integration test"
```

Jevia routes, executes, and records this run automatically. No manual `feedback`
or `runs complete` step is needed. Extra verification is opt-in: a process exit
remains an observed fact, not proof of task success. Existing explicitly enabled
checks are preserved. See the [automatic CLI pipeline](docs/reference.md#automatic-cli-pipeline).

Unreleased native capture supports Claude Code 2.1.251+, tested Codex 0.158.x
on macOS/Linux, and OpenCode v1 >= 1.18.33. Codex hooks require normal `/hooks`
trust review. It records reported models and tool/turn activity without storing
prompts or tool contents. Unsupported versions/remote sessions keep process facts.
Coverage is best-effort, not a claim that every model attempt or successful fix is
known. See [native capture limits](docs/reference.md#native-harness-observations).

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

// Recorded observations and eligible outcomes inform the next route automatically.
const next = await jevia.route("investigate another integration failure");
```

The SDK calls Jevia's shell-free JSON interface, so CLI and programmatic usage
share the same policy, storage, caching, and outcome rules. The CLI must already
be installed and available on `PATH`. Feedback and verification are optional:
skipping feedback leaves the outcome unknown, while future routes still use
recorded CLI observations and other eligible outcomes. No history argument or
manual fetch is needed. The SDK does not instrument an externally launched agent
merely because your application called `route()`.

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
