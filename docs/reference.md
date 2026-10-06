# Jevia reference

## CLI 0.1.9: routing context and native reliability

CLI 0.1.9 includes the selected harness's tier/model candidates in routing,
caps each outgoing history window at 64 KiB, and streams history projections
and SQL stats rows to reduce memory use. It fixes literal native task arguments,
transient version probes, and inconclusive verifier outcomes, and preserves
pending recordings during maintenance. Exported SQL snapshots remain restorable
within the existing 8 MiB record limit.

Native contract tests use actual pinned Codex/OpenCode executables with local
responses: reviewed hooks, cancellation, provider rejection, resume with a changed
model, and OpenCode foreground subagents. They do not establish live-provider
task-solving accuracy or every interactive/background workflow.

Record schema 6, SQL schema 1, configuration, and automatic history reuse are
unchanged; feedback and extra verification remain optional. Node SDK 0.1.3 stays
compatible without a new npm release. Rust core users should review the new
`JevError::UnknownHarness` variant when matching errors exhaustively.

## CLI 0.1.8: native capture and storage safety

CLI 0.1.8 fixes live Claude capture, adds preview-first Codex hook review and
`harness health --require-events`, and improves native startup and OpenCode
tool-failure recording. It also rejects special history inputs, bounds JSONL
records and provider responses, and improves recovery and cleanup performance.
Record schema 6, SQL schema 1, and recording/verification defaults are unchanged.
Node SDK 0.1.3 remains compatible; no SDK upgrade is needed for these CLI fixes.
Back up oversized history before explicit maintenance: streamed JSONL physical
lines are now limited to 8 MiB and are never silently skipped or truncated.

## CLI 0.1.7: storage, replay, and diagnostics

CLI 0.1.7 adds bounded-memory JSONL updates, consistent routing snapshots,
passive-history SQL indexes, bounded replay and lock waits, recording maintenance,
terminal-safe application output, routing explanations, and structured storage
checks. Node SDK 0.1.3 adds `checkStorageReport()` for CLI 0.1.7 or newer.
Record schema 6 and SQL schema 1 are unchanged; automatic recording/history use
and optional feedback/verification keep their existing defaults.

## CLI 0.1.6: passive routing context

### Application-owned execution recording

`jevia runs record-execution <run-id> --json` reads a single bounded JSON object
from stdin (`harness`, requested `model`, `duration_ms`, optional `exit_code`, and
optional `events`). Node SDK 0.1.2 exposes `recordExecution()` with the same fields
and requires CLI 0.1.6 or newer.
Call only after app-owned work has stopped. This is optional; it does not launch
an agent, run verification, or require feedback. `jevia run` already records its
own execution and must not be reported again through this API.

The operation records one completed application-owned attempt. Its outcome stays
unknown unless separately assessed; existing manual feedback remains unchanged.
Observations are explicitly sourced as `application`, not trusted native hooks.
An exact retry is idempotent; conflicting snapshots, legacy runs without an
explicit routed lifecycle, active runs, and existing native executions are
rejected. JSONL, SQLite, and PostgreSQL apply the same atomic storage contract.
Later routing automatically includes the passive summary and invalidates cached
decisions based on older history. No history plumbing or cache clearing is needed.

