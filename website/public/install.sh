#!/bin/sh
set -eu

# Keep the installer inside a function so a partial download cannot start it.
main() {
  JEVIA_VERSION='0.1.0'

  if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' 'Jevia requires Rust 1.92+ and Cargo. Install them from https://rustup.rs, then run this command again.' >&2
    return 1
  fi

  printf 'Installing Jevia %s from crates.io (requires Rust 1.92+)...\n' "$JEVIA_VERSION"
  cargo install --locked --version "$JEVIA_VERSION" jevia || return "$?"
  printf '%s\n' 'Jevia installed. Run `jevia init` to get started.'
}

main "$@"
