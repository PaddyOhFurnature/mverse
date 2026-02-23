#!/usr/bin/env bash
# Download the latest mverse release binaries from GitHub
# Usage: ./scripts/install.sh [--relay-only] [--client-only] [--server-only] [--dir /path/to/install]
# Requires: curl, jq (or curl only — falls back to hardcoded latest tag)

set -e

REPO="PaddyOhFurnature/mverse"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
RELAY_ONLY=false
CLIENT_ONLY=false
SERVER_ONLY=false

for arg in "$@"; do
  case $arg in
    --relay-only)  RELAY_ONLY=true ;;
    --client-only) CLIENT_ONLY=true ;;
    --server-only) SERVER_ONLY=true ;;
    --dir=*)       INSTALL_DIR="${arg#*=}" ;;
  esac
done

mkdir -p "$INSTALL_DIR"

if ! command -v curl &>/dev/null; then
  echo "Error: 'curl' is required but not installed."
  exit 1
fi

# Resolve latest release tag via GitHub API (no auth required for public repos)
echo "Resolving latest release..."
if command -v jq &>/dev/null; then
  TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | jq -r '.tag_name')
else
  TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
fi

if [ -z "$TAG" ] || [ "$TAG" = "null" ]; then
  echo "Error: could not determine latest release tag from GitHub API."
  exit 1
fi

BASE_URL="https://github.com/$REPO/releases/download/$TAG"

echo "Downloading mverse $TAG from github.com/$REPO ..."
echo "Install dir: $INSTALL_DIR"
echo ""

dl() {
  local name="$1"
  echo "Downloading $name..."
  curl -fSL --progress-bar -o "$INSTALL_DIR/$name" "$BASE_URL/$name"
  chmod +x "$INSTALL_DIR/$name"
  echo "  ✓ $name"
}

if [ "$RELAY_ONLY" = false ] && [ "$SERVER_ONLY" = false ]; then
  dl "metaworld_alpha"
fi

if [ "$CLIENT_ONLY" = false ]; then
  if [ "$SERVER_ONLY" = false ]; then
    dl "metaverse-relay"
  fi
  dl "metaverse-server"
fi

echo ""
echo "Done. Installed to $INSTALL_DIR"
echo ""
if [ "$CLIENT_ONLY" = false ] && [ "$SERVER_ONLY" = false ]; then
  echo "Run the client:  $INSTALL_DIR/metaworld_alpha"
fi
echo "Run the relay:   $INSTALL_DIR/metaverse-relay --port 4001"
echo "Run the server:  $INSTALL_DIR/metaverse-server"
