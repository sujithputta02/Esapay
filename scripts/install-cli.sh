#!/usr/bin/env bash
# ==============================================================================
# ⚡ ESA (Executable State Architecture) — One-Line CLI Installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/sujithputta02/Esapay/main/scripts/install-cli.sh | bash
# ==============================================================================

set -e

BOLD="\033[1m"
GREEN="\033[32m"
YELLOW="\033[33m"
CYAN="\033[36m"
RED="\033[31m"
RESET="\033[0m"

echo -e "${CYAN}==============================================================================${RESET}"
echo -e "${BOLD}  ⚡ ESA (Executable State Architecture) — CLI Installer${RESET}"
echo -e "${CYAN}==============================================================================${RESET}"

# 1. Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    PLATFORM="apple-darwin"
    ;;
  Linux)
    PLATFORM="unknown-linux-gnu"
    ;;
  *)
    echo -e "${RED}❌ Unsupported operating system: $OS${RESET}"
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64)
    ARCH_TARGET="x86_64"
    ;;
  arm64|aarch64)
    ARCH_TARGET="aarch64"
    ;;
  *)
    echo -e "${RED}❌ Unsupported CPU architecture: $ARCH${RESET}"
    exit 1
    ;;
esac

TARGET="${ARCH_TARGET}-${PLATFORM}"
echo -e "  Detected environment: ${BOLD}${GREEN}${TARGET}${RESET}"

# 2. Determine installation directory
INSTALL_DIR="/usr/local/bin"
if [ ! -w "$INSTALL_DIR" ]; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

REPO="sujithputta02/Esapay"
VERSION="latest"
BINARY_NAME="esa"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# 3. Check for release binary or fallback to local cargo compile
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/esa-${TARGET}.tar.gz"

echo -e "  Installing to: ${CYAN}${INSTALL_DIR}/${BINARY_NAME}${RESET}"

# Check if script is run within an existing workspace
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" >/dev/null 2>&1 && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd)"

if [ -f "$WORKSPACE_ROOT/target/debug/esa" ]; then
  echo -e "  ${YELLOW}Found local build binary in workspace.${RESET}"
  cp "$WORKSPACE_ROOT/target/debug/esa" "${INSTALL_DIR}/${BINARY_NAME}"
  chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
elif [ -f "$WORKSPACE_ROOT/target/release/esa" ]; then
  echo -e "  ${YELLOW}Found local release binary in workspace.${RESET}"
  cp "$WORKSPACE_ROOT/target/release/esa" "${INSTALL_DIR}/${BINARY_NAME}"
  chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
elif command -v cargo >/dev/null 2>&1 && [ -f "$WORKSPACE_ROOT/Cargo.toml" ]; then
  echo -e "  ${YELLOW}Compiling ESA CLI via Cargo...${RESET}"
  cargo build --release --manifest-path "$WORKSPACE_ROOT/Cargo.toml" --bin esa
  cp "$WORKSPACE_ROOT/target/release/esa" "${INSTALL_DIR}/${BINARY_NAME}"
  chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
else
  echo -e "  Fetching binary from GitHub release..."
  if curl -sLf "$DOWNLOAD_URL" -o "$TMP_DIR/esa.tar.gz" 2>/dev/null; then
    tar -xzf "$TMP_DIR/esa.tar.gz" -C "$TMP_DIR"
    cp "$TMP_DIR/esa" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
  else
    echo -e "  ${YELLOW}Release asset not yet online. Building directly with cargo...${RESET}"
    cargo install --git "https://github.com/${REPO}.git" esa-cli --bin esa --root "${INSTALL_DIR}/.."
  fi
fi

# 4. Verify installation
echo -e "\n${GREEN}==============================================================================${RESET}"
echo -e "${BOLD}${GREEN}  ✅ ESA CLI installed successfully!${RESET}"
echo -e "${GREEN}==============================================================================${RESET}"

if command -v esa >/dev/null 2>&1; then
  echo -e "  Version:  $(${INSTALL_DIR}/esa --version)"
  echo -e "  Binary:   ${INSTALL_DIR}/esa"
else
  echo -e "  ${YELLOW}⚠️  Note: '${INSTALL_DIR}' is not in your \$PATH.${RESET}"
  echo -e "  Add it by running: ${BOLD}export PATH=\"\$PATH:${INSTALL_DIR}\"${RESET}"
fi

echo -e "\n${BOLD}Quickstart Commands:${RESET}"
echo -e "  ${CYAN}esa health${RESET}                      # Check backend & agent health"
echo -e "  ${CYAN}esa status${RESET}                      # Check live Executable State vitals"
echo -e "  ${CYAN}esa gateways${RESET}                    # Multi-gateway status (Razorpay, Stripe, etc.)"
echo -e "  ${CYAN}esa checkout --gateway auto${RESET}     # Run autonomous payment checkout"
echo -e "  ${CYAN}esa dashboard${RESET}                   # Launch Web Dashboard connected to your server"
echo -e ""
