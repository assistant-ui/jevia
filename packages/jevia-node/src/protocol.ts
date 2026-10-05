import type { RouteRecord, StorageCheckReport } from "./index.js";

type Guard = (value: unknown) => boolean;
const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const isText: Guard = (value) => typeof value === "string" && value.trim().length > 0;
const isUnsigned: Guard = (value) => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const isProbability: Guard = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;
const isExitCode: Guard = (value) => value === null || (
  typeof value === "number" && Number.isInteger(value) && value >= -2147483648 && value <= 2147483647
);
const oneOf = (...choices: readonly string[]): Guard => (value) =>
  typeof value === "string" && choices.includes(value);
const isOutcome = oneOf("success", "failure", "unknown");
const isOutcomeSource = oneOf("process_exit", "verification", "manual");
const isState = oneOf("routed", "running", "verifying", "completed", "launch_failed", "interrupted", "cancelled", "timed_out");
const nullable = (guard: Guard): Guard => (value) => value === null || guard(value);
const optional = (value: unknown, guard: Guard): boolean => value === undefined || guard(value);

export function isStorageCheckReport(value: unknown): value is StorageCheckReport {
  if (!isObject(value) || value.schema_version !== 1 || !oneOf("basic", "deep")(value.check) ||
    typeof value.ok !== "boolean" || !nullable(oneOf("jsonl", "sqlite", "postgres"))(value.backend) ||
    !nullable(isUnsigned)(value.records)) return false;
  if (value.ok) {
    if (value.backend === null || value.records === null || value.error !== null) return false;
    if (value.backend === "jsonl") return value.passive_history_index === "not_applicable";
    return oneOf("present", "missing", "unavailable")(value.passive_history_index) ||
      (value.backend === "postgres" && value.passive_history_index === "unsupported");
  }
  if (value.passive_history_index !== null || !isObject(value.error) || !isText(value.error.message)) return false;
  switch (value.error.code) {
    case "configuration_unavailable": return value.backend === null && value.records === null;
    case "storage_unavailable":
    case "history_check_failed": return value.backend !== null && value.records === null;
    case "index_check_failed": return value.backend !== null && value.records !== null;
    default: return false;
  }
}

const isIdentifier: Guard = (value) => typeof value === "string" && value.length > 0 && value.length <= 256 && !/[^a-zA-Z0-9._:/@+-]/.test(value);
const isEventKind = oneOf("session_started", "session_ended", "turn_started", "turn_completed", "turn_failed",
  "tool_succeeded", "tool_failed", "tool_completed", "turn_interrupted", "model_observed",
  "task_reported_complete", "model_changed", "subagent_started", "subagent_stopped");
const isHarnessEvent: Guard = (value) => isObject(value) && isEventKind(value.kind) && isUnsigned(value.recorded_at_ms) &&
  ["session_id", "agent_id", "model", "previous_model", "tool_name"].every((key) => optional(value[key], isIdentifier));
const isEventCounts = (value: unknown): value is Record<string, number> => isObject(value) &&
  Object.entries(value).every(([kind, count]) => isEventKind(kind) && isUnsigned(count));
