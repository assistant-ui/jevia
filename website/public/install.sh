#!/bin/sh
set -eu

# Keep the installer inside a function so a partial download cannot start it.
main() {
  if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' 'Jevia requires Rust 1.92+ and Cargo. Install them from https://rustup.rs, then run this command again.' >&2
    return 1
  fi

  printf '%s\n' 'Installing Jevia from assistant-ui/jevia (requires Rust 1.92+)...'
  cargo install --locked --git https://github.com/assistant-ui/jevia jevia || return "$?"
  printf '%s\n' 'Jevia installed. Run `jevia init` to get started.'
}

main "$@"
