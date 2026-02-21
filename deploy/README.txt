METAVERSE PORTABLE DEPLOYMENT
==============================

Copy this entire deploy/ folder to remote machines for testing.

SCRIPTS:
--------
./update_binaries.sh      # (dev machine) Build and copy latest binaries
./run_client.sh [name]    # Run client (default: bob, or alice, charlie, etc.)
./run_relay.sh [port]     # Run dedicated relay server (default port: 4001)

USAGE:
------
On dev machine (after making changes):
  ./deploy/update_binaries.sh

To copy to remote machine:
  scp -r deploy/ user@remote:/path/to/metaverse/

On remote machine:
  cd /path/to/metaverse/deploy
  ./run_client.sh           # Run as bob
  ./run_client.sh alice     # Run as alice
  ./run_relay.sh            # Run relay server

CONTROLS:
---------
  WASD     - Move
  Space    - Jump/Fly up  
  F        - Toggle walk/fly mode
  E        - Dig block
  Q        - Place block
  T        - Chat
  Mouse    - Look around
  Escape   - Exit

MESH RELAY ARCHITECTURE:
------------------------
Every client is now BOTH:
  • A relay client (can use other peers as relays)
  • A relay server (can be a relay for other peers)

This creates a true P2P mesh where any peer can help any other peer
connect through NAT. Dedicated relays are just "always-on peers".

Watch for these messages:
  ✅ [RELAY SERVER] Reservation accepted for peer: ...
  🔄 [RELAY SERVER] Circuit: ... → ...

This proves your client is helping others connect!
  
