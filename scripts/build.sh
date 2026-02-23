#!/usr/bin/env bash
# Build all metaverse binaries and place them in bin/ at the project root.
# Usage:
#   ./scripts/build.sh           — debug build (fast)
#   ./scripts/build.sh --release — optimised release build

set -e

cd "$(dirname "$0")/.."

PROFILE="debug"
CARGO_FLAGS=""

for arg in "$@"; do
  case $arg in
    --release) PROFILE="release"; CARGO_FLAGS="--release" ;;
  esac
done

echo "Building metaverse binaries ($PROFILE)..."
echo ""

cargo build $CARGO_FLAGS \
  --bin metaverse-relay \
  --bin metaverse-server \
  --example metaworld_alpha \
  2>&1 | grep -E "^error|Compiling metaverse|Finished"

echo ""
echo "Copying to bin/..."

mkdir -p bin

cp target/$PROFILE/metaverse-relay    bin/metaverse-relay
cp target/$PROFILE/metaverse-server   bin/metaverse-server
cp target/$PROFILE/examples/metaworld_alpha bin/metaworld_alpha

chmod +x bin/metaverse-relay bin/metaverse-server bin/metaworld_alpha

echo ""
echo "Done:"
ls -lh bin/
echo ""
echo "Run the client:  ./bin/metaworld_alpha"
echo "Run the relay:   ./bin/metaverse-relay --port 4001"
echo "Run the server:  ./bin/metaverse-server"
