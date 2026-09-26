# jevia-core

Core routing contracts and the Jev client for
[Jevia](https://github.com/assistant-ui/jevia), an outcome-aware model router
for coding agents.

This library provides validated project configuration, capability tiers,
confidence-based fallback policy, routing and outcome records, execution and
verification evidence, and learning-aware cache fingerprints. It does not
launch harnesses or persist history; those responsibilities belong to the
`jevia` CLI.

```rust
use jevia_core::Config;

let config = Config::default();
config.validate().expect("default routing policy is valid");
assert!(config.tiers.contains_key(&config.router.fallback_tier));
```

Requires Rust 1.92 or newer. The API is experimental.

For installation and end-to-end usage, see the
[Jevia guide](https://github.com/assistant-ui/jevia#quick-start).
