#!/usr/bin/env bash
# Download the latest mverse release binaries from GitHub
# Usage: ./scripts/install.sh [--relay-only] [--client-only] [--dir /path/to/install]

set -e

REPO="PaddyOhFurnature/mverse"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
RELAY_ONLY=false
CLIENT_ONLY=false

for arg in "$@"; do
  case $arg in
    --relay-only)  RELAY_ONLY=true ;;
    --client-only) CLIENT_ONLY=true ;;
    --dir=*)       INSTALL_DIR="${arg#*=}" ;;
  esac
done

mkdir -p "$INSTALL_DIR"

if ! command -v gh &>/dev/null; then
  echo "Error: 'gh' (GitHub CLI) is required. Install from https://cli.github.com"
  exit 1
fi

echo "Downloading latest mverse release from github.com/$REPO ..."
echo "Install dir: $INSTALL_DIR"
echo ""

cd "$INSTALL_DIR"

if [ "$RELAY_ONLY" = false ]; then
  echo "Downloading metaworld_alpha (client)..."
  gh release download --repo "$REPO" --pattern "metaworld_alpha" --clobber
  chmod +x metaworld_alpha
  echo "  ✓ metaworld_alpha"
fi

if [ "$CLIENT_ONLY" = false ]; then
  echo "Downloading metaverse-relay..."
  gh release download --repo "$REPO" --pattern "metaverse-relay" --clobber
  chmod +x metaverse-relay
  echo "  ✓ metaverse-relay"

  echo "Downloading metaverse-server..."
  gh release download --repo "$REPO" --pattern "metaverse-server" --clobber
  chmod +x metaverse-server
  echo "  ✓ metaverse-server"
fi

echo ""
echo "Done. Installed to $INSTALL_DIR"
echo ""
echo "Run the client:  $INSTALL_DIR/metaworld_alpha"
echo "Run the relay:   $INSTALL_DIR/metaverse-relay --port 4001"
echo "Run the server:  $INSTALL_DIR/metaverse-server"
