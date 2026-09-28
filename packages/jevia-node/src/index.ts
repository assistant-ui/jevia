import { execFile, type ExecFileException } from "node:child_process";
import { constants } from "node:os";

export type Outcome = "success" | "failure" | "unknown";
export type OutcomeSource = "process_exit" | "verification" | "manual";
export type DecisionSource = "live" | "cache";
export type RunState =
  | "routed"
  | "running"
  | "verifying"
  | "completed"
  | "launch_failed"
  | "interrupted"
  | "cancelled"
  | "timed_out";

export interface OutcomeEvidence {
  source: OutcomeSource;
  recorded_at_ms: number;
}

export interface FeedbackEvent {
  previous_outcome: Outcome;
  previous_source: OutcomeSource | null;
  outcome: Outcome;
  recorded_at_ms: number;
  reason: string | null;
}

export interface RunLifecycle {
  state: RunState;
  started_at_ms: number | null;
  finished_at_ms: number | null;
}

export interface VerificationEvidence {
  command: string;
  launched: boolean;
  duration_ms: number;
  exit_code: number | null;
}

export interface ExecutionEvidence {
  harness: string;
  model: string;
  duration_ms: number;
  exit_code: number | null;
  verification?: VerificationEvidence;
}

export interface RouteDecision {
  run_id: string;
  tier: string;
  suggested_tier: string;
  confidence: number;
  probabilities: Record<string, number>;
  fallback_applied: boolean;
  jev_model: string;
  created_at_ms: number;
  source: DecisionSource;
}

export interface RouteRecord extends RouteDecision {
  schema_version: number;
  task: string | null;
  outcome: Outcome;
  execution?: ExecutionEvidence;
  lifecycle?: RunLifecycle;
  outcome_evidence?: OutcomeEvidence;
  feedback?: FeedbackEvent[];
}

export interface CommandOptions {
  signal?: AbortSignal;
}

export interface RouteOptions extends CommandOptions {
  noCache?: boolean;
}

export interface FeedbackOptions extends CommandOptions {
  reason?: string;
}

export interface ListRunsOptions extends CommandOptions {
  limit?: number;
}

export interface JeviaClientOptions {
  /** Jevia executable name or absolute path. Defaults to `jevia`. */
  binary?: string;
  /** Arguments inserted before every Jevia command, useful for wrappers and tests. */
  binaryArgs?: readonly string[];
  /** Project directory containing `.jevia/config.toml`. Defaults to `process.cwd()`. */
  cwd?: string | URL;
  /** Environment overrides merged with the current process environment. */
  env?: NodeJS.ProcessEnv;
  /** Maximum time for one CLI call. Defaults to 30 seconds. */
  timeoutMs?: number;
  /** Maximum captured stdout or stderr size. Defaults to 4 MiB. */
  maxBufferBytes?: number;
}

export class JeviaCommandError extends Error {
  #command: readonly string[];
  #stdout: string;
  #stderr: string;
  readonly exitCode: number | null;
  readonly signal: NodeJS.Signals | null;

  constructor(
    command: readonly string[],
    cause: ExecFileException,
    stdout: string,
    stderr: string,
  ) {
    // execFile's message/cause can contain the entire task and captured output.
    // Keep raw diagnostics behind explicit getters, out of normal error logging.
    super("Jevia command failed");
    this.name = "JeviaCommandError";
    this.#command = Object.freeze([...command]);
    this.exitCode = typeof cause.code === "number" && Number.isSafeInteger(cause.code)
      ? cause.code : null;
    this.signal = cause.signal && Object.hasOwn(constants.signals, cause.signal)
      ? cause.signal : null;
    this.#stdout = stdout;
    this.#stderr = stderr;
  }

  /** Sensitive: explicitly accessing this getter reveals literal CLI arguments. */
  get command(): readonly string[] { return this.#command; }
  /** Sensitive: explicitly accessing this getter reveals raw CLI output. */
  get stdout(): string { return this.#stdout; }
  /** Sensitive: explicitly accessing this getter reveals raw CLI diagnostics. */
  get stderr(): string { return this.#stderr; }
}

export class JeviaProtocolError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "JeviaProtocolError";
  }
}

export class JeviaClient {
  readonly binary: string;
  readonly binaryArgs: readonly string[];
  readonly cwd: string | URL;
  readonly env: NodeJS.ProcessEnv;
  readonly timeoutMs: number;
  readonly maxBufferBytes: number;

