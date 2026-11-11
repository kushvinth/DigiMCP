#!/bin/sh
set -e

# DigiMCP install script
# https://github.com/kushvinth/DigiMCP

REPO="kushvinth/DigiMCP"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# ANSI Colors
BOLD="\033[1m"
GREEN="\033[32m"
BLUE="\033[34m"
CYAN="\033[36m"
RED="\033[31m"
YELLOW="\033[33m"
GRAY="\033[90m"
RESET="\033[0m"

main() {
  printf "\n"
  printf "${CYAN}  ╭─────────────────────────────────────────────────────────────────╮${RESET}\n"
  printf "${CYAN}  │  ${BOLD}DigiMCP — Student Intelligence MCP Server & Portal CLI${RESET}${CYAN}         │${RESET}\n"
  printf "${CYAN}  │  ${GRAY}Academics · Materials · Writes · In-Tree RTK Token Saver${RESET}${CYAN}       │${RESET}\n"
  printf "${CYAN}  ╰─────────────────────────────────────────────────────────────────╯${RESET}\n\n"

  printf "  ${BLUE}›${RESET} Detecting platform and architecture...\n"
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
      printf "  ${RED}✖ Unsupported architecture: %s${RESET}\n" "$ARCH" >&2
      exit 1
      ;;
  esac

  case "$OS" in
    darwin)
      ASSET="digimcp-macos-universal.tar.gz"
      ;;
    linux)
      if [ "$ARCH" = "aarch64" ]; then
        ASSET="digimcp-linux-aarch64.tar.gz"
      else
        ASSET="digimcp-linux-x86_64.tar.gz"
      fi
      ;;
    *)
      printf "  ${RED}✖ Unsupported operating system: %s${RESET}\n" "$OS" >&2
      printf "  ${BLUE}›${RESET} For Windows, download pre-built binaries from GitHub Releases or run:\n" >&2
      printf "      cargo install digimcp\n" >&2
      exit 1
      ;;
  esac

  mkdir -p "$INSTALL_DIR"
  TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'digimcp-install')"
  trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

  DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
  printf "  ${BLUE}›${RESET} Downloading DigiMCP from GitHub Releases...\n"
  printf "    ${GRAY}%s${RESET}\n" "$DOWNLOAD_URL"

  if curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ASSET}" 2>/dev/null; then
    tar -xzf "${TMP_DIR}/${ASSET}" -C "${TMP_DIR}"
    mv "${TMP_DIR}/digi" "${INSTALL_DIR}/digi"
    chmod +x "${INSTALL_DIR}/digi"
    ln -sf "${INSTALL_DIR}/digi" "${INSTALL_DIR}/digii" 2>/dev/null || true
    ln -sf "${INSTALL_DIR}/digi" "${INSTALL_DIR}/digimcp" 2>/dev/null || true
    printf "  ${GREEN}✔${RESET} Binary installed to ${BOLD}%s/digi${RESET} (with ${CYAN}digii${RESET} and ${CYAN}digimcp${RESET} aliases)\n\n" "$INSTALL_DIR"
  else
    printf "  ${YELLOW}•${RESET} Pre-built release binary not yet published; building via cargo...\n"
    if command -v cargo >/dev/null 2>&1; then
      cargo install digimcp
      printf "  ${GREEN}✔${RESET} digimcp installed via cargo\n\n"
    else
      printf "  ${RED}✖${RESET} Pre-built binary not found and cargo is not available.\n" >&2
      printf "    Please visit https://github.com/%s/releases to download manually.\n\n" "$REPO" >&2
      exit 1
    fi
  fi

  # Path verification
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
      printf "  ${YELLOW}!${RESET} Notice: ${BOLD}%s${RESET} is not currently in your \$PATH.\n" "$INSTALL_DIR"
      printf "    Add it to your shell profile (~/.zshrc, ~/.bashrc, or ~/.config/fish/config.fish):\n"
      printf "      ${CYAN}export PATH=\"%s:\$PATH\"${RESET}\n\n" "$INSTALL_DIR"
      ;;
  esac

  printf "  ${GREEN}${BOLD}DigiMCP is ready.${RESET}\n\n"
  printf "  ${BOLD}Quick start:${RESET}\n"
  printf "    ${CYAN}1. digi login${RESET}              Connect your DigiiCampus (CollPoll) account\n"
  printf "    ${CYAN}2. digi setup${RESET}              Auto-wire into Claude Code, Codex, Cursor & Gemini\n"
  printf "    ${CYAN}3. digi dashboard${RESET}          View live schedule, token health & tools\n\n"
  printf "  ${BOLD}Prompt ideas for your agents:${RESET}\n"
  printf "    ${GRAY}• \"What assignments and quizzes are due this week?\"${RESET}\n"
  printf "    ${GRAY}• \"Check my attendance across all courses and show bunks left\"${RESET}\n"
  printf "    ${GRAY}• \"List course materials and fetch signed download links for Module 2\"${RESET}\n\n"
}

main "$@"
