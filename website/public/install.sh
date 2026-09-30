#!/bin/sh
set -eu

JEVIA_VERSION="${JEVIA_VERSION:-0.1.7}"
JEVIA_REPOSITORY='assistant-ui/jevia'
JEVIA_INSTALL_DIR="${JEVIA_INSTALL_DIR:-${HOME:?HOME must be set}/.local/bin}"
JEVIA_DOWNLOAD_BASE_URL="${JEVIA_DOWNLOAD_BASE_URL:-https://github.com/${JEVIA_REPOSITORY}/releases/download/v${JEVIA_VERSION}}"

temporary_directory=''
temporary_destination=''

cleanup() {
  if [ -n "$temporary_destination" ]; then
    rm -f "$temporary_destination"
  fi
  if [ -n "$temporary_directory" ] && [ -d "$temporary_directory" ]; then
    rm -rf "$temporary_directory"
  fi
}

fail() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "Jevia installation requires $1"
}

detect_target() {
  operating_system=$(uname -s)
  architecture=$(uname -m)

  case "$operating_system" in
    Darwin) platform='apple-darwin' ;;
    Linux) platform='unknown-linux-musl' ;;
    *) fail "unsupported operating system: $operating_system" ;;
  esac

  case "$architecture" in
    x86_64 | amd64) architecture='x86_64' ;;
    arm64 | aarch64) architecture='aarch64' ;;
    *) fail "unsupported architecture: $architecture" ;;
  esac

  printf '%s-%s\n' "$architecture" "$platform"
}

verify_checksum() {
  asset=$1
  checksum=$2

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$temporary_directory" && sha256sum -c "${checksum##*/}")
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$temporary_directory" && shasum -a 256 -c "${checksum##*/}")
  else
    fail 'Jevia installation requires sha256sum or shasum to verify the download'
  fi

  [ -s "$asset" ] || fail 'downloaded Jevia binary is empty'
}

main() {
  require_command curl
  require_command uname
  require_command mktemp
  require_command mkdir
  require_command chmod
  require_command cp
  require_command mv

  target=$(detect_target)
  asset_name="jevia-v${JEVIA_VERSION}-${target}"
  download_url="${JEVIA_DOWNLOAD_BASE_URL}/${asset_name}"

  temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/jevia-install.XXXXXX")
  asset_path="${temporary_directory}/${asset_name}"
  checksum_path="${asset_path}.sha256"

  printf 'Downloading Jevia %s for %s...\n' "$JEVIA_VERSION" "$target"
  curl --proto '=https' --tlsv1.2 -fsSL "$download_url" -o "$asset_path"
  curl --proto '=https' --tlsv1.2 -fsSL "${download_url}.sha256" -o "$checksum_path"
  verify_checksum "$asset_path" "$checksum_path"

  mkdir -p "$JEVIA_INSTALL_DIR"
  [ -w "$JEVIA_INSTALL_DIR" ] || fail "install directory is not writable: $JEVIA_INSTALL_DIR"

  temporary_destination="${JEVIA_INSTALL_DIR}/.jevia.install.$$"
  cp "$asset_path" "$temporary_destination"
  chmod 755 "$temporary_destination"
  mv "$temporary_destination" "${JEVIA_INSTALL_DIR}/jevia"
  temporary_destination=''

  printf 'Installed Jevia %s to %s/jevia\n' "$JEVIA_VERSION" "$JEVIA_INSTALL_DIR"
  case ":${PATH:-}:" in
    *":${JEVIA_INSTALL_DIR}:"*) ;;
    *)
      printf 'Add %s to PATH, then run `jevia init`.\n' "$JEVIA_INSTALL_DIR"
      exit 0
      ;;
  esac
  printf '%s\n' 'Run `jevia init` to get started.'
}

trap cleanup EXIT HUP INT TERM
main "$@"
