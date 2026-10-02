// Isolate environment, cwd, and child processes from the test runner.
import jeviaObservations from "../../crates/jevia-cli/src/observations/opencode.mjs";

const plugin = await jeviaObservations();
await plugin.event({ event: { type: "session.created", properties: { info: { id: "fixture-session" } } } });
await plugin["chat.params"]({ sessionID: "fixture-session", model: { providerID: "fixture", id: "model" } });
await plugin.event({ event: { type: "session.idle", properties: { sessionID: "fixture-session" } } });
