import { JeviaCommandError, JeviaProtocolError, type JeviaCommandErrorKind } from "../src/index.js";

const labels: Record<JeviaCommandErrorKind, string> = {
  not_found: "Check binary and cwd",
  permission_denied: "Check execution permissions",
  timeout: "Inspect state before retrying",
  aborted: "Caller cancelled",
  output_limit: "Output was too large",
  invalid_options: "Check client options",
  exit: "CLI reported failure",
  signal: "Process received a signal",
  spawn_failed: "Other process failure",
};
function describe(error: unknown): string {
  if (error instanceof JeviaCommandError) return labels[error.kind];
  if (error instanceof JeviaProtocolError) return error.kind;
  return "unknown";
}
void describe;
