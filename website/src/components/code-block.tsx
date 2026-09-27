"use client";

import { Highlight, type PrismTheme } from "prism-react-renderer";

import { CopyButton } from "./copy-button";

interface CodeBlockProps {
  code: string;
  label: string;
  language?: "typescript";
}

const CODE_THEME: PrismTheme = {
  plain: {
    color: "#b8b8b8",
    backgroundColor: "transparent",
  },
  styles: [
    {
      types: ["comment", "prolog", "doctype", "cdata"],
      style: { color: "#666666", fontStyle: "italic", opacity: 0.8 },
    },
    {
      types: ["keyword", "atrule"],
      style: { color: "#f2f2f2", fontWeight: "500" },
    },
    {
      types: ["string", "char", "attr-value", "regex"],
      style: { color: "#d4d4d4" },
    },
    {
      types: ["function", "method", "property-access"],
      style: { color: "#e2e2e2" },
    },
    {
      types: ["class-name", "builtin", "constant"],
      style: { color: "#c4c4c4" },
    },
    {
      types: ["number", "boolean"],
      style: { color: "#ababab" },
    },
    {
      types: ["operator", "punctuation"],
      style: { color: "#919191", opacity: 0.78 },
    },
  ],
};

export function CodeBlock({ code, label, language = "typescript" }: CodeBlockProps) {
  return (
    <div className="docs-code-block">
      <div className="docs-code-meta">
        <span>{label}</span>
        <div className="docs-code-actions">
          <span aria-hidden="true">TS</span>
          <CopyButton value={code} label={label} kind="code" />
        </div>
      </div>
      <Highlight theme={CODE_THEME} code={code} language={language}>
        {({ className, style, tokens, getLineProps, getTokenProps }) => (
          <pre
            className={className}
            style={{ ...style, background: "transparent" }}
            tabIndex={0}
            aria-label={label + " code"}
          >
            <code>
              {tokens.map((line, lineIndex) => {
                const lineProps = getLineProps({ line });

                return (
                  <span
                    {...lineProps}
                    className={`${lineProps.className} docs-code-line`}
                    key={lineIndex}
                  >
                    {line.map((token, tokenIndex) => (
                      <span {...getTokenProps({ token })} key={tokenIndex} />
                    ))}
                  </span>
                );
              })}
            </code>
          </pre>
        )}
      </Highlight>
    </div>
  );
}
