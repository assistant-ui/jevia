"use client";

import { Check, Copy } from "lucide-react";
import { useEffect, useState } from "react";

interface CopyButtonBaseProps {
  label: string;
  format?: "command" | "markdown";
  kind?: "command" | "code";
  disabled?: boolean;
}

type CopyButtonProps = CopyButtonBaseProps &
  ({ value: string; valueUrl?: never } | { value?: never; valueUrl: string });

export function CopyButton(props: CopyButtonProps) {
  const {
    label,
    format = "command",
    kind = "command",
    disabled = false,
  } = props;
  const [state, setState] = useState<"idle" | "copied" | "error">("idle");

  useEffect(() => {
    if (state === "idle") return;
    const timeout = window.setTimeout(() => setState("idle"), 1800);
    return () => window.clearTimeout(timeout);
  }, [state]);

  async function copy() {
    try {
      const copyValue =
        props.valueUrl !== undefined
          ? await fetch(props.valueUrl).then((response) => {
              if (!response.ok) throw new Error("Could not load copy source");
              return response.text();
            })
          : props.value;

      await navigator.clipboard.writeText(copyValue);
      setState("copied");
    } catch {
      setState("error");
    }
  }

  const text =
    state === "copied"
      ? "COPIED"
      : state === "error"
        ? "RETRY"
        : format === "markdown"
          ? "COPY .MD"
          : "COPY";
  const description =
    format === "markdown" ? `${label} as Markdown` : `${label} ${kind}`;

  return (
    <>
      <button
        className="copy-button"
        data-format={format}
        type="button"
        disabled={disabled}
        onClick={copy}
        aria-label={`${text}: ${label}`}
        title={`Copy ${description}`}
      >
        {state === "copied" ? (
          <Check size={14} strokeWidth={1.8} aria-hidden="true" />
        ) : (
          <Copy size={14} strokeWidth={1.8} aria-hidden="true" />
        )}
        <span>{text}</span>
      </button>
      <span className="sr-only" aria-live="polite">
        {state === "copied"
          ? `Copied ${description}`
          : state === "error"
            ? `Could not copy ${description}. Try again.`
            : ""}
      </span>
    </>
  );
}
