import assert from "node:assert/strict";
import test from "node:test";
import { JeviaClient, JeviaProtocolError } from "../dist/index.js";
import { isRouteRecord } from "../dist/protocol.js";

const record = {
  schema_version: 3, run_id: "id", tier: "fast", suggested_tier: "fast",
  confidence: 0.9, probabilities: { fast: 0.9 }, fallback_applied: false,
  jev_model: "test", created_at_ms: 1, source: "live", task: null, outcome: "unknown",
};
const execution = { harness: "agent", model: "test", duration_ms: 4, exit_code: 0 };
const verification = { command: "test", launched: true, duration_ms: 1, exit_code: null };
const lifecycle = { state: "completed", started_at_ms: 1, finished_at_ms: 2 };
const evidence = { source: "manual", recorded_at_ms: 3 };
const feedback = { previous_outcome: "unknown", previous_source: null, outcome: "success", recorded_at_ms: 3, reason: null };

test("accepts supported legacy records without inventing optional evidence", () => {
  for (const schema_version of [1, 2, 3, 4, 5]) {
    assert.ok(isRouteRecord({ ...record, schema_version }));
  }
  assert.ok(isRouteRecord({ ...record, execution: { ...execution, verification }, lifecycle, outcome_evidence: evidence, feedback: [feedback], future_metadata: {} }));
  assert.ok(isRouteRecord({ ...record, probabilities: {}, task: "", feedback: [] }));
});

test("rejects invalid scalar, schema, timestamp, and probability fields", () => {
  for (const [field, values] of Object.entries({
    schema_version: [0, 6, 999, 1.5, "3"], run_id: ["", 1], tier: ["", null],
    suggested_tier: [[], ""], jev_model: [true, ""], task: [false, {}], outcome: ["done", null],
    source: ["remote", null], fallback_applied: [1, null],
    confidence: [-1, 1.1, NaN, Infinity, "0.9"],
    created_at_ms: [-1, 0.5, Number.MAX_SAFE_INTEGER + 1, Infinity, null],
    probabilities: [[], null, { fast: "0.9" }, { fast: -1 }, { fast: Infinity }, { fast: 2 }],
  })) {
    for (const value of values) assert.equal(isRouteRecord({ ...record, [field]: value }), false, field);
  }
  for (const field of Object.keys(record)) {
    const incomplete = { ...record };
    delete incomplete[field];
    assert.equal(isRouteRecord(incomplete), false, `missing ${field}`);
  }
  for (const value of [[], null, "record"]) assert.equal(isRouteRecord(value), false);
});

test("validates every optional evidence object and nested field", () => {
  const shapes = { execution, lifecycle, outcome_evidence: evidence, feedback: [feedback] };
  for (const field of Object.keys(shapes)) {
    for (const value of [null, "private-invalid", {}, 42]) {
      assert.equal(isRouteRecord({ ...record, [field]: value }), false, field);
    }
  }
  for (const [field, shape] of Object.entries({ execution, lifecycle, outcome_evidence: evidence })) {
    for (const key of Object.keys(shape)) {
      const incomplete = { ...shape };
      delete incomplete[key];
      assert.equal(isRouteRecord({ ...record, [field]: incomplete }), false, `${field}.${key}`);
      assert.equal(isRouteRecord({ ...record, [field]: { ...shape, [key]: [] } }), false, `${field}.${key}`);
    }
  }
  for (const key of Object.keys(verification)) {
    const incomplete = { ...verification };
    delete incomplete[key];
    assert.equal(isRouteRecord({ ...record, execution: { ...execution, verification: incomplete } }), false, key);
  }
  for (const key of Object.keys(feedback)) {
    const incomplete = { ...feedback };
    delete incomplete[key];
    assert.equal(isRouteRecord({ ...record, feedback: [incomplete] }), false, key);
  }
  for (const value of [-2147483649, 2147483648, 0.5, "0"]) {
    assert.equal(isRouteRecord({ ...record, execution: { ...execution, exit_code: value } }), false);
  }
  assert.equal(isRouteRecord({ ...record, lifecycle: { ...lifecycle, state: "done" } }), false);
  assert.equal(isRouteRecord({ ...record, outcome_evidence: { ...evidence, source: "guessed" } }), false);
  assert.equal(isRouteRecord({ ...record, feedback: [{ ...feedback, reason: 1 }] }), false);
});

