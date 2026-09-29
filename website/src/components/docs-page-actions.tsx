import { FileText } from "lucide-react";

import { CopyButton } from "./copy-button";

export function DocsPageActions() {
  return (
    <div className="docs-page-actions" aria-label="Page actions">
      <a
        className="docs-page-action"
        href="/api/docs-markdown"
        target="_blank"
        rel="noreferrer"
      >
        <FileText size={14} strokeWidth={1.7} aria-hidden="true" />
        <span>View .md</span>
      </a>
      <CopyButton
        valueUrl="/docs.md"
        label="Jevia documentation"
        format="markdown"
        kind="code"
      />
    </div>
  );
}
