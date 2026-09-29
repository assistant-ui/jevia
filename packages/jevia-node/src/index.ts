import type { ExecFileException } from "node:child_process";
import { constants } from "node:os";
import { isRouteRecord } from "./protocol.js";
import { runCommand } from "./command.js";

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

export interface CompleteOptions extends FeedbackOptions {
  /** Confirm your external harness and verification have both stopped. */
  confirmStopped: true;
}

export interface ListRunsOptions extends CommandOptions {
  limit?: number;
}

export type StorageTarget =
  | { backend: "sqlite"; path?: string }
  | {
      backend: "postgres";
      project: string;
      /** Environment variable NAME, never a connection URL. Defaults to JEVIA_DATABASE_URL. */
      urlEnv?: string;
      /** Development only: permit plaintext connections to loopback hosts. */
      allowInsecureLocalhost?: boolean;
    };

export type StorageSetupOptions = CommandOptions & {
  /** Explicitly import the current JSONL history, retaining the source file. */
  importJsonl?: boolean;
} & (
  | { apply?: false; confirmStopped?: never }
  | { apply: true; confirmStopped: true }
);

export interface StorageCheckOptions extends CommandOptions {
  /** Scan records without a write probe. Does not repair data or test write access. */
  deep?: boolean;
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

/** Safe, stable categories; never raw operating-system error strings. */
export type JeviaCommandErrorKind =
  | "not_found"
  | "permission_denied"
  | "timeout"
  | "aborted"
  | "output_limit"
  | "invalid_options"
  | "exit"
  | "signal"
  | "spawn_failed";

export class JeviaCommandError extends Error {
  #command: readonly string[];
  #stdout: string;
  #stderr: string;
  readonly exitCode: number | null;
  readonly signal: NodeJS.Signals | null;
  readonly kind: JeviaCommandErrorKind;

