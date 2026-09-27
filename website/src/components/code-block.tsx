"use client";

import { CopyButton } from "./copy-button";

interface CodeBlockProps {
  code: string;
  label: string;
  language?: string;
}

export function CodeBlock({ code, label, language = "ts" }: CodeBlockProps) {
  return (
    <div className="docs-code-block">
      <div className="docs-code-meta">
        <span>{label}</span>
        <div className="docs-code-actions">
          <span aria-hidden="true">{language}</span>
          <CopyButton value={code} label={label} kind="code" />
        </div>
      </div>
      <pre tabIndex={0} aria-label={label + " code"}>
        <code>{code}</code>
      </pre>
    </div>
  );
}
