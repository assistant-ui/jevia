import { JeviaClient, type HarnessObservations, type HarnessEvent } from "../src/index.js";
const client = new JeviaClient();
async function inspect() {
  const record = await client.show("run-id");
  const observations: HarnessObservations | undefined = record.execution?.observations;
  const event: HarnessEvent | undefined = observations?.events[0];
  const model: string | undefined = event?.model;
  return model;
}
void inspect;
// Routing does not require feedback, verification, or a manually supplied history.
void client.route("task");
// @ts-expect-error activity is not a successful task label
const invalid: HarnessEvent = { kind: "solved", recorded_at_ms: 1 };
void invalid;
