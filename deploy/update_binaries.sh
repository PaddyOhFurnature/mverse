#!/bin/bash
# Deploy script - Build and copy binaries to deploy/bin
# Run this after making changes to prepare for remote testing

set -e  # Exit on any error

echo "═══════════════════════════════════════════════════════════"
echo "  METAVERSE DEPLOYMENT SCRIPT"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Navigate to project root (in case script is run from elsewhere)
cd "$(dirname "$0")/.."

echo "📦 Building release binaries..."
echo ""

# Build metaworld_alpha (main client launcher)
echo "  ⚙️  Building metaworld_alpha..."
cargo build --release --example metaworld_alpha --quiet
if [ $? -eq 0 ]; then
    echo "  ✅ metaworld_alpha built successfully"
else
    echo "  ❌ Failed to build metaworld_alpha"
    exit 1
fi

# Build metaverse_relay (dedicated relay server)
echo "  ⚙️  Building metaverse_relay..."
cargo build --release --bin metaverse-relay --quiet
if [ $? -eq 0 ]; then
    echo "  ✅ metaverse_relay built successfully"
else
    echo "  ❌ Failed to build metaverse_relay"
    exit 1
fi

echo ""
echo "📋 Copying binaries to deploy/bin/..."
echo ""

# Ensure deploy/bin exists
mkdir -p deploy/bin

# Copy metaworld_alpha
cp target/release/examples/metaworld_alpha deploy/bin/
SIZE_ALPHA=$(du -h deploy/bin/metaworld_alpha | cut -f1)
echo "  ✅ metaworld_alpha → deploy/bin/ (${SIZE_ALPHA})"

# Copy metaverse_relay
cp target/release/metaverse-relay deploy/bin/
SIZE_RELAY=$(du -h deploy/bin/metaverse-relay | cut -f1)
echo "  ✅ metaverse_relay → deploy/bin/ (${SIZE_RELAY})"

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  DEPLOYMENT READY"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Binaries in deploy/bin/:"
ls -lh deploy/bin/ | grep -v ^total | awk '{printf "  • %-25s %6s\n", $9, $5}'
echo ""
echo "📦 Ready to copy deploy/ folder to remote machine"
echo ""
echo "Next steps:"
echo "  1. Copy deploy/ folder to remote machine:"
echo "     scp -r deploy/ user@remote:/path/to/metaverse/"
echo ""
echo "  2. On remote machine, run:"
echo "     cd /path/to/metaverse/deploy"
echo "     ./run_client.sh"
echo ""
echo "═══════════════════════════════════════════════════════════"
