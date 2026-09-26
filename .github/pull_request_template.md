## Outcome

Describe the user-visible result of this change.

## Changes

- Describe the most important implementation change.

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] `pnpm --dir website type-check` (when the website changes)
- [ ] `pnpm --dir website build` (when the website changes)

## Risk and compatibility

Describe persisted-format, privacy, security, or compatibility implications.

## Review checklist

- [ ] The pull-request title uses a Conventional Commit prefix.
- [ ] New behavior is covered by tests.
- [ ] No credentials or sensitive run history are included.
- [ ] User-facing behavior is documented.
