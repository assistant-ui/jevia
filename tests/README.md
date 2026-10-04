# Native harness contracts

`cargo build --locked -p jevia && node --test tests/opencode-plugin.test.mjs`
tests the production OpenCode plugin with fault-injected collectors and the real
Rust journal/recovery path. It covers spawn errors, nonzero/signal exits, forced
timeout termination, loss-marker validation, and successful collection. These
offline tests do not require an installed harness or provider credentials and
run on Linux, macOS, and Windows in CI. They are not live-provider evidence.

`native-harness.test.mjs` runs actual OpenCode 1.18.33 and Codex
0.158.0-alpha.2 executables through `jevia run`. CI installs these exact versions
into its temporary directory. Locally, build the CLI and set
`JEVIA_NATIVE_BIN_DIR` to a directory containing the pinned executables, then run:

```sh
cargo build --locked -p jevia
node --test tests/native-harness.test.mjs
```

Without that environment variable the tests explicitly skip; CI sets it and a
missing/wrong binary fails. Child environments are allowlisted, homes/config/data
directories are isolated, and model endpoints are local HTTP fixtures. No real
provider credentials or paid generations are needed. The only generated tool
call prints a fixed marker; no model-generated code is executed.

The native contracts run on Linux and macOS. They need npm and registry access
for **setup**: the OpenCode fixture installs the real, version-matched
`@opencode-ai/plugin` dependency in its isolated config directory, with lifecycle
scripts disabled and a separate 60-second deadline. Missing dependencies fail
setup before any model task. Execution then sets npm offline mode and retains a
45-second Jevia deadline; setup and execution timings are reported separately.
This avoids treating a fresh-profile download as a recording timeout. Codex's
fixture disables unrelated marketplace plugins and startup update checks, not
Jevia hooks or their native trust review. No user profile is modified and Jevia's
production descendant-cleanup behavior is unchanged. See the upstream
[OpenCode dependency initialization](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/opencode/src/config/config.ts)
and [Codex feature configuration](https://learn.chatgpt.com/docs/config-file/config-reference).

OpenCode exercises plugin loading, request-model metadata, neutral tool-completion
events, process recording, privacy, and journal cleanup. Codex exercises real
configuration parsing and both **untrusted** and **reviewed** hook paths. The
negative test retains `no_events`. The positive test uses Python 3's standard
PTY library to drive the real CLI's hook-review UI inside its credential-free
fixture home, approving only the eight Jevia hooks. It then executes a real tool
call and checks saved tool/turn events while the outcome remains `unknown`.
The test never seeds trust hashes, edits trust state, or bypasses review. A changed
UI or unexpected hook count fails closed. Python 3 and a Unix PTY are required.
On Ubuntu 24.04, CI installs the distribution `bubblewrap` helper and its scoped
AppArmor profile following the [native sandbox prerequisites](https://learn.chatgpt.com/docs/sandboxing).
The test keeps the read-only sandbox and requires a zero-exit tool result with
the marker at the next provider turn; an attempted tool event alone is not enough.
It does not disable the system-wide user-namespace restriction.

Contracts: [OpenCode providers](https://opencode.ai/docs/providers/),
[OpenCode plugins](https://opencode.ai/docs/plugins/),
[Codex hooks and trust](https://learn.chatgpt.com/docs/hooks), and
[Codex custom providers](https://learn.chatgpt.com/docs/config-file/config-advanced#custom-model-providers).

## Opt-in live checks

`scripts/live-harness-smoke.mjs claude|codex|opencode` uses real Jev routing and
real model calls. Set `JEVIA_LIVE_TEST=1`, load `TYPESAFE_API_KEY` privately, and
choose available models using `JEVIA_LIVE_CODEX_MODEL` or
`JEVIA_LIVE_OPENCODE_MODEL` (Claude defaults to `sonnet`). The harness executables
must be on `PATH`. Recordings stay in the printed disposable project, including
on failure. It also checks harness health and makes a second live Jev
classification that must automatically load the recorded passive observation.
No extra verifier or manual feedback is used.

For Codex, set `JEVIA_LIVE_CODEX_PROFILE` to an isolated authenticated profile.
First run with `--prepare`, then use that same profile and Jevia executable to
run `jevia harness review codex --launch` in the printed project. Approve only
the reviewed Jevia hooks and exit; run the smoke test without `--prepare`.
Do not add `--ignore-user-config`: it discards the trust decisions too. The test
does not approve hooks or copy credentials automatically. It must receive real
tool and turn events to pass; successful process exit alone is insufficient.
