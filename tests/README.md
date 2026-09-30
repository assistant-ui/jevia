# Native harness contracts

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

OpenCode exercises plugin loading, request-model metadata, neutral tool-completion
events, process recording, privacy, and journal cleanup. Codex exercises real
configuration parsing and the **untrusted** hook path: a successful execution
stays `unknown` and its hooks remain `no_events` until normally reviewed. The test
does not seed trust databases, disable policy, or bypass hook review. Positive
trusted-hook dispatch remains covered by the existing wrapper contract tests;
that is not a claim of real-binary trusted-hook end-to-end coverage.

Contracts: [OpenCode providers](https://opencode.ai/docs/providers/),
[OpenCode plugins](https://opencode.ai/docs/plugins/),
[Codex hooks and trust](https://learn.chatgpt.com/docs/hooks), and
[Codex custom providers](https://learn.chatgpt.com/docs/config-file/config-advanced#custom-model-providers).
