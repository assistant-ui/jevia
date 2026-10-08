import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { isRouteRecord } from "../dist/protocol.js";

const fixture = JSON.parse(readFileSync(new URL("../../../crates/jevia-core/tests/fixtures/numeric-record.json", import.meta.url), "utf8"));

// Keep fixtures local to each published Rust crate, and catch contract drift here.
assert.deepEqual(fixture, JSON.parse(readFileSync(new URL("../../../crates/jevia-cli/tests/fixtures/numeric-record.json", import.meta.url), "utf8")));

test("execution text agrees with Rust without requiring verification", () => {
  const cases = JSON.parse(readFileSync(new URL("../../../crates/jevia-core/tests/fixtures/execution-text.json", import.meta.url), "utf8"));
  for (const schema_version of [1, 2, 3, 4, 5, 6]) {
    for (const path of [["execution", "harness"], ["execution", "model"], ["execution", "verification", "command"]]) {
      for (const [text, valid] of cases) {
        const value = structuredClone(fixture);
        value.schema_version = schema_version;
        path.slice(0, -1).reduce((object, key) => object[key], value)[path.at(-1)] = text;
        assert.equal(isRouteRecord(value), valid, `${schema_version}:${path.join(".")}:${JSON.stringify(text)}`);
      }
    }
  }
  const optional = structuredClone(fixture);
  delete optional.execution.verification;
  assert.ok(isRouteRecord(optional));
  delete optional.execution;
  delete optional.lifecycle;
  assert.ok(isRouteRecord(optional));
});

test("history timestamps and durations agree with Rust at safe-integer boundaries", () => {
  for (const schema_version of [1, 2, 3, 4, 5, 6]) {
    for (const path of [
      ["created_at_ms"], ["lifecycle", "started_at_ms"], ["lifecycle", "finished_at_ms"],
      ["outcome_evidence", "recorded_at_ms"], ["feedback", 0, "recorded_at_ms"],
      ["execution", "duration_ms"], ["execution", "verification", "duration_ms"],
      ["execution", "observations", "events", 0, "recorded_at_ms"],
    ]) {
      for (const number of [0, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1, 2 ** 64, -1, 0.5]) {
        const value = structuredClone(fixture);
        value.schema_version = schema_version;
        const owner = path.slice(0, -1).reduce((object, key) => object[key], value);
        owner[path.at(-1)] = number;
        assert.equal(isRouteRecord(value), Number.isSafeInteger(number) && number >= 0, `${schema_version}:${path.join(".")}:${number}`);
      }
    }
  }
});
