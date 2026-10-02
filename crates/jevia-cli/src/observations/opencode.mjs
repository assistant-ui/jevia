// Session-local OpenCode v1 plugin. No network, dependencies, raw content, or
// modifications to model responses/permissions. Observations are best effort.
import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { lstat, open } from "node:fs/promises";
import { basename, dirname } from "node:path";

// Match the collector's empty .loss sidecar: one or more unconfirmed writes,
// never a payload or an exact missing-event count. Do not depend on spawning
// another collector when the executable itself may be unavailable.
async function recordLoss(journal) {
  if (!/^jevia-events-.+\.jsonl$/.test(basename(journal)) || !(await lstat(journal)).isFile()) {
    throw new Error("invalid journal");
  }
  const file = await open(journal, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
  try {
    if (!(await file.stat()).isFile()) throw new Error("invalid journal");
    const buffer = Buffer.alloc(512 * 1024 + 1);
    let length = 0;
    while (length < buffer.length) {
      const { bytesRead } = await file.read(buffer, length, buffer.length - length, null);
      if (!bytesRead) break;
      length += bytesRead;
    }
    if (length === buffer.length) throw new Error("journal limit exceeded");
    const snapshot = JSON.parse(buffer.subarray(0, length).toString("utf8"));
    if (snapshot.type !== "snapshot" || snapshot.event?.source !== "opencode_plugin" || !Array.isArray(snapshot.event.events)) {
      throw new Error("invalid journal source");
    }
  } finally {
    await file.close();
  }
  const marker = journal.slice(0, -6) + ".loss";
  let loss;
  try {
    loss = await open(marker, "wx", 0o600);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const stat = await lstat(marker);
    if (!stat.isFile() || stat.size !== 0) throw new Error("invalid loss marker");
    return;
  }
  try { await loss.sync(); } finally { await loss.close(); }
  if (process.platform !== "win32") {
    const directory = await open(dirname(journal), "r");
    try { await directory.sync(); } finally { await directory.close(); }
  }
}

export default async function jeviaObservations() {
  const executable = process.env.JEVIA_OBSERVATION_EXECUTABLE;
  const journal = process.env.JEVIA_OBSERVATION_JOURNAL;
  let warned = false;
  const lost = async () => {
    try { await recordLoss(journal); } catch {
      if (!warned) {
        warned = true;
        console.error("jevia: native observation recording could not be confirmed (details redacted)");
      }
    }
  };
  // Error parts can be published more than once. Retain only bounded opaque
  // identifiers, never their arguments, outputs, or error text.
  const failedParts = new Set();
  let failuresTruncated = false;
  const identifier = (value) => typeof value === "string" && value.length > 0 &&
    value.length <= 256 && !/[^a-zA-Z0-9._:/@+-]/.test(value) ? value : undefined;

  const emit = (hook_event_name, fields = {}) => new Promise((resolve) => {
    if (!executable || !journal) return resolve();
    const event = { hook_event_name };
    for (const key of ["session_id", "agent_id", "model", "tool_name"]) {
      const value = identifier(fields[key]);
      if (value) event[key] = value;
    }
    let settled = false;
    let timer;
    let killTimer;
    let failed = false;
    const done = (failure) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(killTimer);
      if (failure || failed) void lost().finally(resolve);
      else resolve();
    };
    try {
      const child = spawn(executable, ["capture-event", "--journal", journal, "--source", "opencode_plugin"], {
        stdio: ["pipe", "ignore", "ignore"], windowsHide: true,
      });
      timer = setTimeout(() => {
        failed = true;
        child.kill();
        // A stuck collector must not outlive the hook indefinitely after SIGTERM.
        killTimer = setTimeout(() => { child.kill("SIGKILL"); done(true); }, 100);
      }, 1500);
      child.on("error", () => done(true));
      child.on("close", (code, signal) => done(code !== 0 || signal !== null));
      child.stdin.on("error", () => { failed = true; });
      child.stdin.end(JSON.stringify(event));
    } catch { done(true); }
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
      if (event?.type === "message.part.updated" && p?.part?.type === "tool" && p.part.state?.status === "error") {
        const id = identifier(p.part.id);
        if (id && failedParts.has(id)) return;
        if (!id || failedParts.size >= 1024) {
          if (failuresTruncated) return;
          failuresTruncated = true;
          // The collector marks this unrecognized input as discarded/partial.
          return emit("UnobservedToolFailure");
        }
        failedParts.add(id);
        return emit("PostToolUseFailure", { session_id: p.part.sessionID, tool_name: p.part.tool });
      }
      if (event?.type === "session.created") {
        return emit(p?.info?.parentID ? "SubagentStart" : "SessionStart", {
          session_id: p?.info?.id, agent_id: p?.info?.parentID ? p.info.id : undefined,
        });
      }
      if (event?.type === "session.idle") return emit("Stop", { session_id: p?.sessionID });
      if (event?.type === "session.error") return emit("StopFailure", { session_id: p?.sessionID });
      // Other message/part updates are not counted as independent attempts.
    },
  };
}
