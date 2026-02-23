#!/usr/bin/env bash
# Build and publish a new mverse release to GitHub
# Usage: ./scripts/release.sh v0.1.0 "Release notes here"

set -e

VERSION="${1:?Usage: $0 <version> [notes]}"
NOTES="${2:-Release $VERSION}"
REPO="PaddyOhFurnature/mverse"

if ! command -v gh &>/dev/null; then
  echo "Error: 'gh' (GitHub CLI) is required."
  exit 1
fi

echo "Building release binaries..."
cargo build --release 2>&1 | grep -E "Compiling|Finished|error"

echo ""
echo "Creating GitHub release $VERSION..."
gh release create "$VERSION" \
  --repo "$REPO" \
  --title "$VERSION" \
  --notes "$NOTES"

echo "Uploading binaries..."
gh release upload "$VERSION" \
  target/release/metaworld_alpha \
  target/release/metaverse-relay \
  --repo "$REPO" \
  --clobber

echo ""
echo "✓ Release $VERSION published"
echo "  https://github.com/$REPO/releases/tag/$VERSION"
echo ""
echo "Install on any machine with:"
echo "  curl -sSf https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | bash"