Inputs are limited to 256 events / 256 KiB, allowlisted identifier fields, safe
integer timestamps/duration, and an optional signed 32-bit exit code. Raw text,
tool inputs/outputs, arbitrary event fields, outcome labels, and verifier evidence
are rejected. The new `application` source uses record schema 6 (older schemas
remain readable); upgrade all readers/writers together. See the
[SDK example](../packages/jevia-node/README.md#optional-passive-execution-recording).

### Recording health

Run `jevia harness health codex` (or `claude` / `opencode`, using your configured
adapter name). Add `--json` for the versioned, redacted report. It shows the current
capture mode/configuration separately from the most recent matching execution in
the last 100 stored runs: source, capture status, cumulative events, discarded
inputs, sampled-event timestamp, and actionable advice. `ok` means inspection
succeeded, not that capture is complete or the task succeeded.

This command reads your selected JSONL, SQLite, or PostgreSQL storage but never
launches a harness/version probe, contacts a model, replays journals, or runs
verification. `jevia harness check` remains the offline static preflight. An empty
window, legacy record, or `no_events` status is not proof of a broken adapter.
In particular, Codex hooks may need normal `/hooks` trust review; diagnostics do
not bypass it or claim to know that trust is the cause. Current config may differ
from the saved run. Raw prompts, events, model/session identifiers, commands,
credentials, and underlying storage error details are omitted.

### Shutdown durability

Final journal read failures preserve the most recent checkpoint and mark capture
partial. Terminal state writes cannot replace newer durable observations with an
older snapshot. Unreadable, corrupt, or subsequently changed journals remain on
disk for recovery; cleanup only removes a journal matching the saved snapshot.
This does not infer task success or run additional verification.

### Private recording files and Git

Project-local `.jevia/.gitignore` rules cover history, caches, native journals,
loss markers, journal-checkpoint temporary files, event locks, and generated
adapter plugins. `config.toml` and the ignore file itself remain trackable so
policy can be reviewed. Before native capture creates artifacts, Jevia repairs
missing rules in older projects without requiring `init --force` or changing
configuration. Existing comments/custom rules are retained; cooperating writers
are serialized and updates are atomic.

If ignore-rule repair fails, native capture stays unavailable and process-level
recording continues; no native journal/plugin is created by that attempt.
Git ignore rules prevent ordinary accidental additions, not access to local data.
They do not untrack files already committed or staged, delete existing files, or
override explicit force-adds; custom Git rules can also negate exclusions.
Review any previously tracked `.jevia` runtime data
separately. Retained journals can contain allowlisted model/session/tool metadata
even though raw prompts and tool contents are not captured.

### Passive model reporting

In CLI 0.1.6, `jevia stats` / `jevia stats --json` includes a separate
`observations` section over the same bounded history window. It reports coverage
(`recorded`, `partial`, `no_events`, `unavailable`, `disabled`, `unsupported`, or
legacy `not_reported`), active checkpoints, whole-session event counts, sampled
runs, and per-observed-model run/event counts. Requested aliases are never used
as attribution. Events lacking a reported model remain unattributed; models beyond
the 128-group report limit contribute to omitted-model counts. The report signals
group truncation and counters saturated at JavaScript's maximum safe integer.

Tool errors and model switches are operational facts, **not task failure rates**.
Existing verified/manual outcome metrics are unchanged; there is no inferred
per-model success score or latency attribution. No task text, session/agent IDs,
commands, or transcripts enter the report. Reporting is read-only, including for
live checkpoints, and uses whole-session totals when present (legacy samples
remain supported). No feedback or extra verification is needed for these metrics.

### Native harness observations

CLI 0.1.8 `jevia run` enables native capture automatically for supported direct
executables (including `.exe` names). It probes `--version` with a five-second
deadline (15 seconds for the slower OpenCode launcher) before adding a session-local adapter. Probes own a process group/job,
accept at most 256 bytes of successful UTF-8 output, and stop descendants on
failure, timeout, or cancellation. Failed probes allow up to one additional
second for cleanup and never expose captured output. No user/project config is
rewritten. Set `observations = "off"` to disable native capture; process recording
stays on. Other harnesses, including Gemini, retain process facts only.

CLI 0.1.9: transient probe exit/read/wait/deadline failures are retried once,
only after the first process group/job is confirmed stopped. Each attempt keeps
the deadline above and up to one second for cleanup (at most 12 seconds for
Claude/Codex or 32 for OpenCode). Missing or inaccessible executables, oversized
or non-UTF-8 output, unsupported versions, and unconfirmed cleanup do not retry.
Diagnostics identify the failure category without printing subprocess output.
There is no persistent support cache: replacing an executable triggers a fresh
check on the next run. A failed probe still preserves process recording, reports
native capture as unavailable, and never invents events or a success outcome.

| Harness | Auto-detection contract | Capture and limits |
| --- | --- | --- |
| Claude Code | 2.x >= 2.1.212 | Silent exec-form hooks; preserves custom `--settings`, `--bare`, and `--safe-mode` by skipping injection. Managed settings may disable hooks. Version 2.1.212 was verified with a real file-read task and session/tool/turn events. |
| Codex | 0.158.x stable or the tested 0.158.0-alpha.2, macOS/Linux | Inline session hook overrides. `exec`/`review` run locally; interactive launches get `--no-daemon` to isolate the journal environment. Existing `-c`/`--config`, `--disable`, remote/attach/server arguments skip injection. Windows stays process-only. |
| OpenCode | v1 >= 1.18.33, < 2 | Private dependency-free `.mjs` plugin appended through `OPENCODE_CONFIG_CONTENT`; existing valid JSON overrides and plugin entries are preserved. Invalid/JSONC inline overrides, `--pure`, and remote/attach/server launches stay process-only. |

Compatible wrappers can explicitly set `observations = "claude_hooks"`,
`"codex_hooks"`, or `"opencode_plugin"` in their harness table. These bypass the
version probe, not platform/config safety checks; the wrapper must honor the same
arguments, environment, and event contract. Unknown future Codex minors are
deliberately not auto-enabled until checked against the adapter contract.

**Codex hook trust:** review Jevia's hooks using `/hooks` before expecting native
events. Jevia never bypasses trust, enables disabled hooks, changes approval
policy, or installs global configuration. The command definition stays stable
across runs; changing Jevia's executable path can require renewed trust. Skipped
hooks can leave `no_events` even when process recording works. The
[official hook contract](https://learn.chatgpt.com/docs/hooks) describes trust and
event fields. Codex `PostToolUse` becomes neutral `tool_completed`, because it can
also fire for nonzero shell exits. `Interrupt` is `turn_interrupted`, not task
failure. Reported model IDs attach only to the event that contains them; no
cross-subagent model-switch inference is made.

For the standard Codex preset, use `jevia harness review codex` to preview the
review flow, then `jevia harness review codex --launch` in a terminal. This opens
a local read-only Codex session with the same Jevia hook definitions. Open
`/hooks`, inspect the `capture-event` commands and approve only those you accept,
then quit. Jevia submits no task, routes nothing, and does not save this review
session as task evidence. Only Codex persists your explicit trust choice. The
preview does not execute anything; custom presets and disabled capture are not
silently rewritten. Use the same installed Jevia binary for review and runs:
changing its path or hook definitions can require renewed review.

Regular Codex observation hooks allow five seconds for cold process startup;
`SessionEnd` and `Interrupt` use the harness's three-second maximum. Jevia never
adds `--dangerously-bypass-hook-trust` automatically. A live test using an
explicitly approved one-run bypass confirms adapter dispatch, **not** persisted
trust for later runs.

The run summary reports actual capture coverage (`observations=recorded`,
`no_events`, `unavailable`, `partial`, `disabled`, or `unsupported`) and the
observed event count. Saving process metadata does **not** mean native events
were captured. A zero-event run warns explicitly; it remains an unknown task
outcome, even when the process exits zero. `recorded` means some events arrived,
not proof of complete coverage or a successfully solved task.

#### Opt-in live capture check

After `cargo build --release -p jevia`, run the real-provider smoke check with
existing authenticated harness CLIs and `TYPESAFE_API_KEY` in the environment:

```bash
JEVIA_LIVE_TEST=1 node scripts/live-harness-smoke.mjs claude
JEVIA_LIVE_TEST=1 JEVIA_LIVE_CODEX_PROFILE=/path/to/isolated-profile JEVIA_LIVE_CODEX_MODEL=your-model node scripts/live-harness-smoke.mjs codex
JEVIA_LIVE_TEST=1 JEVIA_LIVE_OPENCODE_MODEL=provider/model node scripts/live-harness-smoke.mjs opencode
```

This makes potentially paid API calls, uses a disposable synthetic project,
and prints the path to its actual `runs.jsonl`. It never injects synthetic
events or bypasses hook trust. A successful process with zero native events
**fails** the check. It requires both a tool event and turn completion, and
leaves the project and recording available for inspection. It checks harness
health and makes a second live classification to verify automatic history reuse
without feedback or extra verification. All tiers use the
same selected harness model to isolate capture behavior; this is not a model
quality benchmark. The Claude call has a $0.50 budget; all have a 90-second
harness deadline. The separate CI native-contract tests use loopback model
responses and are not evidence of a successful live-provider run.

For Codex, authenticate the isolated profile with your authorized credentials,
run the smoke script with `--prepare`, then open `jevia harness review codex
--launch` in the printed project with `CODEX_HOME` pointing to that profile. Use
the same Jevia executable when reviewing and testing. Review the capture-event
commands, approve only those you accept, exit, and rerun without `--prepare`.
Do not use `--ignore-user-config` for this test: it also discards saved hook trust.
Neither the smoke script nor Jevia bypasses trust or approves hooks automatically.

OpenCode error tool parts produce `tool_failed` facts; repeated updates for the
same part are counted once. The adapter retains at most 1,024 opaque failed-part
IDs per session; beyond that it reports partial capture instead of guessing
counts. Error text, tool arguments, and tool output are not stored. These are
tool-level errors, not assessed task failures. Child `PWD` is synchronized with
the configured project directory for probes, interactive, and supervised runs.

#### Before a demo

Install CLI 0.1.9 with `cargo install jevia --version 0.1.9 --locked` and confirm
`jevia --version`. Alternatively, build this source with
`cargo build --release -p jevia`. The smoke script defaults to
`target/release/jevia`; set `JEVIA_TEST_BINARY` to the absolute installed binary
path to test that copy. Complete Codex's hook review with the same binary, run a
small real task, and then check the **saved** event coverage:

```bash
jevia harness health codex --require-events
jevia harness health claude --require-events
jevia harness health opencode --require-events
```

`--require-events` exits nonzero when no matching execution exists or its capture
is empty, unavailable, disabled, unsupported, or partial. The saved recording must
also identify a native source (`claude_hooks`, `codex_hooks`, or `opencode_plugin`);
application/SDK events and records without a source do not satisfy this check.
SDK recording and feedback remain optional, and recorded application history
continues to inform routing. With `--json`, the
report schema stays unchanged: `ok` describes reading the report, while the exit
status additionally enforces the event requirement. This checks the latest saved
execution, not the current configuration, a fresh launch, all possible event
types, or task correctness. Inspect the run ID so an old successful recording
cannot be mistaken for the run you are demonstrating. Do not advertise a harness
as live-verified merely because its contract tests or process exit succeeded.

OpenCode uses its [plugin API](https://opencode.ai/docs/plugins/) and
[runtime config override](https://opencode.ai/docs/config/). The adapter contract
is covered by fixtures against the
[v1.18.33 hook types](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/plugin/src/index.ts).
`chat.params` produces `model_observed` for the model selected for that request,
not proof of a provider completion. `tool.execute.after` produces neutral
`tool_completed`; tool output is never interpreted. Session created/idle/error
events and submitted messages provide activity boundaries; streaming message
updates are ignored to avoid counting each update as an attempt. There is no
guarantee of tool-error, subagent-completion, model-switch, or session-end coverage.
Private plugin files are removed after terminal persistence. A crash may leave
an inert `jevia-observer-*.mjs` file alongside the retained journal; replay does not
execute it. Native adapters are contract-tested with local fixtures, not a claim
of exhaustive live-provider coverage.

`execution.observations` contains a source, coverage status, the most recent 256
allowlisted events, and whole-session `totals`. Counters continue after the event
sample fills. `totals.models` attributes event counts only to models explicitly
reported on those events; missing models stay in `unattributed_event_counts`.
At most 32 model identifiers are retained. Further models contribute to
`omitted_model_event_counts` and set `models_truncated`, without unbounded growth.
When totals are present, each sampled event must fit its named-model,
unattributed, or omitted-model counter. Contradictory attribution is rejected by
history reads/imports and the Node SDK; it is never silently reassigned or repaired.
Legacy observations without totals remain readable.
Model switches are observations, not failures. Counts are observed activity, not
task scores; partial capture/legacy truncated journals provide lower bounds.
Supported events include session/turn boundaries, tool
success/failure or neutral completion, reported task completion, subagent boundaries, and session model
changes. A model is saved only when explicitly present in an event; configured
`execution.model` remains the requested model. No outcome is inferred from these
events. Tool success and an agent marking a task complete are not proof of a fix.

The [Claude hook contract](https://code.claude.com/docs/en/hooks#postmodelswitch)
does not report temporary per-turn fallback-chain substitutions via model-switch
hooks. Do not treat these observations as a complete model-attempt ledger, token
accounting, or proof of which model solved a task. `recorded` means some events
arrived, not that all activity was captured. `unsupported`, `disabled`,
`unavailable`, `no_events`, and `partial` expose other coverage states.

Hook envelopes are capped at 8 MiB, including trailing whitespace. Large tool
bodies no longer discard otherwise valid metadata merely for exceeding 64 KiB.
The decoder borrows raw field spans instead of allocating JSON trees for tool
content; only allowlisted identifiers of at most 256 ASCII bytes, event kinds, and
ingestion timestamps are retained. The envelope buffer is transient and bounded.
Prompts, assistant messages, tool arguments,
tool output, transcript paths/files, and error bodies are not stored. Malformed or
oversized envelopes (including duplicate metadata keys) are discarded and counted;
detected gaps mark capture partial. Invalid optional identifiers are omitted.
The raw event sample rolling over does not stop aggregation. Routing receives the
whole-session counts and bounded model identifiers, with `summary_truncated` when
the raw sample or model details were bounded. Journals are atomically replaced
under stable striped locks, so a torn replacement leaves the previous checkpoint.
Hooks never return agent instructions or deny permission. Lock contention, hook
timeouts, and asynchronous events after session exit can lose events: capture is
best-effort, not an audit log.
When a hook detects a failed write (including lock contention), it attempts to
create one private, empty `.loss` sidecar beside its journal. The next checkpoint
or replay reports `partial` coverage and adds one unconfirmed input to
`discarded_inputs`, regardless of repeated failures. This means at least one
capture attempt failed; a write may have reached disk before reporting an error,
so it is not an exact count or proof that an event is absent. Re-reading does not
increase that count; recorded events and task outcomes remain unchanged. No
raw input is queued. The marker and journal are removed only after the matching
snapshot is saved. If storage also prevents the marker, the hook emits a fixed,
redacted diagnostic and still exits zero. A killed hook or an event arriving after
cleanup can remain unobservable; this is not a guarantee of complete capture.

The OpenCode plugin also attempts this marker when its collector cannot start,
exits unsuccessfully, loses its input pipe, or exceeds its 1.5-second timeout.
A timed-out collector gets a short termination grace period before forced stop.
Marker writes validate a bounded, regular OpenCode journal and never store the
failed payload. If marking fails, the plugin warns once per session with a fixed,
redacted message and still allows the agent to continue. Abruptly stopping the
plugin/harness itself can prevent this accounting; capture remains best-effort.

A private journal under `.jevia/jevia-events-<run-id>-*.jsonl` is checkpointed into
the selected JSONL/SQLite/PostgreSQL run record while the harness runs (at most once
every two seconds when observations change), and saved again when it stops.
Checkpoint failures do not fail the harness; the synced journal remains available.
Pending checkpoints do not suspend process supervision: non-interactive timeouts
and cancellation continue to stop the owned process tree. JSONL checkpoints skip
a busy history lock and retry later; final history persistence can still wait for
that lock after the process has stopped. Delayed supervisor checkpoints cannot
overwrite a terminal run.
Before the next route/run, Jevia makes a bounded, best-effort replay attempt for
retained journals, skipping runs whose execution lease is still held. Replaying a
cumulative snapshot does not duplicate events, alter feedback, or mark a run
complete: loss of the supervisor is not proof its child stopped. Active journals
remain available for later events. Read-only `stats`/`runs show` do not replay.
`runs recover` also replays after its existing lifecycle safety checks (including
PostgreSQL's `--confirm-stopped`). Journals are removed only after terminal
persistence; corrupt, ambiguous, or unavailable journals are retained for inspection.
Replay retains at most 128 candidate runs (two journal names per run suffice to
detect ambiguity), with a two-second total budget before routing. It enumerates
the complete directory before acting, without a fixed directory-entry ceiling;
this is not an unbounded repair job.
The same budget applies to recovery replay. Busy journal/history locks are skipped
immediately, not waited on once per journal; an expired attempt retains its files
for a later invocation. File scanning and journal/history reads run off the async
task so the replay timer remains responsive.
Since CLI 0.1.7, a private atomic cursor in `.jevia/replay-state/`
rotates this candidate window between invocations, including past busy, corrupt,
or ambiguous attempts. Concurrent replay passes do not race the cursor; explicit
`runs recover <id>` does not move it. The two-second budget still applies.
Incomplete directory scans (including a read error or exhausted budget) defer replay
without deleting anything, because an unseen duplicate could make a journal
ambiguous. Slow directories may still require explicit inspection/maintenance; the cursor is
not an unbounded background repair service or a guarantee against slow storage.
Routing receives bounded event counts and model summaries, not session IDs/raw events.

In CLI 0.1.7, replay's JSONL workers also check the remaining budget between read
chunks and records, after decoding, and before publishing a checkpoint rewrite.
Expiry releases the history lock and discards unpublished temporary output; the
journal remains available for retry. This is cooperative cancellation, not a hard
real-time guarantee: a blocking filesystem call, decoding one large record, or
an atomic publication already in progress cannot be preempted. Required routing
history still validates completely; a replay timeout never substitutes partial
history or invents an outcome. Ordinary readers reuse their line buffer as well.

#### File-lock contention

Cache lock acquisition waits at most 500 ms per operation; a busy routing cache
falls back to a live request without overwriting that cache. JSONL history lock
acquisition waits at most two seconds per operation, then returns an explicit
`run history busy` error. Required history is never silently replaced by an empty
window. This also bounds lock acquisition for writes and maintenance; a failed
terminal write retains the journal and may require explicit recovery after the
other writer finishes. Locks are never force-unlocked or deleted.
These are lock-acquisition budgets, not whole-command, filesystem-I/O, SQL-query,
or harness deadlines. Cache expiry is checked after loading the locked cache.

New records use schema 6 (schemas 1–5 stay readable). Older CLI versions reject
newer schemas rather than silently erasing metadata. Back up history and upgrade
all clients sharing a store together to CLI 0.1.6 and Node SDK 0.1.2. SQL database
schema 1 is unchanged. Do not downgrade clients against updated history.

### History and task outcomes

Finished `jevia run` executions automatically inform subsequent routing even without
feedback or a verifier. Jev receives two separate, bounded windows: known outcomes
and passive execution observations (requested model, harness, elapsed time, process
exit, and lifecycle state). Each uses `router.history_limit`; pending and active
runs do not crowd out either window. Observations that change the supplied context
invalidate stale routing cache entries. Stored task text follows the existing privacy setting.

CLI 0.1.9: routing also limits each history window to **64 KiB of serialized
JSON** (128 KiB combined, including array punctuation). Historical task text is a
UTF-8-safe prefix of at most **2,048 bytes**, explicitly marked `task_truncated`
when shortened. Within each count window, newer records get priority; a record
that cannot fit is omitted, and smaller older candidates can still fit. No
records beyond `router.history_limit` are fetched to backfill omissions. Jev
receives `history_budget` counts and instructions not to treat missing context
as a failure. Both known outcomes and passive observations remain automatic.

The CLI reuses one bounded projection snapshot for routing, cache keys, and
diagnostics. JSONL still validates the full stream under one shared lock. Small
histories are projected in one pass; after a count window fills, only byte offsets
are retained for later candidates. Selected records are reread from the same open
file under the same lock. Projection work is capped at twice `history_limit`
per window, rather than scaling with all eligible records; validation remains
linear in retained history size. SQL streams selected rows from one consistent snapshot. Full task
text and feedback history are not retained in the routing windows. The existing
Rust slice-based APIs remain available, alongside `RoutingCandidate`,
`RoutingHistory`, `route_with_history`, and `route_cache_key_with_history` for
streaming integrations. See `scripts/benchmark-routing-history.mjs` for a
repeatable synthetic before/after memory check; it makes no live model calls.
`scripts/benchmark-jsonl-projections.mjs` compares release binaries on small and
large JSONL histories, alternates timed runs, and asserts identical provider
request bytes. It uses only a synthetic loopback provider.

This affects only outbound historical context, not saved history, the current
task, or the prompt passed to a harness. It is not a cap on the entire request
(current task/configuration are separate), storage record size, or memory needed
to read stored records. The shared core projection applies to the CLI, Node SDK
with the updated CLI, and Rust client.

In CLI 0.1.7, both windows come from one consistent history snapshot.
Concurrent feedback cannot move an attempt between categories midway through
the read. JSONL validates the full file under a shared lock and retains only the two
bounded windows; SQL uses a read snapshot without holding a project write lock.

A process exiting zero or nonzero leaves task `outcome` as `unknown`; its exit code
is still recorded and returned by the CLI. A known outcome requires optional manual
feedback or an explicitly requested verifier. Historical process-derived labels
remain readable but are not promoted into quality evidence. The configured model
is labeled `requested_model` in passive routing context, not an observed model.

This is the complete CLI and operations reference. Start with the
[project README](../README.md) or the [product documentation](https://jevia.dev/docs)
for the shorter installation and integration path.

CLI 0.1.7 escapes control characters in human-readable routing/run
metadata, feedback confirmations, and harness names. Imported values cannot add
terminal commands or spoof extra output lines through these displays. Stored
values, harness arguments, and JSON/SDK responses retain their original contents;
output streamed directly from a child harness is not filtered. Application error
chains and cache warnings are escaped at their final rendering boundary, including
unknown-harness names and filesystem errors. Human-readable project/backup/archive
paths are escaped without changing the paths used for filesystem operations.
Ordinary quotes, backslashes and Unicode stay readable in these diagnostics.

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
4. record the routing decision in the configured history store;
5. automatically record execution facts and supported native events for `jevia run`;
6. include passive history and optional known outcomes in future routing decisions.

The application owns the policy. Jev supplies a structured decision signal.

Jev routing responses are limited to 1 MiB, including chunked responses and
responses without a content-length header. An oversized response fails safely
without caching a decision or recording a routed run. Request timeouts still apply.

## Quick start

Use the copyable installer on the [Jevia landing page](https://jevia.dev). It
downloads a checksum-verified prebuilt binary; Rust and Cargo are not required.
On macOS, Linux, or WSL:

```bash
curl -fsSL https://jevia.dev/install.sh | sh
jevia --version
jevia init
export TYPESAFE_API_KEY="your-key"
jevia check
jevia route "investigate an intermittent distributed-lock failure"
```

On native Windows, use PowerShell 5.1 or newer, then run the same `jevia`
commands:

```powershell
irm https://jevia.dev/install.ps1 | iex
jevia --version
jevia init
$env:TYPESAFE_API_KEY = "your-key"
jevia check
jevia route "investigate an intermittent distributed-lock failure"
```

`jevia check` makes one live Jev request and requires a valid API key. Use
`jevia doctor` for configuration/storage checks without a Jev API request.
PostgreSQL storage checks do connect to the configured database.

To install the exact crates.io release with Rust 1.92 or newer:

```bash
cargo install jevia --version 0.1.9 --locked
```

To try unreleased development changes instead:

```bash
cargo install --git https://github.com/assistant-ui/jevia --locked jevia
```

### Run an agent: automatic outcome recording

After [configuring your installed harness once](#harness-adapters), use
`jevia run` for the end-to-end flow:

```bash
jevia run codex "fix the failing test"
jevia runs
```

Jevia routes the task, launches the agent, and saves execution facts automatically.
**No manual `feedback` or `runs complete` step is needed.** Recorded observations
and optional known outcomes inform later routing. The name `codex` must match your configured adapter;
Jevia does not install or authenticate the agent for you.

**CLI 0.1.6 default change:** additional verification is now opt-in. Recording
still happens automatically. CLI 0.1.4–0.1.5 enabled test discovery by default;
CLI 0.1.6 only discovers tests when `auto_verify = true` is explicitly
configured. See [automatic CLI pipeline](#automatic-cli-pipeline)
for detection, overrides, and verification limits.

### Route only: your integration owns execution

`jevia route` (used in the connection quickstart above) selects a tier but does
not execute an agent or verify a task. If your integration knows the result of
work executed outside `jevia run`, it can optionally report that outcome:

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
| `jevia harness presets` | List the built-in Codex, Claude Code, OpenCode, and Gemini CLI command templates. |
| `jevia harness setup <name>` | Preview an explicit harness template; back up and save only with `--apply`. |
| `jevia harness check <name> [--json]` | Inspect configuration and local executable candidates without launching programs or calling APIs. |
| `jevia harness review <name> [--launch]` | Preview or explicitly open a read-only native Codex hook-review session; never auto-approve trust. |
| `jevia harness health <name> [--json] [--require-events]` | Inspect recent saved capture; optionally fail if native events are missing or partial. |
| `jevia route <task>` | Ask Jev for a tier and record the decision. |
| <code>jevia run &lt;harness&gt; &lt;task&gt;</code> | Route, launch, and record automatically; additional verification is opt-in. |
| `jevia runs` | Inspect recent records in the configured backend. |
| `jevia stats [--limit <records>] [--json]` | Summarize recent routing decisions, verified outcomes, manual feedback, and cache hits. |
| `jevia feedback <id> <outcome>` | Report externally verified work or manually correct an outcome; not required after `run`. |
| `jevia runs complete <id> <outcome> --confirm-stopped` | Explicitly finish external work with manual evidence. |
| `jevia doctor` | Validate configuration, credentials, and configured storage. |
| `jevia storage setup <sqlite\|postgres>` | Preview database setup; explicitly apply after validation, backup, and optional JSONL import. |
| `jevia storage init` | Explicitly initialize an opt-in database schema and project. |
| `jevia storage check` | Check storage without needing a Jev API key. |
| `jevia storage check --deep` | Inspect all history records and SQL routing/order metadata without a write probe or repair. |
| `jevia storage import-jsonl [--from <file>] [--apply]` | Preview/import local history into a database without changing the source. |
| `jevia storage export --output <file>` | Export history to a new JSONL snapshot; never overwrite a file. |
| `jevia check` | Validate configured storage and complete a live Jev routing round trip without storing a run. |
| `jevia cache status` | Inspect routing-cache settings and entry counts. |
| `jevia cache clear` | Remove cached decisions without touching run history. |

Run `jevia <command> --help` for command-specific options.

The human-readable `runs` listing reports `verification=pass` only for a launched
verifier with exit code 0, and `fail` only for a launched verifier with a nonzero
exit code. Missing exit status (including timeout, cancellation, or a signal) and
launch failures remain `unknown`; no configured verification is `none`. This
display does not change saved outcomes, lifecycle state, or JSON output.

## Routing insights

```sh
jevia stats
jevia stats --limit 500
jevia stats --json
```

`stats` reads the configured JSONL, SQLite, or PostgreSQL history without calling
Jev, launching a harness, changing outcomes, or reading the decision-cache file.
It needs no Jev API key; PostgreSQL still requires its configured database
connection. Initialize an opted-in database with `jevia storage init` first.
This command requires v0.1.2 or newer.

The default window is the **latest 1,000 records in append order**, not a date
range or an all-time total. `--limit` accepts 1–100,000. The report says when older
records were excluded; archived records are not included. Stats aggregate each
record without retaining the window's full task, feedback, and event payloads.
SQL reads are project-scoped and paged in batches of at most 200 records within
one consistent snapshot. CLI 0.1.9 SQL stats decode and discard each row as it
arrives instead of materializing a whole page; bounded driver prefetch still
contributes to memory usage. JSONL counts nonblank lines, then scans and validates
the full file under the same shared history lock and file handle, decoding each
record once. Working memory depends on an input line/driver buffer and the
aggregate groups, not the combined payload size of the requested window. Tier
groups remain complete; reported model groups retain their existing 128 limit.

Run `node scripts/benchmark-stats-memory.mjs BEFORE_BINARY AFTER_BINARY` on Linux
or macOS for a synthetic, credential-free comparison. It measures peak RSS and
checks identical stats for JSONL and SQLite with 200 records containing 512 KiB
task strings. Results are local measurements, not a fixed memory guarantee.

Totals and per-tier groups use the **selected tier recorded at routing time**,
including tiers since removed from configuration. Tiers are not concrete model
identities: changing a harness's model mapping does not split historical groups.
Each record counts once, using its current outcome and provenance:

- **Verified success rate:** verifier-backed successes divided by verifier-backed
  successes plus failures. Manual feedback, process-exit-only results, active
  runs, unknown outcomes, and legacy outcomes without provenance are excluded.
  A configured verifier's result is not a guarantee of task correctness.
- **Manual feedback:** separate success/failure counts. Correcting a verified
  outcome with manual feedback moves that record to the manual group; prior
  feedback events and the old verifier result are not counted again.
- **Learning evidence:** known, non-active verifier-backed or manual outcomes,
  using the same eligibility rule as routing. This is the eligible count in the
  stats window, not necessarily the smaller evidence window sent to Jev.
- **Cache hits:** recorded cached decisions divided by all records in the window,
  not current cache occupancy or a count of API calls. Bypassed/disabled-cache
  decisions still count in the denominator. Legacy records without a decision
  source use the existing `live` default.
- **Other outcomes:** process-exit-only and unattributed known outcomes, plus
  active and unknown counts. Active takes precedence over unknown, so the outcome
  groups partition the records without double-counting.

Rates with no eligible observations display `n/a`, not 0%. The JSON report uses
`schema_version: 1`, `storage`, `window` (`limit`, `order`, `has_older_records`),
`totals`, and a tier-keyed `tiers` map. Rates are fractions from 0 to 1, or `null`
when their denominator is zero. `verified`, `manual`, `process_exit`, and
`unattributed` each contain `successes` and `failures`; `active` and `unknown`
are separate counts. Output excludes task text, run IDs, feedback notes,
execution commands, and connection URLs; historical tier labels are included.

These are descriptive, potentially small or biased samples—not a model ranking,
a controlled benchmark, or proof that adaptive routing improves results. No
cost savings are estimated because token/cost telemetry is not recorded.

## Harness adapters

### Preview-first setup

`jevia init` leaves harness selection to you. Jevia includes shell-free command
templates for Codex, Claude Code, OpenCode, and Gemini CLI. List them, then preview
one with explicit model IDs for every configured tier:

```sh
jevia harness presets
jevia harness setup codex --preset codex \
  --model fast=your-fast-model \
  --model balanced=your-balanced-model \
  --model strong=your-strong-model
```

The built-in presets currently render these non-interactive argument arrays:

| Preset | Command and arguments |
| --- | --- |
| `codex` | `codex exec --model {model} {task}` |
| `claude` | `claude --print --model {model} {task}` |
| `opencode` | `opencode run --model {model} {task}` |
| `gemini` | `gemini --model {model} --prompt {task}` |

Presets do not choose model IDs, install the harness, alter its permission policy,
or bypass its prompts. Review the preview, ensure the selected harness version
supports the shown flags, then repeat with `--apply`. Use `harness check` afterward
to validate the local executable candidate without launching it.

For another harness or a different invocation shape, provide an explicit argument
template instead:

```sh
jevia harness setup agent --command my-agent \
  --arg=run --arg=--model --arg='{model}' --arg='{task}' \
  --model fast=provider/small \
  --model balanced=provider/standard \
  --model strong=provider/frontier
```

Substitute your agent's actual executable, argument syntax, and accessible model
IDs. Repeat the same command with `--apply` after reviewing its TOML preview.
Setup never launches either
program, calls Jev, opens storage, or checks provider credentials/model access.
It works offline, including when a configured database is unavailable. There
is no interactive prompt or automatic agent installation. Quote placeholders as
shown and use `--arg=--flag` / `--verify-arg=--flag` for leading-hyphen arguments.

- Preview changes no files. Apply shares a configuration lock with database setup,
  saves a private exact-byte backup in ignored `.jevia/config-backups/`, then
  atomically replaces the config after checking for concurrent edits. It preserves
  unrelated settings, harnesses, and comments, and rejects symlink configs. Stop
  concurrent manual config editing; the lock only coordinates Jevia setup commands.
- Changing an existing harness also requires `--replace`. Reapplying identical
  settings does not rewrite the config or make another backup (apply may create
  the config lock sidecar). The selected harness entry is rewritten when changed.
- Automatic test detection and persistent opt-out behavior are available in
  CLI 0.1.4. Omitted verifier options preserve an existing verifier and automatic-detection
  setting. Since CLI 0.1.6, new adapters default to recording without extra tests. Use `--no-verification`
  to disable both explicit and automatic checks, or `--auto-verification` to remove
  a custom verifier and re-enable detection. Supplying `--verify-command` replaces
  its whole command and argument list (for example, `--verify-command cargo
  --verify-arg=test`). Without verification, process success alone is not eligible
  learning evidence. Missing/duplicate/unknown tier mappings and unsupported
  template placeholders are rejected before config replacement.
- Commands, arguments, and model IDs appear in previews and committed config.
  Never put credentials in these flags or templates; let the agent inherit its
  credentials from the environment. Protect retained config backups, especially
  with appropriate directory ACLs on Windows. Nothing is shell-expanded by Jevia.

### Local preflight

```sh
jevia harness check agent
jevia harness check agent --json
```

Preflight validates the project config and selected adapter, renders its templates
for every configured tier, rejects NUL arguments, and checks file candidates for
the agent and configured or automatically detected verifier. It requires no
Jev/provider key or database
connection, does not read history/cache, and creates no files or locks. It never
executes even a `--version` probe. Missing executables, model mappings, or valid
templates produce a failing exit status. Verification disabled by choice/default
is a passing check; an explicitly requested check that cannot be detected is a
warning. Process success alone is not proof of task correctness.

The human report escapes the requested harness name. JSON reports use
`schema_version: 1`, `harness`, `scope: "static"`, `ok`, `checks` (stable `id`,
`status`, `code`, and explanatory `message`), and `limitations`. Once a project
is found, failed checks also produce JSON and exit nonzero. CLI argument errors
and missing projects retain normal CLI diagnostics. Reports omit configured
command paths, arguments, model IDs, credentials, and raw parse/driver errors.

Lookup checks explicit paths or the inherited `PATH`; it does not expand shell
aliases, variables, `~`, or `PATHEXT`. Unix relative paths/PATH entries are checked
against the project root, with regular-file and execute-bit checks. On Windows,
bare names may omit `.exe`; non-`.exe` extensions must be explicit. Preflight
requires absolute explicit paths and absolute PATH entries on Windows, and warns
that it does not search extra system/application directories. This intentionally
conservative check is not a complete reproduction of OS executable resolution;
use an absolute path if lookup is ambiguous. Windows batch wrappers get a warning
because [Rust launches them through `cmd.exe`](https://doc.rust-lang.org/std/process/index.html#windows-argument-splitting).

An `ok` result means **static checks passed**, not that an agent is authenticated
or will successfully launch. Binary format, interpreters, mount/ACL restrictions,
agent-specific flags, provider model access, and task correctness are not tested;
files and environment may change afterward. Preflight does not change `jevia run`
or the existing `jevia check` live routing probe.

### Manual configuration

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

# Optional: uncomment only if you want extra verification after execution.
# [harnesses.agent.verification]
# command = "cargo"
# args = ["test", "--workspace", "--all-features"]
~~~

A complete ready-to-copy configuration is available at
[examples/jevia.toml](https://github.com/assistant-ui/jevia/blob/main/crates/jevia-cli/examples/jevia.toml).

Then route and run a task through that adapter:

~~~bash
jevia run agent "investigate the failing integration test"
~~~

Extra harness arguments must follow <code>--</code> and are appended without
shell interpretation:

~~~bash
jevia run agent "update the parser" -- --verbose
~~~

For a task beginning with a hyphen (for example, a pasted Markdown checklist),
use the explicit equals form. The usual positional form remains supported:

```bash
jevia run claude --task="- Fix the parser" --non-interactive -- --verbose
```

Supply either a positional task or `--task=...`, not both. Jevia's `--` still
introduces extra harness arguments. For exact built-in Claude, Codex, and OpenCode
presets, all tasks are placed after the harness's option delimiter, so even words
such as `attach`, `serve`, or `web` cannot be mistaken for capture options.
Gemini uses `--prompt=...` for leading-dash tasks,
with extra options before the task. Custom wrappers/templates are not rewritten;
configure their own literal-argument convention. A standalone `-` can still mean
stdin to a native harness; use descriptive task text instead.

Templates support <code>{task}</code>, <code>{model}</code>,
<code>{tier}</code>, and <code>{run_id}</code>. Jevia requires the task and
model placeholders, rejects unknown placeholders, launches the configured
executable directly, and mirrors its exit code. A non-zero harness exit skips
verification. Without a verifier or explicit feedback, both zero and non-zero
exits leave task outcome `unknown`, while retaining useful execution observations.

Jevia runs the configured or detected verifier only after the harness succeeds.
A normal verifier exit of zero records success; a normal non-zero exit records
failure. A verifier that cannot start, times out, or terminates by signal leaves
the outcome unknown, preventing an inconclusive check from incorrectly training
the router. Existing records are not rewritten. Verification arguments support the same
placeholders and are also launched directly without shell interpretation.

Completed harness runs record the requested model, harness name, duration,
process exit code, optional verification, and available native observations.
Inspect <code>jevia runs --json</code>. A requested model or a passing verifier
does not establish which internal model attempt produced the fix.

## Node.js API

Use the typed `jevia` npm package when a Node application or agent framework
owns harness execution. It calls the CLI's shell-free JSON interface, keeping
routing policy, storage, caching, and outcome handling in one implementation:

~~~bash
npm install jevia
~~~

~~~ts
import { JeviaClient } from "jevia";

const jevia = new JeviaClient({ cwd: process.cwd() });
const task = "investigate the failing integration test";
const route = await jevia.route(task);

const model = {
  fast: "provider/small",
  balanced: "provider/standard",
  strong: "provider/frontier",
}[route.tier];

const result = await runYourHarness({ task, model, runId: route.run_id });
// Optional: report a known result from your own adapter, without a verifier.
if (result.outcome === "success" || result.outcome === "failure") {
  await jevia.feedback(route.run_id, result.outcome);
}

// Automatically uses eligible outcomes already in the project's storage.
const next = await jevia.route("investigate another integration failure");
~~~

The adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor Agent,
Copilot CLI, Aider, Goose, Amp, or a custom harness. Jevia returns a capability
tier; the application maps that tier to a harness-specific model. Feedback and
verification are optional: `route()` works without either. If feedback is omitted,
the decision stays recorded with outcome `unknown`, and later routing still uses
other eligible outcomes. The optional `result.outcome` field above comes from your
own adapter; Jevia does not infer task success from a process or function return.
The CLI must already be installed and available on `PATH`; npm installation does
not run a binary downloader.

The SDK does not require Jevia's built-in verifier: your application decides how
to establish the outcome and records it. `manual` evidence means explicitly
reported by the application or user, not necessarily human-entered. Each
`route()` automatically loads recent eligible successes/failures from the
selected JSONL, SQLite, or PostgreSQL history before deciding. No history argument
or manual cache clearing is needed; the evidence is part of the cache key.
Finished process-only runs inform a separate observation window, even while their
task outcome is unknown. Active and routed-only runs are not execution observations.
The `[router].history_limit` setting bounds each window (default 20, maximum 100;
0 disables it). This supplies evidence to Jev, not model training. See the
[SDK outcome loop](../packages/jevia-node/README.md#recorded-outcomes-inform-the-next-route-automatically)
for provenance and privacy details.

The client exposes `version`, `route`, `feedback`, `runs`, and `show`. Record
responses are validated at runtime, including supported schema versions,
probabilities, timestamps, lifecycle state, execution evidence, and feedback.
Malformed or unsupported responses raise `JeviaProtocolError` without including
their contents.

`setupStorage(target, options?)` and `checkStorage(options?)` in `jevia@0.1.1`
provide the same explicit, preview-first SQLite and PostgreSQL administration as
the CLI. They require CLI 0.1.2 or newer. Apply
requires `confirmStopped: true`, JSONL import remains explicit, PostgreSQL accepts
an environment variable name rather than a URL, and deep checks do not write or
repair. See the [Node.js package reference](../packages/jevia-node/README.md) for
the complete typed examples and failure guarantees.

`JeviaCommandError` exposes a generic message, exit code, and signal for ordinary
logging. Raw command, stdout, and stderr are available only through explicit
getters for private debugging and may contain tasks or credentials; do not send
them to logs or telemetry.

## Automatic CLI pipeline

After configuring your harness once, use the normal command:

```sh
jevia run codex "fix the failing test"
```

Jevia routes the task, launches the agent, and records lifecycle, process facts,
and supported native events automatically. Later routing uses this history without
requiring a known task outcome. **No manual `feedback` or `runs complete` step is
required.** Optional feedback may report a known outcome or correct an earlier one;
external completion is for work executed outside the supervised CLI flow.

Additional verification is opt-in. When explicitly enabled, it runs once after the
agent process/session finishes successfully, not after each internal tool call.
Recording itself is language-independent: Python, Go, and mixed-language projects
need no test configuration for passive history. Optional checks can use explicit
commands such as `python -m pytest`, `go test ./...`, or your project's test script.
Task execution may still require the agent's normal permission prompts. Installing
the agent, supplying credentials/model mappings, and preparing project dependencies
are one-time prerequisites, not per-run feedback tasks. Jevia does not retry or
repair failed work automatically.

If you opt in with `auto_verify = true` and have no explicit verifier, Jevia
detects existing tests at the project root. The default in CLI 0.1.6 is `false`:

- Rust: `cargo test --workspace` for a root Cargo package/workspace.
- Node: the existing `test` script, using `packageManager`, then an unambiguous
  lockfile, then npm. npm, pnpm, Yarn, and Bun are supported. Empty scripts and
  common placeholder/no-op scripts are not selected.

Automatic checks run with `CI=true`, no stdin, owned process-tree cleanup, and a
five-minute deadline even if the agent was interactive. To override the deadline,
use `--non-interactive --verification-timeout-seconds <seconds>`. Jevia does not
install a test runner or package manager. The existing Cargo/package-manager test
command may build/download dependencies as it normally does; use trusted projects.

An explicit `[harnesses.<name>.verification]` always wins. For mixed Rust/Node
roots, unsupported projects, invalid manifests, or conflicting lockfiles, Jevia
reports that automatic verification is unavailable instead of guessing. The run
is still recorded, but process-only results are not verified learning evidence.
Configure a suitable check only if you want extra verification. `harness check <name>` previews
detection and executable availability without running tests. Passing tests means
the selected checks passed—not a guarantee that every requirement was satisfied.

Test discovery is available since CLI 0.1.4. Since CLI 0.1.6, omitted
`auto_verify` means no extra tests; existing explicit `true` values and custom
verifiers remain honored. Earlier setup commands wrote `auto_verify = true`, so
use `harness setup ... --no-verification --replace --apply` or set it to `false`
and remove any custom verifier to stop those previously configured checks.
The Node SDK's `route`, `feedback`, and `complete` remain explicit and flexible;
they never launch an agent or run project tests automatically.

## Run lifecycle

Execution progress is separate from task outcome. New records track `routed`,
`running`, `verifying`, `completed`, `launch_failed`, or `interrupted`, with start
and finish timestamps. A completed process may still have a failed task outcome.

```sh
jevia runs show <run-id>
jevia runs recover <run-id>
```

`show` prints the complete record as JSON. `recover` explicitly marks a formerly
running/verifying execution as interrupted only if no Jevia supervisor holds its
per-run lease. It leaves the task outcome unknown and never reruns or terminates
processes. After a supervisor crash, inspect any surviving child processes and
workspace changes before starting new work. Legacy unknown outcomes are not
assumed to represent interrupted executions. Feedback on active runs is refused.

The CLI 0.1.7 JSONL lookup retains only the requested record while validating
the full history under its shared lock. `runs show` and journal replay no longer
load all records into memory. Reads remain linear in file size and still report
corrupt/unsupported records anywhere in the file; busy background replay defers
without waiting on a writer. This does not change stored history or SQL lookups.

New and updated records use schema version 6; versions 1–5 remain readable.
Older CLI versions refuse newer schemas rather than silently discard new metadata.
The ignored `run-leases/` sidecars are retained so concurrent processes always
coordinate on the same lock file.

### Complete work from an external harness

When you use `route` and run your own agent, ordinary `feedback` updates the
outcome but deliberately leaves the run pending. After **all external work and
verification have stopped**, explicitly finish it:

```sh
jevia runs complete <run-id> success --confirm-stopped
# A changed known outcome requires an explanation:
jevia runs complete <run-id> failure --confirm-stopped --reason "Tests still fail"
```

Available in CLI 0.1.4, this command atomically records manual feedback and a
completed lifecycle in JSONL, SQLite, or PostgreSQL. The
finish timestamp is when completion was recorded; start time and harness evidence
remain absent because Jevia did not observe execution. `unknown` is also accepted
when work stopped but the result is inconclusive; it does not become learning evidence.

Only pending external records qualify. Active/terminal runs, supervised evidence,
held execution leases, and SQL supervisor ownership are refused. This command
does not stop processes, verify work, retry tasks, or bypass recovery. In the next
CLI release, identical completion retries (same outcome and trimmed reason) return
the saved external completion with its original timestamp and no duplicate
feedback. Conflicting retries and subsequent corrections are refused. A request
that races an active lease may need a later retry. Older CLIs require inspecting
`runs show` after an uncertain response. Completed records become eligible for normal preview-first archival;
other pending and active records remain protected.

### Bounded non-interactive execution

For a headless harness, opt into owned process-tree supervision:

```sh
jevia run agent "fix the parser" --non-interactive --timeout-seconds 300 --verification-timeout-seconds 120
```

Each deadline applies only to its execution phase, not the Jev routing request.
Both flags require `--non-interactive` and accept 1–86400 seconds. Unspecified
deadlines are unlimited. This mode disables stdin and uses a Unix process group
or Windows job object; stdout/stderr still stream normally. Ctrl-C (and SIGTERM
on Unix) stops the owned group/job and records `cancelled`; a deadline records
`timed_out`. Outcomes remain unknown, failed/cancelled harnesses skip verification,
and verification cancellation preserves the harness evidence. Exit codes are 130
for cancellation and 124 for timeout. Cleanup errors record `interrupted` instead
of claiming the process tree was stopped.

The main command exiting does not finish a supervised phase while background
processes remain in its group/job. Jevia waits for them before starting verification
or recording completion, and the phase deadline and cancellation remain active.
The main command's exit code is preserved; background commands must report their
own failures to the main command (or the verifier) if they should affect the outcome.

CLI 0.1.9: live recording checkpoints carry a one-second work budget into JSONL
workers, including queued workers whose async waiter was cancelled. Slow scans or
rewrites stop between chunks/records without publishing partial history; the
journal remains the retry source. This prevents an abandoned background rewrite
from monopolizing the history lock while the supervisor saves terminal state.
Blocking operating-system I/O and decoding a single record are not preemptible;
this is a cooperative work bound, not a hard filesystem-latency guarantee.

Without this flag, existing interactive terminal behavior remains unchanged.
This is not a sandbox: descendants that deliberately escape a process group/job,
SIGKILL of Jevia, and machine crashes cannot be handled reliably. Use explicit
recovery and inspect the workspace in those cases; no work is automatically retried.

## Outcome provenance

`jevia run` saves this evidence automatically. The manual feedback examples below
are for externally verified work, corrections, or legacy records—not a required
step after each CLI run.

Run records distinguish `process_exit`, `verification`, and `manual` evidence.
Legacy process-only success/failure remains visible, but only known outcomes from a
completed verifier or explicit application or human feedback are supplied to Jev as learning
evidence. Legacy outcomes without provenance are not silently promoted; confirm
them with `feedback` if you want them used in routing. `runs` reports the source
and whether the result is eligible as quality evidence. Finished executions can
still contribute passive observations independently of that quality label.

```sh
jevia feedback <run-id> success
jevia feedback <run-id> failure --reason "The integration test still fails"
jevia runs show <run-id>
```

Changing an already-known outcome requires a nonempty `--reason`. Every feedback
operation retains the prior outcome/source, timestamp, and optional reason in a
audit trail in the selected backend; original execution evidence is preserved. Reasons are limited
to 4096 bytes and are never sent to Jev. Setting the outcome to `unknown` removes
it from the known-outcome window; any finished execution facts remain eligible as
passive observations. These records are not a tamper-proof audit log.

## Routing explanations

Use `jevia route "task" --explain` or `jevia run agent "task" --explain` to
inspect routing facts. This requires CLI 0.1.7 or newer. Explanation lines go
to stderr, before harness launch; `--json` stdout and saved run records keep
their existing format. No explanation is printed unless requested.

```text
jevia: explain cache=miss:not_found coordination=acquired write=stored
jevia: explain known_outcomes=2 passive_observations=8 history_limit_per_kind=20 history_stage=candidates_before_byte_budget
jevia: explain source=live confidence=0.94 floor=0.65 fallback=not_applied elapsed_ms=120
```

`known_outcomes` counts the eligible outcome candidates within the count window,
before the byte budget. `passive_observations` counts the separate window of
finished executions without eligible outcome evidence, including process-only, unknown, and partial
capture. A run is not counted in both windows. Active/routed-only executions are
not passive history. Feedback and additional verification remain optional, and
recorded history is included automatically. On cache hits these are the inputs
considered when building the cache fingerprint, not a new provider request.
These are candidate counts, not a claim that every record was transmitted:
byte limits can omit records or shorten historical task text. The projection's
`history_budget` in the provider request reports included/omitted counts.

Cache status distinguishes `hit`, `miss:not_found`, `miss:expired`, `disabled`,
`bypassed`, `unavailable`, and `key_unavailable`. Coordination reports
`not_needed`, `acquired`, `waited`, `timed_out`, or `unavailable`; cache writes
report `stored`, `skipped`, or `failed`. A fingerprint miss cannot identify which
input changed. Confidence/floor and `fallback=below_confidence_floor` describe
the policy applied to the returned signal, not the model's internal reasoning.
Elapsed time includes startup replay and routing, but not harness execution.

Explanation lines contain only fixed labels and numeric facts: no task text,
model/tier names, credentials, record IDs, session IDs, or cache fingerprints.
This does not suppress ordinary CLI/harness output or change task-text storage
and provider privacy settings. A failed route does not print a successful
decision explanation or create a synthetic run.

## Routing cache

CLI 0.1.9: `jevia run <harness>` supplies Jev with `current_harness.name` and
`current_harness.tier_models`, containing only that harness's configured models for
the declared tiers. This lets routing distinguish the current candidate models from
historical models with the same tier labels. Commands, arguments, verifier settings,
and unrelated harness definitions are not sent. Candidate names are not execution
evidence, pricing data, or proof of capability; confidence fallback is unchanged.
Both the provider request and cache fingerprint use this context. Changing a model
mapping invalidates the matching decision; existing entries safely miss once after
the cache fingerprint upgrade. No history is removed.

Plain `jevia route` (including SDK `route()`) remains harness-agnostic and does not
infer a harness from history. SDK `run()` receives the same context as CLI `run`.
Rust callers can opt in with `JevClient::route_for_harness`; the existing `route`
method keeps its signature and harness-agnostic behavior.

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
recent outcomes and passive observations actually sent to Jev. A new observation,
success, failure, verification result, policy change, model change, or harness change
produces a miss when it changes these inputs. The fingerprint uses the same
bounded history projection and budget metadata as live routing. Changes confined
to omitted content or a truncated task's discarded suffix do not invalidate an
otherwise equivalent decision. Pending outcomes also do not invalidate it.
The byte-budget update advances the cache fingerprint version; existing entries
safely miss once without deleting history.

Cache files contain the fingerprint and decision signal, not task text. Every
hit receives a fresh run ID and timestamp, and run records expose
<code>source=live</code> or <code>source=cache</code>. Use
<code>--no-cache</code> on <code>jevia route</code> or
<code>jevia run</code> when a forced live decision is needed.

Cache errors never block routing: Jevia reports the problem and falls through
to a live request. API errors are never cached, expired decisions are never
used as an offline fallback, and <code>jevia cache clear</code> provides an
explicit recovery path for a damaged cache.

Concurrent equivalent cache misses normally share one live routing request.
Waiters recheck both the cache and current learning evidence before using a
decision; they still receive independent run IDs. Coordination uses up to 256
stable lock stripes in the ignored `cache-leases/` directory, so lock files do
not grow per task. Unrelated requests can occasionally share a stripe and wait.
No global history/cache lock is held during network calls. An OS lease is
released if its owner exits; failures are not cached, so another caller can try.

Waiting is bounded to the configured Jev timeout plus one second (at most 30
seconds). On expiry or a coordination error, routing proceeds live; duplicate
requests are possible in that fallback. `--no-cache` skips coordination too.

## Storage

JSONL remains the default. SQLite and PostgreSQL are opt-in alternatives; changing
the backend does **not** synchronize or automatically move existing history.
The normal `route`, `run`, `runs`, `feedback`, `doctor`, and `check` commands use
the selected backend. These options require v0.1.2 or newer.

### SQLite: local database, no server

Streaming JSONL history, health checks, imports, and cache reads accept at most
8 MiB per physical line (including its newline; an unterminated final line must
leave one byte for that newline). Oversized lines fail with a
redacted error and are never truncated or skipped. JSONL appends and rewrites
also reject oversized output before publication. Existing oversized history
must be inspected and reduced explicitly; this does not bound the total memory
of arbitrary SQL rows. CLI 0.1.9 JSONL repair/archive streams payloads but still
keeps a set of unique run IDs for duplicate detection.

The guided CLI path avoids editing TOML by hand (run `jevia init` first):

```sh
jevia storage setup sqlite --path .data/jevia/history.db --import-jsonl
# Stop all Jevia writers/supervisors using this workspace, then:
jevia storage setup sqlite --path .data/jevia/history.db --import-jsonl --apply --confirm-stopped
jevia storage check
jevia stats
```

The first command previews without changing project files or contacting a database.
Guided SQLite and PostgreSQL setup stream history into a private temporary
snapshot that is removed on close. Memory scales with the largest record and
the set of run IDs needed to detect duplicates, rather than the entire history.
Apply imports only that validated snapshot and rechecks a streaming SHA-256
fingerprint of the original bytes before import and before switching config.
This detects source changes; it does not replace the requirement to stop writers.
Without `--path`, SQLite uses `.jevia/jevia.db`. `--path` accepts another
project-relative or absolute local file; relative paths are resolved from the
discovered project root even when invoked from a subdirectory. Protect and ignore
custom paths outside `.jevia` yourself. An empty JSONL project can omit
`--import-jsonl`; a nonempty one must include it to avoid silently abandoning
existing evidence. The source JSONL file is never deleted or rewritten.

Or configure manually:

Add to `.jevia/config.toml`:

```toml
[storage]
backend = "sqlite"
url = "sqlite://.data/jevia/history.db"
```

Then explicitly initialize and optionally import your old JSONL history:

```sh
jevia storage init
jevia storage check
jevia storage import-jsonl
jevia storage import-jsonl --apply
jevia runs
```

Relative file paths are resolved from the project root, not the current working
directory. SQLite is bundled into the binary. It uses WAL, full synchronous writes,
a five-second busy timeout, and private file permissions on Unix for new databases.
Use a local disk, not a shared/network filesystem. Default `.db` files, WAL/SHM
sidecars, and run locks under `.jevia` are ignored after `init`/`storage init`.
For custom paths/extensions, protect the directory and add your own ignore rules;
on Windows, protect the directory with the appropriate filesystem ACLs.

### PostgreSQL: bring your own database

Provision a dedicated PostgreSQL database and put its connection URL in your
secret manager or environment as `JEVIA_DATABASE_URL`. Do not put passwords in
the project config or CLI arguments.

```sh
jevia storage setup postgres --project my-project --import-jsonl
# Stop all Jevia writers/supervisors using this workspace, then:
jevia storage setup postgres --project my-project --import-jsonl --apply --confirm-stopped
jevia storage check
```

`--url-env MY_DATABASE_URL` selects a different environment variable **name**, not
a URL value. Preview does not resolve that variable or test connectivity. On apply,
the existing TLS and timeout rules apply; `--allow-insecure-localhost` is available
only for loopback development databases. The command creates the Jevia schema and
project, not a PostgreSQL server, database, or user.

The equivalent manual configuration is:

```toml
[storage]
backend = "postgres"
url_env = "JEVIA_DATABASE_URL"
project = "my-project"
```

Run `jevia storage init` once with schema-creation permissions, then
`jevia storage check`. Normal operation needs read/write access to the
`jevia_projects` and `jevia_runs` tables and read access to `jevia_schema`,
not permission to create databases. Remote connections require certificate and
hostname verification (`sslmode=verify-full`); `sslrootcert`, `sslcert`, `sslkey`,
and `application_name` URL options are supported. Only local development can opt
into plaintext using `allow_insecure_localhost = true` and a loopback host.

Use a **direct or session-pooled connection**, not a transaction-mode pooler:
execution guards use PostgreSQL session advisory locks. Each CLI invocation uses
a small pool; running a harness also holds a dedicated guard connection.

Use the same `project` value across trusted workspaces to share evidence. This
namespace is **not authorization or tenant isolation**: anyone with access to the
database tables can access other projects. Separate database roles/databases or a
future authenticated managed API are needed for mutually untrusted users.

### Passive history lookup

SQLite and PostgreSQL 16+ use a database-maintained partial index for finished
executions without a known assessed outcome. The latest observation window is
selected in SQL, then returned in append order. Pending/active runs and known
outcomes do not crowd out these observations or require a full scan before a
routing cache lookup. Feedback, lifecycle updates, imports, and archival update
the index transactionally, including writes from older clients that can read the
stored record schema. Automatic history use and optional verification/SDK feedback
are unchanged; the index is not a new success signal.

New database setup creates the index. For an existing database, back up and run
`jevia storage init` during a quiet maintenance window with schema permissions.
This explicitly builds the index over existing rows without rewriting records,
ordering, ownership, or sequence counters. SQL schema **1** and record schema
**6** are unchanged. Normal routing/diagnostics do not create indexes, and stores
without this index remain usable but may scan more rows. Building the index can
block writers and is subject to the existing five-second database timeout; a
failed initialization rolls back rather than switching storage or dropping data.
Resolve contention before retrying; a build that exceeds the timeout even while
idle needs a separately planned maintenance operation, not repeated routing calls.
PostgreSQL before 16 retains the compatible paginated lookup without this index.

In CLI 0.1.7, `storage check`, `storage check --deep`, `doctor`, and
`check` also report the passive index as present, missing, unavailable,
unsupported (PostgreSQL before 16), or not applicable (JSONL). Missing-index
guidance points to the explicit initialization above; diagnostics never build or
replace an index. Catalog checks also recognize the expected filtering predicate,
not just the index name/columns. Unexpected or unrecognized predicates report
unavailable; diagnostics never execute catalog SQL or silently rebuild indexes.
Recognition is conservative: a manually rewritten equivalent expression or a
new PostgreSQL catalog rendering may require inspection rather than report present.
This is not a proof of query-plan selection or a general SQL equivalence check. The
original storage health result remains separate: a missing performance index is
not data corruption. SDK `checkStorage()` returns the additional human-readable
line without changing its API.

### Structured storage checks

Available in CLI 0.1.7 and Node SDK 0.1.3.

```sh
jevia storage check --json
jevia storage check --deep --json
```

JSON mode emits one report on stdout, exits 0 when `ok` is true and 1 when a
check fails. Argument errors still use the normal CLI usage error (exit 2).
Report schema 1 contains `backend` (`jsonl`, `sqlite`, `postgres`, or `null` before
configuration is available), `check` (`basic` or `deep`), `ok`, `records`,
`passive_history_index`, and `error`. Unavailable counts/index status are `null`,
not zero or missing. Index statuses are `present`, `missing`, `unavailable`,
`unsupported`, and `not_applicable`; a missing/unexpected index does not itself
make a successful storage health check fail.

Failures use static messages and one of `configuration_unavailable`,
`storage_unavailable`, `history_check_failed`, or `index_check_failed`. Reports
contain no paths, project names, URLs, credentials, run IDs, or record contents.
They summarize sequential checks, not one atomic database-wide snapshot. The
existing basic/deep guarantees below apply; neither mode initializes storage or
repairs records/indexes, and no Jev credential or request is needed.

Node SDK 0.1.3 adds `checkStorageReport({ deep?: boolean, signal? })`.
It validates schema and report/exit-code consistency and returns typed healthy
**or failed** reports; check `report.ok`. Process, timeout, cancellation, and
protocol errors still throw. This method requires CLI 0.1.7 or newer;
older CLIs fail explicitly rather than falling back to parsing text.
`checkStorage()` remains the unchanged human-readable API.

Routing validates the selected SQL history windows, not every unrelated record.
Malformed JSON remains an index candidate and fails closed if selected. Use
`jevia storage check --deep` for full-store integrity validation; the optimization
does not replace that diagnostic or repair corrupt history.

### Setup safety and recovery

`storage setup` is preview-first. Applying requires both `--apply` and
`--confirm-stopped`: stop all source writers and supervisors, including scheduled
jobs and other workspaces using the source file. Recover any active run records
before retrying; this command does not stop processes or recover runs for you.

Apply backs up the exact old config to a new `.jevia/config-backups/config-*.toml`
file, initializes/checks the destination, imports requested history transactionally,
then atomically replaces config **last**. Routing, privacy, cache, harness settings,
and unrelated TOML comments are preserved. Config permissions are retained;
new backups are private on Unix and added to local ignore rules. Backups are not
automatically removed. On Windows, protect the directory with appropriate ACLs.

Active, duplicate, malformed, unsupported, or conflicting imported records cause
failure. Identical destination records are skipped for safe retries. Setup does
not replace config on connection, schema, permission, or import failure. It
serializes other setup invocations and checks for changes to the source/config
before switching, but cannot prevent an editor or already-running supervisor
from writing: the stop-writers requirement is not optional.

The database and filesystem are **not one atomic transaction**. A failed setup
can leave an initialized database, a config backup, or (if the final config save
fails) imported records. Inspect config, keep the original JSONL and backup, and
retry only after reconciling concurrent changes. A crash after replacement may
mean config was already switched; verify with `jevia storage check`. Never blindly
restore old config once new work has written to the database, since histories can
diverge. No automatic synchronization, rollback deletion, or backend fallback is
performed.

Once configured, repeat the same setup command **without `--import-jsonl`** to
initialize/check the same target without rewriting config or adding another backup.
Old JSONL files may be stale, so importing them from an already-SQL project needs
the separate explicit `storage import-jsonl` command. Changing between SQL targets
or back to JSONL is not supported by guided setup; use explicit export/import and
review the configuration change yourself.
Changing the value of the PostgreSQL URL environment variable can independently
change the destination; setup cannot detect which database it previously named.

### Guarantees and current limits

- Database writes are transactional. Feedback history and its current outcome
  change together; project-scoped write locks prevent lost updates. Retention
  holds the same lock while saving recovery files; schedule it during a quiet period.
- Recent history and eligible evidence use indexed, bounded queries in append
  order. Complete versioned records preserve execution and feedback provenance.
- `storage check` verifies schema and CRUD permissions with a rolled-back probe;
  it does not insert fake evidence. `doctor`/`check` include this access check.
  SQL records are not individually decoded by the default access check.
- `storage check --deep` instead inspects the complete selected history without
  a write probe, automatic repair, Jev request, or cache access. JSONL validates
  record schemas and nonempty/unique run IDs under the shared history lock,
  streaming records while retaining an ID set (memory grows with the IDs).
  SQL uses 200-row pages in one consistent read snapshot to validate record
  schemas, indexed run IDs, learning flags against current evidence rules, and
  positive append ordinals bounded by the project's append counter. Gaps left
  by retention are valid; the counter need not equal the newest retained ordinal.
  Only the configured PostgreSQL project is inspected. Ordinary SQL writers can
  continue, though a long scan may delay database cleanup/WAL recycling.
- A deep-check failure exits nonzero and identifies the line or append-position
  where possible, without printing task text, IDs, or database credentials. Keep
  the current history and backups and investigate locally before making changes;
  no records or indexes are silently rewritten. Success means the inspected
  snapshot passed these logical checks, not that write permissions, physical
  database integrity, or task-outcome correctness were verified. Run the default
  access check separately when needed; database-native integrity checks and
  backups remain the operator's responsibility. Missing storage is not initialized.
- Database/record schema versions are checked; unknown versions are rejected.
  Driver errors are redacted and database operations have five-second timeouts.
  An outage never silently switches history back to local JSONL.
- Imports preview by default and commit all-or-nothing with `--apply`. Identical
  run IDs are skipped; conflicting records, duplicate source IDs, active runs,
  or malformed/unsupported records abort the import. Stop source writers first.
  Keep the unchanged source as your backup; no automatic bidirectional sync or
  background replication is provided.
- `storage import-jsonl` validates and streams the source into a private unnamed
  temporary file before taking a database write lock. It holds the source's shared
  JSONL lock during capture, then imports only the captured records in one database
  transaction. Later changes to the source are not included; preview and apply
  capture independently. Both modes need temporary disk space for the normalized
  history in the OS temporary directory (use protected directory ACLs on Windows).
  Memory grows with the run-ID set plus the largest record, not all task bodies;
  the ID set is released before database writes. Snapshot failures prevent writes;
  later read errors or conflicts roll back all inserts and their ordering counter.
  The temporary file is removed on close/process exit, not kept as a backup.
  Imports still normalize known fields and omit unknown additive fields. Large
  imports hold the project write lock until commit/rollback (SQLite serializes
  all database writers); this is not a resumable or chunk-committed import. Guided
  `storage setup --import-jsonl` still captures its migration source in memory.
- `jevia storage export --output .jevia/snapshot.jsonl` writes a new private
  snapshot in append order. It streams one JSONL record or a bounded SQL page at
  a time, rather than loading the entire history. JSONL holds its shared history
  lock; SQL uses one consistent read snapshot across all pages (PostgreSQL
  repeatable-read/read-only, SQLite WAL snapshot) without taking the project write
  lock. SQL changes committed after the snapshot begins appear in a later export,
  not partway through this one. Very large records still require memory, and a
  long SQL snapshot can delay database cleanup/WAL recycling.
- Export writes to a private temporary file beside the destination, validates all
  records, flushes/syncs the file, and publishes without overwriting any existing
  path, including symlinks. Handled read/write failures do not publish a partial
  destination. The parent directory is synced on Unix; if that final sync fails,
  the error says the complete file was already created. A process crash can leave
  a private `.jevia-export-*.tmp` file. Protect and ignore exports and their output
  directory; they may contain task text. On Windows, use appropriate directory
  ACLs. Exports normalize known record fields, omit unknown additive fields, and
  are logical record snapshots, not exact-byte or physical database backups.
- The decision cache and cache-miss coordination remain local. Every routing
  attempt fetches current eligible evidence before computing its cache key, so
  new shared outcomes invalidate affected decisions. Cross-machine request
  deduplication and offline write queues are not included.
- SQLite run locks sit beside the canonical database path. PostgreSQL guards
  block recovery while the supervisor's database session is alive. Because a
  lost session does not prove its subprocess stopped, PostgreSQL recovery also
  requires `jevia runs recover <id> --confirm-stopped` after inspecting and
  stopping the original supervisor and any surviving processes. Recovery is
  terminal and fences late writes from that supervisor; it never reruns work.
  There is no automatic lease expiration or remote process termination.
- `runs archive` supports all three backends with preview-first, backup-first
  retention (see below). `runs repair` remains JSONL-only: database integrity
  repair and physical backups require database-native tooling.
- Managed hosting, managed credentials, and a dashboard are not part of this
  integration. Data is stored locally or in the user's own database.

### Default JSONL data

Project configuration lives in `.jevia/config.toml` and is intended to be
reviewed and committed. Run history lives in `.jevia/runs.jsonl` and is ignored
by the project-local `.jevia/.gitignore` because prompts and outcomes may be
sensitive. Jevia coordinates concurrent readers and writers through the
ignored `.jevia/runs.lock` sidecar so parallel agents cannot overwrite one
another's evidence.

Routing decisions live in the ignored `.jevia/cache.jsonl` file and use the
same locking and atomic-replacement guarantees through `.jevia/cache.lock`.

By default Jevia stores task text in the selected backend so it can supply useful examples to
future decisions. Set `store_task_text = false` under `[privacy]` to retain only
routing metadata.

In CLI 0.1.7, JSONL updates and basic health checks stream the complete
history instead of retaining every record in memory. Updates still take an
exclusive lock and atomically replace the file only after all records validate;
a missing run, rejected change, malformed suffix, or write failure leaves the
original file intact. This reduces memory use, not the linear scan/rewrite cost.
Deep checks still retain run identities to detect duplicates.

In JSONL mode, `jevia doctor` validates the complete history and reports malformed records
without deleting or rewriting them.

Malformed configuration, history, and cache diagnostics omit raw values, including
parser error chains. They report line/column positions where available and safe
configuration guidance. This does not redact deliberately requested run/task output
or stdout/stderr streamed by your configured harness and verifier.

Provider and transport failures also omit raw response values and endpoint URLs,
including underlying error chains. Diagnostics retain safe failure categories,
JSON positions where available, and HTTP status codes.

Routing HTTP requests never follow redirects, including same-origin redirects.
Configure the canonical Jev API base URL directly. A 3xx response fails with its
status code; its Location header and response body are not logged. This prevents
redirects from forwarding current tasks, historical evidence, or credentials to
another endpoint. Failed requests do not create routing decisions or cache entries.

### History maintenance

Both maintenance commands preview by default. Inspect the report before repeating
with `--apply`; `--json` provides counts and saved paths for automation.

JSONL maintenance and routing-cache reads require regular files. Directories,
named pipes (including links to them), and other special inputs are rejected
without reading or replacing them. Missing files retain their empty-store behavior.

```sh
jevia runs repair
jevia runs repair --apply
jevia runs archive --keep 1000
jevia runs archive --keep 1000 --apply
```

Repair is JSONL-only. It handles an incomplete, unterminated final JSON line after
an interrupted write, or a valid final record missing its newline. It refuses malformed middle
lines, complete invalid records, unsupported schemas, and duplicate IDs. It does
not guess at missing fields or rewrite individual outcomes.

JSONL repair and archival use the same routing-decision validation as normal
reads: identities, tiers, and model must be nonempty, and confidence/probability
values must be finite and within 0–1. The 8 MiB physical-line limit also applies,
including space for a final newline. Invalid complete records are refused even
when a later tail is repairable. Preview and apply leave the original bytes
untouched and create no backup/archive on validation failure.

CLI 0.1.9: JSONL maintenance streams one bounded physical line at a time.
Preview retains only run IDs and counts, not full records or output buffers.
Apply makes a second validated pass under the same exclusive history lock,
streaming exact bytes to private recovery files and a temporary replacement.
Both passes must have matching counts and a SHA-256 digest before recovery files
are finalized and history is atomically replaced. Memory grows with unique run
IDs and the largest record, not the combined history, backup, and archive sizes.

Archival works with JSONL, SQLite, and PostgreSQL. It keeps the most recently
**appended** `--keep` eligible terminal records (minimum one), plus every active,
routed/pending, or legacy-unknown record. SQL also retains any record with an
execution owner, even if its recorded lifecycle appears terminal. This command
does not recover runs, stop processes, or infer that old work has finished.

CLI 0.1.9: all backends also retain any run with a local pending recording journal
or loss marker in the invoking project's `.jevia/`, even if that run is terminal.
These protected rows do not consume the `--keep` allowance. Preview does not replay
or read recording contents; apply inventories recording names under the history/
project lock and rechecks before publishing, refusing changes if new owners appear.
Unreadable inventories fail closed. Corrupt, duplicate, or unsafe recording files
protect their owner too; archival is not a way to discard them. After a normal
route/run successfully replays and removes the journal, its terminal row can be
archived on a later invocation. PostgreSQL cannot inspect journals on other machines;
finish/recover remote recordings before running retention from a different host.

Older eligible records move to a separate JSONL archive. They no longer appear in
`runs` or `stats`, accept feedback, or inform routing. Routing fetches current
evidence before cache lookup, so removing relevant evidence changes the cache key;
archival itself does not clear or rewrite the local decision cache. Choose a
retention window large enough for the evidence you need. This is explicit
maintenance, not automatic pruning.

All backends save recovery files under the invoking project's
`.jevia/history-backups/` and `.jevia/history-archives/` **on the CLI machine**,
including when PostgreSQL is remote. Apply recomputes its plan under the history
lock. Preview (and a no-op apply) removes no records and creates no recovery files.

- **JSONL:** repair and archival back up the exact original file before atomic
  replacement. Archived record bytes, including additive metadata, are preserved.
- **SQL:** archival holds the selected project's write lock, validates all records
  in bounded pages, and saves a full project-record snapshot plus an archive of
  the selected rows before any deletion. A private temporary journal bounds
  memory while deletion batches commit in one transaction. Saved JSON preserves
  additive fields but flattens physical line breaks into JSONL; it is not an
  exact-byte or physical database backup. SQL project/ordinal/owner metadata and
  other projects are not included. There is no database schema change.

SQL rejects malformed/unsupported records and mismatched indexed IDs, rechecks
the snapshot against its plan, and conditions deletions on the saved row values.
Jevia writers serialize with retention; long operations
can make other commands hit their database timeout. Coordinate external SQL
writers too, since they may bypass Jevia's project lock. A snapshot failure
prevents deletion. A deletion failure rolls back all batches and leaves any
finalized recovery files. If a connection fails while committing, the result may
be ambiguous: inspect active history and the reported files before retrying or
restoring. Never assume a failed response means nothing committed.

These directories are ignored local data and may contain sensitive prompts.
New snapshot files/directories use private permissions on Unix; on Windows,
protect the project directory with appropriate filesystem ACLs. Jevia syncs files
before deleting, and also syncs snapshot directories on Unix. Backups and archives
are never overwritten or automatically removed. Total disk use can increase;
SQL archival does not vacuum or compact the database.

For SQL restoration, first stop writers, export the current state to a new file,
and review the chosen archive before explicitly importing it:

```sh
jevia storage export --output .jevia/before-restore.jsonl
# Replace sql-runs-UUID.jsonl with the archive path printed by `runs archive`.
jevia storage import-jsonl --from .jevia/history-archives/sql-runs-UUID.jsonl
jevia storage import-jsonl --from .jevia/history-archives/sql-runs-UUID.jsonl --apply
```

Import keeps run IDs, outcomes, and known provenance, skips identical records,
and aborts on conflicts. It **appends restored records as newest**, not at their
original SQL positions, which can change the routing evidence window. Unknown
additive JSON fields remain in the archive but are not retained by typed import.
A full pre-archive snapshot can contain active records and is not suitable for
blind import. For JSONL restoration, stop writers and save current history before
manual replacement; an older backup would otherwise discard newer runs. SQL
snapshots do not replace a database-native disaster-recovery backup strategy.

## Recording artifact maintenance

```sh
jevia recordings inspect --json
jevia recordings cleanup --json
# Only after all local/remote harnesses and hook processes have stopped:
jevia recordings cleanup --apply --confirm-stopped --json
```

`inspect` counts local journal, loss-marker, generated-plugin, and temporary
checkpoint filenames. It does not read their payloads, parse configuration or
history, contact providers/databases, replay events, or change files. It reports
aggregate counts, not run IDs or event data. Project discovery still requires a
`.jevia/config.toml` file. Explicit inspection/cleanup inventories all directory
entries, including directories larger than 4,096 names. Unrelated names are not
retained in memory; inventory memory scales with artifact filenames, not payloads.
A read error aborts rather than treating a partial inventory as complete. Apply
still refreshes the complete journal/marker inventory before each move. These
operator-requested commands do not use automatic replay's two-second budget.

`cleanup` without `--apply` is an advisory preview using the configured JSONL,
SQLite, or PostgreSQL history. It may open normal storage/locking sidecars, but
does not alter history or create an archive. Apply additionally requires
`--confirm-stopped`, repairs private ignore rules, and rechecks each run under
its execution lease. For PostgreSQL this uses the shared session advisory lock;
the explicit confirmation covers remote or orphaned children that Jevia cannot
prove have stopped. Preview is not a reservation: the applied count may change.

Cleanup **never moves journals or loss markers**, infers outcomes, or deletes
history. Active/routed runs, unknown owners, missing/unreadable history, pending
journals/markers, unsafe files, and unmatched contents are retained with aggregate
reason counts. A temporary checkpoint must exactly match the saved observation
snapshot of a terminal run; a generated OpenCode plugin must match the current
bundled plugin and a terminal OpenCode recording. JSONL cleanup reads one fully
validated history snapshot per batch of up to 32 run owners, holding their
execution leases throughout apply. SQL retains indexed single-owner lookups
and at most one execution-lease connection. Busy owners are retained, and a
failed history scan cannot authorize any move in that batch. Cleanup still
rechecks journals and each artifact's contents before moving it. New auxiliary filenames retain
the owning run UUID; older unowned files are intentionally left for manual review.
Symlinks are rejected; Unix hard-linked files are also retained. This is
cooperating-process maintenance, not a sandbox against arbitrary filesystem
writers. Keep the project directory protected with filesystem permissions/ACLs.

Cleanup avoids rescanning the directory for artifacts already known to be
ineligible. Every possible archive move still checks freshly for that owner's
journal/loss marker, rechecks file contents, and holds its execution lease.
Large batches of eligible files can still require repeated directory reads;
the optimization does not remove these safety checks.

Eligible files are **moved, not deleted**, into a unique ignored
`.jevia/recording-archives/cleanup-*` directory. The report includes the archive
path. Files already moved remain recoverable even if a later operation fails;
inspect this directory after an error. To restore, stop writers, review the
archive, and move selected files back to `.jevia/` only when their original names
are absent. Archives are never overwritten or automatically pruned; cleanup
reduces loose artifacts, not total disk usage. No extra verification or feedback
is required, and recorded history continues to inform routing automatically.

## Repository structure

- `crates/jevia-core` contains configuration, typed API contracts, policy, and
  outcome records.
- `crates/jevia-cli` contains JSONL/SQLite/PostgreSQL persistence and terminal commands.
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

Installers replace existing regular Jevia binaries, but refuse directories,
symlinks, Windows junctions, and other non-regular entries at the binary path.
Choose a different `JEVIA_INSTALL_DIR` or inspect the conflicting entry yourself;
the installer does not delete it. Failed checksum checks preserve the old binary.

The site serves `/install.sh` and uses the current page's origin in its copyable
install command: localhost during development and the deployed domain in
production. The script downloads the pinned GitHub release binary, verifies its
SHA-256 checksum, and installs it to `~/.local/bin` by default. It does not need
Rust or Cargo and does not modify shell configuration. Run `pnpm test` in
`website` to check the installer without installing anything.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for the contribution and commit
conventions and [architecture.md](architecture.md) for component
boundaries and routing invariants.

## License

MIT
