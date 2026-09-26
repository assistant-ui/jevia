# Contributing to Jevia

## Development setup

Jevia uses the Rust toolchain pinned in `rust-toolchain.toml`.

Before opening a pull request, run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Branches and commits

Use a short branch prefix that describes the change:

- `feat/` for new behavior;
- `fix/` for bug fixes;
- `docs/` for documentation;
- `chore/` for maintenance.

Commits and pull-request titles follow Conventional Commits, for example:

```text
feat: add a local outcome store
fix: reject unknown fallback tiers
docs: explain privacy defaults
```

Keep commits focused and do not mix unrelated formatting or generated changes
into a feature commit. Pull requests should explain the user-visible outcome,
the verification performed, and any security or compatibility implications.

## Design principles

- Keep routing policy deterministic and inspectable.
- Treat model output as a signal, never as authority to execute an action.
- Keep credentials out of configuration files, logs, and errors.
- Preserve a local-only path; managed services must remain optional.
- Version persisted formats before changing them.
- Add tests for policy boundaries and failure behavior.