  constructor(options: JeviaClientOptions = {}) {
    this.binary = options.binary ?? "jevia";
    this.binaryArgs = [...(options.binaryArgs ?? [])];
    this.cwd = options.cwd ?? process.cwd();
    this.env = { ...process.env, ...options.env };
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.maxBufferBytes = options.maxBufferBytes ?? 4 * 1024 * 1024;

    if (!this.binary.trim()) throw new TypeError("binary cannot be empty");
    if (!Number.isSafeInteger(this.timeoutMs) || this.timeoutMs <= 0) {
      throw new TypeError("timeoutMs must be a positive safe integer");
    }
    if (!Number.isSafeInteger(this.maxBufferBytes) || this.maxBufferBytes <= 0) {
      throw new TypeError("maxBufferBytes must be a positive safe integer");
    }
  }

  async version(options: CommandOptions = {}): Promise<string> {
    const output = (await this.execute(["--version"], options.signal)).trim();
    const match = /^jevia\s+(\S+)$/.exec(output);
    if (!match?.[1]) {
      throw new JeviaProtocolError("Jevia returned an unrecognized version string");
    }
    return match[1];
  }

  async route(task: string, options: RouteOptions = {}): Promise<RouteRecord> {
    requireText(task, "task");
    const args = ["route", "--json"];
    if (options.noCache) args.push("--no-cache");
    args.push("--", task);
    return this.record(await this.execute(args, options.signal));
  }

  async feedback(
    runId: string,
    outcome: Outcome,
    options: FeedbackOptions = {},
  ): Promise<RouteRecord> {
    requireText(runId, "runId");
    if (!(["success", "failure", "unknown"] as const).includes(outcome)) {
      throw new TypeError("outcome must be success, failure, or unknown");
    }

    const args = ["feedback", "--json"];
    if (options.reason !== undefined) {
      requireText(options.reason, "reason");
      args.push(`--reason=${options.reason}`);
    }
    args.push("--", runId, outcome);
    return this.record(await this.execute(args, options.signal));
  }

  async runs(options: ListRunsOptions = {}): Promise<RouteRecord[]> {
    const limit = options.limit ?? 20;
    if (!Number.isSafeInteger(limit) || limit <= 0) {
      throw new TypeError("limit must be a positive safe integer");
    }
    const value = this.json(
      await this.execute(["runs", "--limit", String(limit), "--json"], options.signal),
    );
    if (!Array.isArray(value) || !value.every(isRouteRecord)) {
      throw new JeviaProtocolError("Jevia returned an invalid run list");
    }
    return value;
  }

  async show(runId: string, options: CommandOptions = {}): Promise<RouteRecord> {
    requireText(runId, "runId");
    return this.record(
      await this.execute(["runs", "show", "--json", "--", runId], options.signal),
    );
  }

  private record(output: string): RouteRecord {
    const value = this.json(output);
    if (!isRouteRecord(value)) {
      throw new JeviaProtocolError("Jevia returned an invalid route record");
    }
    return value;
  }

  private json(output: string): unknown {
    try {
      return JSON.parse(output) as unknown;
    } catch {
      throw new JeviaProtocolError("Jevia returned invalid JSON");
    }
  }

  private execute(args: readonly string[], signal?: AbortSignal): Promise<string> {
    const command = [this.binary, ...this.binaryArgs, ...args];
    return new Promise((resolve, reject) => {
      try {
        execFile(
          this.binary,
          [...this.binaryArgs, ...args],
          {
            cwd: this.cwd,
            env: this.env,
            encoding: "utf8",
            maxBuffer: this.maxBufferBytes,
            signal,
            timeout: this.timeoutMs,
            windowsHide: true,
          },
          (error, stdout, stderr) => {
            if (error) {
              reject(new JeviaCommandError(command, error, stdout, stderr));
              return;
            }
            resolve(stdout);
          },
        );
      } catch (cause) {
        // Invalid spawn options can throw synchronously, before the callback.
        reject(new JeviaCommandError(command, cause instanceof Error ? cause : new Error(), "", ""));
      }
    });
  }
}

function requireText(value: string, name: string): void {
  if (typeof value !== "string" || !value.trim()) throw new TypeError(`${name} cannot be empty`);
  if (value.includes("\0")) throw new TypeError(`${name} cannot contain NUL`);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isRouteRecord(value: unknown): value is RouteRecord {
  if (!isObject(value)) return false;
  return (
    typeof value.schema_version === "number" &&
    typeof value.run_id === "string" &&
    typeof value.tier === "string" &&
    typeof value.suggested_tier === "string" &&
    typeof value.confidence === "number" &&
    isObject(value.probabilities) &&
    typeof value.fallback_applied === "boolean" &&
    typeof value.jev_model === "string" &&
    typeof value.created_at_ms === "number" &&
    (value.source === "live" || value.source === "cache") &&
    (value.task === null || typeof value.task === "string") &&
    (value.outcome === "success" || value.outcome === "failure" || value.outcome === "unknown")
  );
}
