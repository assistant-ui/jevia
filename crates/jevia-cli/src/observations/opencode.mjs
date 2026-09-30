// Session-local OpenCode v1 plugin. No network, dependencies, raw content, or
// modifications to agent output/permissions. Observations are best effort.
import { spawn } from "node:child_process";

export default async function jeviaObservations() {
  const executable = process.env.JEVIA_OBSERVATION_EXECUTABLE;
  const journal = process.env.JEVIA_OBSERVATION_JOURNAL;
  const identifier = (value) => typeof value === "string" && value.length > 0 &&
    value.length <= 256 && !/[^a-zA-Z0-9._:/@+-]/.test(value) ? value : undefined;

  const emit = (hook_event_name, fields = {}) => new Promise((resolve) => {
    if (!executable || !journal) return resolve();
    const event = { hook_event_name };
    for (const key of ["session_id", "agent_id", "model", "tool_name"]) {
      const value = identifier(fields[key]);
      if (value) event[key] = value;
    }
    try {
      const child = spawn(executable, ["capture-event", "--journal", journal, "--source", "opencode_plugin"], {
        stdio: ["pipe", "ignore", "ignore"], windowsHide: true,
      });
      const timer = setTimeout(() => { child.kill(); resolve(); }, 1500);
      const done = () => { clearTimeout(timer); resolve(); };
      child.on("error", done);
      child.on("close", done);
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(event));
    } catch { resolve(); }
  });

  return {
    "chat.message": async ({ sessionID }) => emit("UserPromptSubmit", { session_id: sessionID }),
    // Request selection is explicit model metadata, not a completion or proof
    // that a provider actually executed it. Never infer fallback attempts.
    "chat.params": async ({ sessionID, model }) => {
      const provider = identifier(model?.providerID);
      const id = identifier(model?.id);
      return emit("ModelObserved", { session_id: sessionID, model: provider && id ? `${provider}/${id}` : undefined });
    },
    // The after-hook does not establish semantic success. Do not inspect output.
    "tool.execute.after": async ({ sessionID, tool }) => emit("PostToolUse", { session_id: sessionID, tool_name: tool }),
    event: async ({ event }) => {
      const p = event?.properties;
      if (event?.type === "session.created") {
        return emit(p?.info?.parentID ? "SubagentStart" : "SessionStart", {
          session_id: p?.info?.id, agent_id: p?.info?.parentID ? p.info.id : undefined,
        });
      }
      if (event?.type === "session.idle") return emit("Stop", { session_id: p?.sessionID });
      if (event?.type === "session.error") return emit("StopFailure", { session_id: p?.sessionID });
      // message.updated and part.updated can repeat: not counted as attempts.
    },
  };
}
