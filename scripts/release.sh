#!/usr/bin/env bash
# Build and publish a new mverse release to GitHub
# Usage: ./scripts/release.sh v0.1.0 "Release notes here"
#
# IMPORTANT: version is bumped in Cargo.toml BEFORE building so that
# env!("CARGO_PKG_VERSION") in the binary matches the release tag.

set -e

VERSION="${1:?Usage: $0 <version> [notes]}"
NOTES="${2:-Release $VERSION}"
REPO="PaddyOhFurnature/mverse"
SEMVER="${VERSION#v}"   # strip leading 'v' for Cargo.toml

if ! command -v gh &>/dev/null; then
  echo "Error: 'gh' (GitHub CLI) is required."
  exit 1
fi

# ── 1. Bump version in Cargo.toml BEFORE building ────────────────────────────
echo "Bumping version to $SEMVER in Cargo.toml..."
CURRENT=$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
if [ "$CURRENT" = "$SEMVER" ]; then
  echo "  Already at $SEMVER — skipping bump"
else
  sed -i "s/^version = \"$CURRENT\"/version = \"$SEMVER\"/" Cargo.toml
  echo "  $CURRENT → $SEMVER"
fi

# ── 2. Build (version is now baked in via env!("CARGO_PKG_VERSION")) ─────────
echo ""
echo "Building release binaries (version $SEMVER)..."
RUST_MIN_STACK=16777216 cargo build --release \
  --bin metaverse-relay \
  --bin metaverse-server --features jemalloc \
  --example metaworld_alpha \
  2>&1 | grep -E "^error|Compiling metaverse|Finished"

# ── 3. Copy to bin/ ───────────────────────────────────────────────────────────
echo ""
echo "Copying to bin/..."
mkdir -p bin
cp target/release/metaverse-relay           bin/metaverse-relay
cp target/release/metaverse-server          bin/metaverse-server
cp target/release/examples/metaworld_alpha  bin/metaworld_alpha
chmod +x bin/metaverse-relay bin/metaverse-server bin/metaworld_alpha

# ── 4. Commit + tag + push ────────────────────────────────────────────────────
echo ""
echo "Committing and tagging $VERSION..."
git add Cargo.toml Cargo.lock bin/
git commit -m "release: $VERSION" || echo "  (nothing new to commit)"
git tag "$VERSION" || echo "  (tag already exists)"
git push
git push origin "$VERSION" || true

# ── 5. Create GitHub release ──────────────────────────────────────────────────
echo ""
echo "Creating GitHub release $VERSION..."
gh release create "$VERSION" \
  --repo "$REPO" \
  --title "$VERSION" \
  --notes "$NOTES" \
  bin/metaworld_alpha \
  bin/metaverse-relay \
  bin/metaverse-server

echo ""
echo "✓ Release $VERSION published"
echo "  https://github.com/$REPO/releases/tag/$VERSION"
echo ""
echo "Install on any machine with:"
echo "  curl -sSf https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | bash"
