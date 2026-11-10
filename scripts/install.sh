#!/bin/sh
set -e

# quad install script
# https://github.com/kushvinth/quad

REPO="kushvinth/quad"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

main() {
  printf "  › Detecting platform...\n"
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"

  case "$ARCH" in
    x86_64|amd64)
      ARCH="x86_64"
      ;;
    aarch64|arm64)
      ARCH="aarch64"
      ;;
    *)
      printf "  ✖ Unsupported architecture: %s\n" "$ARCH" >&2
      exit 1
      ;;
  esac

  case "$OS" in
    darwin)
      ASSET="quad-macos-universal.tar.gz"
      ;;
    linux)
      if [ "$ARCH" = "aarch64" ]; then
        ASSET="quad-linux-aarch64.tar.gz"
      else
        ASSET="quad-linux-x86_64.tar.gz"
      fi
      ;;
    *)
      printf "  ✖ Unsupported operating system: %s\n" "$OS" >&2
      printf "  › For Windows, please download pre-built binaries or run: cargo install quad-mcp\n" >&2
      exit 1
      ;;
  esac

  mkdir -p "$INSTALL_DIR"
  TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'quad-install')"
  trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

  DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
  printf "  › Downloading quad from %s...\n" "$DOWNLOAD_URL"

  if curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ASSET}" 2>/dev/null; then
    tar -xzf "${TMP_DIR}/${ASSET}" -C "${TMP_DIR}"
    mv "${TMP_DIR}/quad" "${INSTALL_DIR}/quad"
    chmod +x "${INSTALL_DIR}/quad"
    printf "  ✔ quad installed successfully to %s/quad\n\n" "$INSTALL_DIR"
  else
    printf "  • Release binary not yet available; falling back to cargo install...\n"
    if command -v cargo >/dev/null 2>&1; then
      cargo install quad-mcp
      printf "  ✔ quad-mcp installed via cargo!\n\n"
    else
      printf "  ✖ Failed to download release asset and cargo is not installed.\n" >&2
      printf "    Please visit https://github.com/%s/releases to download manually.\n" "$REPO" >&2
      exit 1
    fi
  fi

  # Path check
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
      printf "  ! Notice: %s is not in your \$PATH.\n" "$INSTALL_DIR"
      printf "    Add it to your shell configuration:\n"
      printf "      export PATH=\"%s:\$PATH\"\n\n" "$INSTALL_DIR"
      ;;
  esac

  printf "  Next step:\n"
  printf "    quad login && quad setup\n\n"
}

main "$@"
