import type { RouteRecord } from "./index.js";

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

const isIdentifier: Guard = (value) => typeof value === "string" && value.length > 0 && value.length <= 256 && !/[^a-zA-Z0-9._:/@+-]/.test(value);
const isEventKind = oneOf("session_started", "session_ended", "turn_started", "turn_completed", "turn_failed",
  "tool_succeeded", "tool_failed", "task_reported_complete", "model_changed", "subagent_started", "subagent_stopped");
const isHarnessEvent: Guard = (value) => isObject(value) && isEventKind(value.kind) && isUnsigned(value.recorded_at_ms) &&
  ["session_id", "agent_id", "model", "previous_model", "tool_name"].every((key) => optional(value[key], isIdentifier));
const isObservations: Guard = (value) => isObject(value) && nullable(oneOf("claude_hooks"))(value.source) &&
  oneOf("unsupported", "disabled", "unavailable", "no_events", "recorded", "partial")(value.status) &&
  Array.isArray(value.events) && value.events.length <= 256 && value.events.every(isHarnessEvent) &&
  (value.events.length === 0 || (value.source !== null && (value.status === "recorded" || value.status === "partial"))) &&
  (value.status !== "recorded" || value.events.length > 0);

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
    (value.schema_version === 1 || value.schema_version === 2 || value.schema_version === 3 || value.schema_version === 4) &&
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
