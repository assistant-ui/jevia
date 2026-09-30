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

## Upgrading to 0.1.7

The Rust core API, record schema 6, and configuration are unchanged from 0.1.6.
This coordinated patch carries CLI storage, replay, and diagnostic improvements;
recording/history reuse remain automatic and additional verification is optional.

## Upgrading to 0.1.6

This experimental patch adds observation configuration, execution observation
fields/types, whole-session counters, and `ExecutionRecording`. Review direct
struct construction and exhaustive enum matches when upgrading; this is not
source-compatible with every 0.1.5 caller. Omitted `auto_verify` now defaults to
false, while explicit verifiers and opt-in settings remain enabled.

New records use schema 6; schemas 1–5 remain readable. Upgrade all CLI/SDK readers
and writers sharing history together (CLI/core 0.1.6, Node SDK 0.1.2), and back up
history before upgrading. Native/application observations inform routing without
asserting task success; feedback and extra verification remain optional.

## Error privacy in 0.1.4

Provider and transport errors retain only safe categories, JSON locations, and
HTTP status codes. They no longer retain raw response values, endpoint URLs, or
underlying parser/network error sources. `JevError::Request` now carries a static
category, `InvalidJson` carries `line`/`column`, and `CacheKey`, `InvalidConfidence`,
`UnexpectedAnswerType`, and `UnknownTier` are unit variants. Callers matching
these experimental variants must update their patterns. Configuration validation
errors are separate and should not be logged with untrusted configuration values.

## Upgrading from 0.1.1

Version 0.1.2 adds `Config::storage` for opt-in SQL history backends. Despite the
patch version, callers constructing `Config` directly must add `storage` or use
`..Config::default()`. Existing serialized configs without a storage section
still default to JSONL; the library does not migrate history automatically.

For installation and end-to-end usage, see the
[Jevia guide](https://github.com/assistant-ui/jevia#quick-start).
