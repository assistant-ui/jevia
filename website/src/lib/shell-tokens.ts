type ShellTokenKind = "plain" | "command" | "option" | "operator" | "variable" | "string";

// A small display tokenizer for the site's single-line shell examples, not a shell parser.
// Keep every character intact; copying always uses the original command string.
export function tokenizeShellCommand(command: string) {
  const pattern = /"(?:\\.|[^"\\])*"|'[^']*'|[|&;=]+|\s+|[^\s"'|&;=]+|./g;
  let expectCommand = true;

  return Array.from(command.matchAll(pattern), (match) => {
    const text = match[0];
    const start = match.index;
    let kind: ShellTokenKind = "plain";

    if (/^\s+$/.test(text)) {
      return { text, start, kind };
    }

    if (/^[|&;=]+$/.test(text)) {
      kind = "operator";
      if (text !== "=") expectCommand = true;
    } else if (/^["']/.test(text)) {
      kind = "string";
      expectCommand = false;
    } else if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(text) && command[start + text.length] === "=") {
      kind = "variable";
    } else if (expectCommand) {
      kind = "command";
      expectCommand = false;
    } else if (text.startsWith("-")) {
      kind = "option";
    }

    return { text, start, kind };
  });
}
