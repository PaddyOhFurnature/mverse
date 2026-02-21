#!/bin/bash
cd "$(dirname "$0")"
export METAVERSE_IDENTITY_FILE=./identities/bob.key
./bin/phase1_multiplayer