test("validates whole-session totals separately from the bounded sample", () => {
  const event = { kind: "tool_failed", recorded_at_ms: 1, model: "actual-model" };
  const totals = { event_counts: { tool_failed: 300 }, models: { "actual-model": { tool_failed: 290 } },
    unattributed_event_counts: { tool_failed: 10 }, omitted_model_event_counts: {}, models_truncated: false, discarded_inputs: 0 };
  const observations = { source: "claude_hooks", status: "recorded", events: [event], totals };
  const wrap = (totals) => ({ ...record, schema_version: 5, execution: { ...execution, observations: { ...observations, totals } } });
  assert.ok(isRouteRecord(wrap(totals)));
  for (const invalid of [null, {}, { ...totals, event_counts: { tool_failed: 299 } },
    { ...totals, discarded_inputs: 1 }, { ...totals, discarded_inputs: Number.MAX_SAFE_INTEGER + 1 },
    { ...totals, models: { "PRIVATE CONTENT": { tool_failed: 290 } } },
    { ...totals, omitted_model_event_counts: { tool_failed: 1 } },
    { ...totals, models: Object.fromEntries(Array.from({ length: 33 }, (_, n) => [`m${n}`, {}])) },
  ]) assert.equal(isRouteRecord(wrap(invalid)), false);
});

test("validates bounded passive observations without requiring verification or known outcomes", () => {
  const event = { kind: "model_changed", recorded_at_ms: 3, model: "model-b", previous_model: "model-a" };
  const observations = { source: "claude_hooks", status: "recorded", events: [event] };
  const wrap = (observations) => ({ ...record, schema_version: 4, execution: { ...execution, observations } });
  assert.ok(isRouteRecord(wrap(observations)));
  assert.ok(isRouteRecord(wrap({ source: null, status: "unsupported", events: [] })));
  assert.ok(isRouteRecord(wrap({ ...observations, status: "partial", events: [] })));
  for (const invalid of [
    null, {}, { ...observations, source: "guessed" }, { ...observations, status: "success" },
    { ...observations, events: [] }, { ...observations, events: Array(257).fill(event) },
    { ...observations, source: null }, { ...observations, status: "disabled" },
    ...["PRIVATE bad model", "model\n", "", "a".repeat(257)].map((model) => ({ ...observations, events: [{ ...event, model }] })),
    { ...observations, events: [{ ...event, kind: "solved" }] },
    { ...observations, events: [{ ...event, recorded_at_ms: -1 }] },
  ]) assert.equal(isRouteRecord(wrap(invalid)), false);
});

test("all record-returning methods reject invalid CLI output without exposing it", async () => {
  const invalid = { ...record, lifecycle: "PRIVATE_RESPONSE_SENTINEL" };
  for (const method of ["route", "feedback", "show", "runs", "complete"]) {
    const output = JSON.stringify(method === "runs" ? [record, invalid] : invalid);
    const client = new JeviaClient({
      binary: process.execPath,
      binaryArgs: ["-e", "process.stdout.write(process.env.TEST_RESPONSE)", "--"],
      env: { TEST_RESPONSE: output },
    });
    const pending = method === "feedback" ? client.feedback("id", "success")
      : method === "complete" ? client.complete("id", "success", { confirmStopped: true })
      : method === "runs" ? client.runs() : client[method]("id");
    await assert.rejects(pending, (error) => {
      assert.ok(error instanceof JeviaProtocolError);
      assert.ok(!String(error).includes("PRIVATE_RESPONSE_SENTINEL"));
      assert.equal(error.cause, undefined);
      return true;
    });
  }
});
