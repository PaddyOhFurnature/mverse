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
cargo build --release \
  --bin metaverse-relay \
  --bin metaverse-server \
  --example metaworld_alpha \
  2>&1 | grep -E "^error|Compiling metaverse|Finished"

echo ""
echo "Copying to bin/..."
mkdir -p bin
cp target/release/metaverse-relay           bin/metaverse-relay
cp target/release/metaverse-server          bin/metaverse-server
cp target/release/examples/metaworld_alpha  bin/metaworld_alpha
chmod +x bin/metaverse-relay bin/metaverse-server bin/metaworld_alpha

echo ""
echo "Creating GitHub release $VERSION..."
gh release create "$VERSION" \
  --repo "$REPO" \
  --title "$VERSION" \
  --notes "$NOTES"

echo "Uploading binaries..."
gh release upload "$VERSION" \
  bin/metaworld_alpha \
  bin/metaverse-relay \
  bin/metaverse-server \
  --repo "$REPO" \
  --clobber

echo ""
echo "✓ Release $VERSION published"
echo "  https://github.com/$REPO/releases/tag/$VERSION"
echo ""
echo "Install on any machine with:"
echo "  curl -sSf https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | bash"
