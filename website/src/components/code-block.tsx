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
    color: "#d4d4d4",
    backgroundColor: "transparent",
  },
  styles: [
    {
      types: ["comment", "prolog", "doctype", "cdata"],
      style: { color: "#666666", fontStyle: "italic" },
    },
    {
      types: ["keyword", "atrule"],
      style: { color: "#c7a0dc" },
    },
    {
      types: ["string", "char", "attr-value", "regex"],
      style: { color: "#a8c98c" },
    },
    {
      types: ["function", "method", "property-access"],
      style: { color: "#d9c98e" },
    },
    {
      types: ["class-name", "builtin", "constant"],
      style: { color: "#8cbecf" },
    },
    {
      types: ["number", "boolean"],
      style: { color: "#b6a7f2" },
    },
    {
      types: ["operator", "punctuation"],
      style: { color: "#929292" },
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
