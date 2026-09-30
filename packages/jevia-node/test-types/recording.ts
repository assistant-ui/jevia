import { JeviaClient, type ExecutionRecording, type RouteRecord } from "../src/index.js";

const client = new JeviaClient();
const input: ExecutionRecording = {
  harness: "app", model: "requested", duration_ms: 10,
  events: [{ kind: "model_observed", model: "observed", recorded_at_ms: 1 }],
};
const recorded: Promise<RouteRecord> = client.recordExecution("run", input, { signal: new AbortController().signal });
void recorded;
// @ts-expect-error Outcome assertions belong to the separate optional feedback API.
client.recordExecution("run", { ...input, outcome: "success" });
// @ts-expect-error Raw tool output is not a permitted event field.
client.recordExecution("run", { ...input, events: [{ kind: "tool_completed", recorded_at_ms: 1, output: "raw" }] });
