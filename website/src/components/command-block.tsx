"use client";

import { tokenizeShellCommand } from "../lib/shell-tokens";
import { CopyButton } from "./copy-button";

interface CommandBlockProps {
  command: string;
  label: string;
  compact?: boolean;
  minimal?: boolean;
  disabled?: boolean;
  markdown?: string;
}

export function CommandBlock({
  command,
  label,
  compact = false,
  minimal = false,
  disabled = false,
  markdown,
}: CommandBlockProps) {
  const hasMarkdown = markdown !== undefined;

  return (
    <div
      className="command-block"
      data-compact={compact || undefined}
      data-markdown={hasMarkdown || undefined}
    >
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
        <code>
          {disabled
            ? command
            : tokenizeShellCommand(command).map((token) => (
                <span key={token.start} className={`shell-${token.kind}`}>
                  {token.text}
                </span>
              ))}
        </code>
        <div className="command-actions">
          <CopyButton value={command} label={label} disabled={disabled} />
          {hasMarkdown && (
            <CopyButton
              value={markdown ?? ""}
              label="Jevia setup instructions"
              format="markdown"
              disabled={disabled || !markdown}
            />
          )}
        </div>
      </div>
    </div>
  );
}