  constructor(
    command: readonly string[],
    cause: ExecFileException,
    stdout: string,
    stderr: string,
  ) {
    // Process messages/causes can contain the entire task and captured output.
    // Keep raw diagnostics behind explicit getters, out of normal error logging.
    const kind = commandErrorKind(cause);
    super(kind === "timeout" ? "Jevia command timed out"
      : kind === "aborted" ? "Jevia command aborted" : "Jevia command failed");
    this.name = "JeviaCommandError";
    this.kind = kind;
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
  readonly kind = "protocol" as const;
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
    if (!Number.isSafeInteger(this.timeoutMs) || this.timeoutMs <= 0 || this.timeoutMs > 2_147_483_647) {
      throw new TypeError("timeoutMs must be an integer between 1 and 2147483647");
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

  /**
   * Route using recent eligible outcomes from the project's configured storage.
   * Recorded feedback is loaded automatically and participates in the cache key;
   * callers do not need to fetch or resend history. Does not execute the task.
   */
  async route(task: string, options: RouteOptions = {}): Promise<RouteRecord> {
    requireText(task, "task");
    const args = ["route", "--json"];
    if (options.noCache) args.push("--no-cache");
    args.push("--", task);
    return this.record(await this.execute(args, options.signal));
  }

  /**
   * Record an application-reported outcome for subsequent routing automatically.
   * No built-in verifier is required; provenance remains manual, not verification.
   * Use unknown when the result is uncertain (excluded from learning).
   */
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

  /** Explicitly finish external work; ordinary feedback does not close a run. */
  async complete(runId: string, outcome: Outcome, options: CompleteOptions): Promise<RouteRecord> {
    requireText(runId, "runId");
    if (options?.confirmStopped !== true) {
      throw new TypeError("external completion requires confirmStopped: true");
    }
    if (!(["success", "failure", "unknown"] as const).includes(outcome)) {
      throw new TypeError("outcome must be success, failure, or unknown");
    }
    const args = ["runs", "complete", "--json", "--confirm-stopped"];
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

  /** Preview by default. Requires initialized project config and CLI >= 0.1.2.
   * Returns a human-readable CLI report, not a stable machine-readable schema.
   */
  async setupStorage(target: StorageTarget, options: StorageSetupOptions = {}): Promise<string> {
    requireOptionalBoolean(options.apply, "apply");
    requireOptionalBoolean(options.confirmStopped, "confirmStopped");
    requireOptionalBoolean(options.importJsonl, "importJsonl");
    if (options.apply === true ? options.confirmStopped !== true : options.confirmStopped !== undefined) {
      throw new TypeError("applying storage requires apply: true and confirmStopped: true together");
    }
    if (!target || typeof target !== "object" || Array.isArray(target)) {
      throw new TypeError("storage target must specify sqlite or postgres");
    }
    const args = ["storage", "setup"];
    if (target.backend === "sqlite") {
      requireKeys(target, ["backend", "path"]);
      args.push("sqlite");
      if (target.path !== undefined) {
        requireText(target.path, "path");
        if (/[?#]/.test(target.path) || target.path.includes(":memory:")) {
          throw new TypeError("path must name a persistent SQLite file without query or fragment");
        }
        args.push(`--path=${target.path}`);
      }
    } else if (target.backend === "postgres") {
      requireKeys(target, ["backend", "project", "urlEnv", "allowInsecureLocalhost"]);
      if (typeof target.project !== "string" || !/^[A-Za-z0-9_.-]{1,128}$/.test(target.project)) {
        throw new TypeError("project must contain 1–128 ASCII letters, digits, dots, dashes, or underscores");
      }
      const urlEnv = target.urlEnv === undefined ? "JEVIA_DATABASE_URL" : target.urlEnv;
      if (typeof urlEnv !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(urlEnv)) {
        throw new TypeError("urlEnv must be an environment variable name, not a database URL");
      }
      requireOptionalBoolean(target.allowInsecureLocalhost, "allowInsecureLocalhost");
      args.push("postgres", `--project=${target.project}`, `--url-env=${urlEnv}`);
      if (target.allowInsecureLocalhost === true) args.push("--allow-insecure-localhost");
    } else {
      throw new TypeError("storage target must specify sqlite or postgres");
    }
    if (options.importJsonl === true) args.push("--import-jsonl");
    if (options.apply === true) args.push("--apply", "--confirm-stopped");
    return this.execute(args, options.signal);
  }

  /** Check configured storage without initializing it. Returns a human-readable CLI report. */
  async checkStorage(options: StorageCheckOptions = {}): Promise<string> {
    requireOptionalBoolean(options.deep, "deep");
    return this.execute(["storage", "check", ...(options.deep === true ? ["--deep"] : [])], options.signal);
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
    return runCommand({
      binary: this.binary, args: [...this.binaryArgs, ...args], cwd: this.cwd,
      env: this.env, timeoutMs: this.timeoutMs, maxBufferBytes: this.maxBufferBytes,
    }, signal, (cause, stdout, stderr) => new JeviaCommandError(command, cause, stdout, stderr));
  }
}

function requireText(value: string, name: string): void {
  if (typeof value !== "string" || !value.trim()) throw new TypeError(`${name} cannot be empty`);
  if (value.includes("\0")) throw new TypeError(`${name} cannot contain NUL`);
}

function commandErrorKind(cause: ExecFileException): JeviaCommandErrorKind {
  switch (cause.code) {
    // ENOENT can mean a missing executable OR cwd. Do not misdiagnose which one.
    case "ENOENT": case "ENOTDIR": return "not_found";
    case "EACCES": case "EPERM": return "permission_denied";
    case "JEVIA_TIMEOUT": return "timeout";
    case "ABORT_ERR": return "aborted";
    case "ERR_CHILD_PROCESS_STDIO_MAXBUFFER": return "output_limit";
    case "ERR_INVALID_ARG_TYPE": case "ERR_INVALID_ARG_VALUE":
    case "ERR_OUT_OF_RANGE": case "ERR_INVALID_FILE_URL_PATH":
    case "ERR_INVALID_FILE_URL_HOST": case "ERR_INVALID_URL_SCHEME":
      return "invalid_options";
  }
  if (typeof cause.code === "number" && Number.isSafeInteger(cause.code)) return "exit";
  if (cause.signal && Object.hasOwn(constants.signals, cause.signal)) return "signal";
  return "spawn_failed";
}

function requireOptionalBoolean(value: unknown, name: string): void {
  if (value !== undefined && typeof value !== "boolean") throw new TypeError(`${name} must be a boolean`);
}

function requireKeys(value: object, allowed: readonly string[]): void {
  if (Object.keys(value).some((key) => !allowed.includes(key))) {
    throw new TypeError("unsupported storage target option; pass PostgreSQL credentials through the environment");
  }
}
