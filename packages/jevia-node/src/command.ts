import { execFile, spawn, type ChildProcess, type ExecFileException } from "node:child_process";
import { join } from "node:path";

interface CommandSettings {
  binary: string;
  args: readonly string[];
  cwd: string | URL;
  env: NodeJS.ProcessEnv;
  timeoutMs: number;
  maxBufferBytes: number;
  input?: string;
}

type Failure = (cause: ExecFileException, stdout: string, stderr: string) => Error;
const TERMINATION_GRACE_MS = 100;

/** Bound settlement independently of process exit, with byte-limited capture. */
export function runCommand(settings: CommandSettings, signal: AbortSignal | undefined, failure: Failure): Promise<string> {
  return new Promise((resolve, reject) => {
    let child: ChildProcess | undefined;
    let deadline: NodeJS.Timeout | undefined;
    let settled = false;
    const output = { stdout: [] as Buffer[], stderr: [] as Buffer[] };
    const sizes = { stdout: 0, stderr: 0 };
    const finish = (error?: ExecFileException) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      signal?.removeEventListener("abort", abort);
      const stdout = Buffer.concat(output.stdout).toString("utf8");
      if (error) reject(failure(error, stdout, Buffer.concat(output.stderr).toString("utf8")));
      else resolve(stdout);
    };
    const stop = (code: "JEVIA_TIMEOUT" | "ABORT_ERR") => {
      if (settled) return;
      // Neither AbortSignal.reason nor raw process errors belong in public logs.
      finish(Object.assign(new Error("command interrupted"), { code }));
      if (child) terminate(child);
    };
    const abort = () => stop("ABORT_ERR");
    if (signal?.aborted) {
      abort();
      return;
    }
    try {
      child = spawn(settings.binary, [...settings.args], {
        cwd: settings.cwd,
        env: settings.env,
        stdio: "pipe",
        detached: process.platform !== "win32",
        windowsHide: true,
      });
      // Only explicit recording payloads use stdin, never argv or a temporary file.
      // Early CLI rejection can close the pipe before all input is written.
      child.stdin?.on("error", () => {});
      child.stdin?.end(settings.input);
      child.once("error", finish);
      child.once("close", (code, exitSignal) => {
        if (code === 0) finish();
        else finish(Object.assign(new Error("command failed"), { code: code ?? undefined, signal: exitSignal ?? undefined }));
      });
      for (const stream of ["stdout", "stderr"] as const) {
        child[stream]?.on("data", (chunk: Buffer) => {
          if (settled) return;
          const remaining = settings.maxBufferBytes - sizes[stream];
          output[stream].push(chunk.subarray(0, remaining));
          sizes[stream] += Math.min(chunk.length, remaining);
          if (chunk.length > remaining) {
            finish(Object.assign(new Error("output limit exceeded"), { code: "ERR_CHILD_PROCESS_STDIO_MAXBUFFER" }));
            if (child) terminate(child);
          }
        });
      }
      deadline = setTimeout(() => stop("JEVIA_TIMEOUT"), settings.timeoutMs);
      signal?.addEventListener("abort", abort, { once: true });
      // Also cover an abort that happened while the process was being started.
      if (signal?.aborted) abort();
    } catch (cause) {
      finish(cause instanceof Error ? cause : new Error("command could not start"));
    }
  });
}

function terminate(child: ChildProcess): void {
  const pid = child.pid;
  if (pid === undefined) return;
  const send = (signal: NodeJS.Signals) => {
    try {
      if (process.platform === "win32") return child.kill(signal);
      else process.kill(-pid, signal); // Only this call's detached process group.
      return true;
    } catch {
      // It may already be gone. Never expose system error paths or arguments.
      return false;
    }
  };
  if (process.platform === "win32") {
    // Windows has no POSIX process groups. Ask its native tool to stop the tree
    // before the root exits, with a bounded fallback to killing the direct child.
    const systemRoot = process.env.SystemRoot;
    if (systemRoot) {
      try {
        execFile(join(systemRoot, "System32", "taskkill.exe"), ["/PID", String(pid), "/T", "/F"],
          { windowsHide: true, timeout: 1000, killSignal: "SIGKILL" }, () => send("SIGKILL"));
      } catch { send("SIGKILL"); }
    } else send("SIGKILL");
  } else {
    if (send("SIGTERM")) {
      // Keep escalation alive even if the wrapper exits before descendants.
      setTimeout(() => send("SIGKILL"), TERMINATION_GRACE_MS);
    }
  }
  child.stdin?.destroy();
  child.stdout?.destroy();
  child.stderr?.destroy();
  child.unref();
}
