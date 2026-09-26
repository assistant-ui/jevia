"use client";

import { Check, Copy } from "lucide-react";
import { useEffect, useState } from "react";

interface CommandBlockProps {
  command: string;
  label: string;
  compact?: boolean;
  minimal?: boolean;
  disabled?: boolean;
}

export function CommandBlock({
  command,
  label,
  compact = false,
  minimal = false,
  disabled = false,
}: CommandBlockProps) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");

  useEffect(() => {
    if (copyState === "idle") return;
    const timeout = window.setTimeout(() => setCopyState("idle"), 1800);
    return () => window.clearTimeout(timeout);
  }, [copyState]);

  async function copyCommand() {
    try {
      await navigator.clipboard.writeText(command);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  }

  const status =
    copyState === "copied" ? "Copied" : copyState === "error" ? "Copy failed" : "Copy";

  return (
    <div className="command-block" data-compact={compact || undefined}>
      {!minimal && (
        <div className="command-meta">
          <span>{label}</span>
          <span aria-hidden="true">bash</span>
        </div>
      )}
      <div className="command-line">
        <span className="command-prompt" aria-hidden="true">
          $
        </span>
        <code>{command}</code>
        <button
          className="copy-button"
          type="button"
          disabled={disabled}
          onClick={copyCommand}
          aria-label={`${status}: ${label}`}
          title={`${status} command`}
        >
          {copyState === "copied" ? (
            <Check size={14} strokeWidth={1.8} aria-hidden="true" />
          ) : (
            <Copy size={14} strokeWidth={1.8} aria-hidden="true" />
          )}
          <span>{status}</span>
        </button>
      </div>
      <span className="sr-only" aria-live="polite">
        {copyState === "copied"
          ? `${label} command copied`
          : copyState === "error"
            ? `Could not copy ${label} command`
            : ""}
      </span>
    </div>
  );
}
