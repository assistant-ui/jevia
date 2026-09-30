import { JeviaClient } from "../src/index.js";
const client = new JeviaClient();
void client.complete("id", "success", { confirmStopped: true });
void client.complete("id", "unknown", { confirmStopped: true, reason: "checks inconclusive", signal: new AbortController().signal });
// @ts-expect-error confirmation is required
void client.complete("id", "success");
// @ts-expect-error false does not confirm completion
void client.complete("id", "success", { confirmStopped: false });