const isTotals: Guard = (value) => {
  if (!isObject(value) || !isEventCounts(value.event_counts) || !isEventCounts(value.unattributed_event_counts) ||
    !isEventCounts(value.omitted_model_event_counts) || !isObject(value.models) || Object.keys(value.models).length > 32 ||
    typeof value.models_truncated !== "boolean" || !isUnsigned(value.discarded_inputs)) return false;
  if (Object.keys(value.omitted_model_event_counts).length > 0 && !value.models_truncated) return false;
  const sum = { ...value.unattributed_event_counts };
  for (const [model, counts] of Object.entries(value.models)) if (!isIdentifier(model) || !isEventCounts(counts)) return false;
  for (const counts of [...Object.values(value.models), value.omitted_model_event_counts] as Record<string, number>[]) {
    for (const [kind, count] of Object.entries(counts)) sum[kind] = (sum[kind] ?? 0) + count;
  }
  return isUnsigned(Object.values(value.event_counts).reduce((a, b) => a + b, 0)) &&
    Object.keys(sum).length === Object.keys(value.event_counts).length &&
    Object.entries(sum).every(([kind, count]) => count === (value.event_counts as Record<string, number>)[kind]);
};
const matchesTotals = (value: Record<string, unknown>): boolean => {
  if (value.totals === undefined) return true;
  if (!isTotals(value.totals)) return false;
  const totals = value.totals as {
    event_counts: Record<string, number>; discarded_inputs: number;
    models: Record<string, Record<string, number>>; models_truncated: boolean;
    unattributed_event_counts: Record<string, number>; omitted_model_event_counts: Record<string, number>;
  };
  const events = value.events as { kind: string; model?: string; previous_model?: string }[];
  const count = Object.values(totals.event_counts).reduce((a, b) => a + b, 0);
  // Mirror Rust's attribution buckets without mutating the returned CLI record.
  const models = new Map(Object.entries(totals.models).map(([model, counts]) => [model, { ...counts }]));
  const unattributed = { ...totals.unattributed_event_counts };
  const omitted = { ...totals.omitted_model_event_counts };
  return count >= events.length && (count === 0 || events.length > 0) &&
    (totals.discarded_inputs === 0 || value.status === "partial") &&
    events.every((event) => {
      if (event.previous_model !== undefined && !models.has(event.previous_model) && !totals.models_truncated) return false;
      const bucket = event.model === undefined ? unattributed
        : models.get(event.model) ?? (totals.models_truncated ? omitted : undefined);
      if (!bucket || !(bucket[event.kind]! > 0)) return false;
      bucket[event.kind]! -= 1;
      return true;
    });
};
const isObservations: Guard = (value) => isObject(value) && nullable(oneOf("claude_hooks", "codex_hooks", "opencode_plugin", "application"))(value.source) &&
  oneOf("unsupported", "disabled", "unavailable", "no_events", "recorded", "partial")(value.status) &&
  Array.isArray(value.events) && value.events.length <= 256 && value.events.every(isHarnessEvent) &&
  (value.events.length === 0 || (value.source !== null && (value.status === "recorded" || value.status === "partial"))) &&
  (value.status !== "recorded" || value.events.length > 0) && matchesTotals(value);

const isVerification: Guard = (value) => isObject(value) &&
  isText(value.command) && typeof value.launched === "boolean" &&
  isUnsigned(value.duration_ms) && isExitCode(value.exit_code);
const isExecution: Guard = (value) => isObject(value) &&
  isText(value.harness) && isText(value.model) && isUnsigned(value.duration_ms) &&
  isExitCode(value.exit_code) && optional(value.verification, isVerification) && optional(value.observations, isObservations);
const isLifecycle: Guard = (value) => isObject(value) && isState(value.state) &&
  nullable(isUnsigned)(value.started_at_ms) && nullable(isUnsigned)(value.finished_at_ms);
const isEvidence: Guard = (value) => isObject(value) &&
  isOutcomeSource(value.source) && isUnsigned(value.recorded_at_ms);
const isFeedbackEvent: Guard = (value) => isObject(value) &&
  isOutcome(value.previous_outcome) && nullable(isOutcomeSource)(value.previous_source) &&
  isOutcome(value.outcome) && isUnsigned(value.recorded_at_ms) &&
  (value.reason === null || typeof value.reason === "string");
const isFeedback: Guard = (value) => Array.isArray(value) && value.every(isFeedbackEvent);

/** Validate the CLI wire contract without echoing values or inventing legacy evidence. */
export function isRouteRecord(value: unknown): value is RouteRecord {
  if (!isObject(value)) return false;
  return (
    (value.schema_version === 1 || value.schema_version === 2 || value.schema_version === 3 || value.schema_version === 4 || value.schema_version === 5 || value.schema_version === 6) &&
    isText(value.run_id) && isText(value.tier) && isText(value.suggested_tier) &&
    isProbability(value.confidence) && isObject(value.probabilities) &&
    Object.values(value.probabilities).every(isProbability) &&
    typeof value.fallback_applied === "boolean" && isText(value.jev_model) &&
    isUnsigned(value.created_at_ms) && (value.source === "live" || value.source === "cache") &&
    (value.task === null || typeof value.task === "string") && isOutcome(value.outcome) &&
    optional(value.execution, isExecution) && optional(value.lifecycle, isLifecycle) &&
    optional(value.outcome_evidence, isEvidence) && optional(value.feedback, isFeedback)
  );
}

/** Reject extra payload fields rather than silently accepting prompt/tool contents. */
export function isExecutionRecording(value: unknown): boolean {
  return isObject(value) && Object.keys(value).every((key) => ["harness", "model", "duration_ms", "exit_code", "events"].includes(key)) &&
    isIdentifier(value.harness) && isIdentifier(value.model) && isUnsigned(value.duration_ms) &&
    optional(value.exit_code, isExitCode) && optional(value.events, (events) =>
      Array.isArray(events) && events.length <= 256 && events.every((event: unknown) =>
        isObject(event) && isHarnessEvent(event) && Object.keys(event).every((key) =>
          ["kind", "recorded_at_ms", "session_id", "agent_id", "model", "previous_model", "tool_name"].includes(key))));
}
