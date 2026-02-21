#!/bin/bash
# Run metaworld_alpha client
# Usage: ./run_client.sh [identity_name]
#
# Examples:
#   ./run_client.sh           # Uses bob identity (default)
#   ./run_client.sh alice     # Uses alice identity
#   ./run_client.sh charlie   # Uses charlie identity

# Navigate to deploy directory
cd "$(dirname "$0")"

# Default to bob if no identity specified
IDENTITY="${1:-bob}"

# Check if identity file exists
IDENTITY_FILE="identities/${IDENTITY}.key"
if [ ! -f "$IDENTITY_FILE" ]; then
    echo "❌ Identity file not found: $IDENTITY_FILE"
    echo ""
    echo "Available identities:"
    ls identities/*.key 2>/dev/null | sed 's/identities\///;s/\.key$//' | sed 's/^/  • /'
    exit 1
fi

echo "═══════════════════════════════════════════════════════════"
echo "  METAVERSE CLIENT"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Identity: ${IDENTITY}"
echo "File:     ${IDENTITY_FILE}"
echo ""
echo "Controls:"
echo "  WASD     - Move"
echo "  Space    - Jump/Fly up"
echo "  F        - Toggle walk/fly mode"
echo "  E        - Dig block"
echo "  Q        - Place block"
echo "  T        - Chat (type message, Enter to send)"
echo "  Mouse    - Look around"
echo "  Escape   - Exit"
echo ""
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Starting client..."
echo ""

# Set identity and run
export METAVERSE_IDENTITY_FILE="$IDENTITY_FILE"

# Point to data directory containing srtm-global.tif and elevation_cache
# On dev machine: repo root. On remote: set METAVERSE_DATA_DIR before running.
if [ -z "$METAVERSE_DATA_DIR" ]; then
    # Default: look for srtm file relative to script location
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    if [ -f "$SCRIPT_DIR/../srtm-global.tif" ]; then
        export METAVERSE_DATA_DIR="$SCRIPT_DIR/.."
    elif [ -f "$SCRIPT_DIR/srtm-global.tif" ]; then
        export METAVERSE_DATA_DIR="$SCRIPT_DIR"
    fi
fi

if [ -n "$METAVERSE_DATA_DIR" ]; then
    echo "📁 Data dir: $METAVERSE_DATA_DIR"
else
    echo "⚠️  No SRTM terrain file found - terrain will be flat"
    echo "   Set METAVERSE_DATA_DIR to folder containing srtm-global.tif"
    echo "   or copy srtm-global.tif to the deploy folder"
fi

./bin/metaworld_alpha
