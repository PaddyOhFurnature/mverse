#!/bin/bash
# Run dedicated relay server
# Usage: ./run_relay.sh [port] [external_ip]
#
# Examples:
#   ./run_relay.sh                    # Port 4001, auto-detect IP
#   ./run_relay.sh 4001               # Specific port, auto-detect IP
#   ./run_relay.sh 4001 49.182.84.9   # Specific port and external IP

# Navigate to deploy directory
cd "$(dirname "$0")"

# Default port
PORT="${1:-4001}"

# External IP (optional)
EXTERNAL_IP="${2:-}"

echo "═══════════════════════════════════════════════════════════"
echo "  METAVERSE RELAY SERVER"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Port: ${PORT}"
if [ -n "$EXTERNAL_IP" ]; then
    echo "External IP: ${EXTERNAL_IP}"
else
    echo "External IP: Auto-detect"
fi
echo ""
echo "Purpose:"
echo "  • Helps peers discover each other across the internet"
echo "  • Coordinates NAT hole-punching (DCUtR)"
echo "  • Provides fallback relay when direct P2P fails"
echo ""
echo "Note: This is NOT a game server - just a relay node."
echo "      Once peers establish direct P2P, relay is unused."
echo ""
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Starting relay server..."
echo ""

# Build command
CMD="./bin/metaverse-relay --port ${PORT}"

# Add external address if provided
if [ -n "$EXTERNAL_IP" ]; then
    CMD="$CMD --external-addr /ip4/${EXTERNAL_IP}/tcp/${PORT}"
fi

# Run relay server
$CMD
