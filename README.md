# mverse — Planet-Scale P2P Metaverse

A voxel-based multiplayer world engine built on libp2p. No central servers — clients connect peer-to-peer through relay nodes, sync terrain and player state via gossipsub, and persist world changes locally.

---

## What it is

> Status: the current local-authoritative terrain and atlas workflow is still under active testing. Treat the source on `main` as experimental until the local COP30/GSHHG/OSM validation path has been rerun end-to-end.

**Current operator references:**

- `docs/PROJECT_REALIGNMENT_ROADMAP.md` — current cleanup/consolidation plan, working state, broken state, and rewrite phases
- `docs/CONTROL_ATLAS_WORKFLOW_RESET.md` — canonical control-atlas operational workflow
- `docs/INDEX.md` — documentation map and document authority guide

- **Voxel world** — Minecraft-style block editing on a procedurally generated planet-scale terrain
- **P2P multiplayer** — clients discover and connect to each other through relay nodes, no game server required
- **CGNAT/VPN friendly** — WebSocket transport punches through firewalls, Starlink, 4G NAT
- **Local-first** — your world data lives on your machine; relay nodes only forward traffic
- **Relay mesh** — multiple relays peer with each other for redundancy

---

## Components

| Binary | Description |
|--------|-------------|
| `atlas` | Control-atlas workflow helper — generate teleport/flyby inputs and merge cached control packs into `world_data/tiles.db` |
| `metaworld_alpha` | Game client — renders the world, lets you move/edit blocks, connects to peers |
| `metaverse-relay` | Relay node — routes P2P traffic, lightweight, run headless |
| `metaverse-server` | Full server — world data storage, DHT, content hosting |

---

## Quick start

### Download (latest release)
```bash
gh release download --repo PaddyOhFurnature/mverse --pattern '*' --dir ./mverse-bin
```
Or grab from: https://github.com/PaddyOhFurnature/mverse/releases/latest

### Or build from source
```bash
git clone https://github.com/PaddyOhFurnature/mverse.git
cd mverse
cargo build --release
# binaries at: target/release/metaworld_alpha  target/release/metaverse-relay
```

---

## Running the client

```bash
./metaworld_alpha
```

The client now reads `client.json` from the repo root by default.

- `network_enabled: false` → fully local/offline testing
- `bootstrap_enabled: false` → LAN/direct peers only, no remote bootstrap
- `security_enabled: false` → keep key/security plumbing present but do not enforce it during local iteration

The checked-in `client.json` is set up for local testing. Toggle those flags when you want to move back to networked or authenticated runs.

You can also point at another config file explicitly:

```bash
./metaworld_alpha --config ./client.json
```

**Controls:**
- `WASD` — move
- Mouse — look
- Left click — remove block
- Right click — place block
- `Q` — place block
- `E` — remove block
- `Esc` — quit

---

## Running a relay

### Interactive TUI mode
```bash
./metaverse-relay --port 4001
```

### With config file (recommended for persistent nodes)
```bash
./metaverse-relay --config relay.json
```

Example `relay.json`:
```json
{
  "port": 4001,
  "ws_port": 9001,
  "max_circuits": 100,
  "circuit_duration_secs": 3600,
  "circuit_data_limit_mb": 1024,
  "peer_relays": [
    "/ip4/103.216.220.190/tcp/4001/p2p/12D3KooWEVntE1LWekdyNJec7u9tKhPtFoRJsxddSJTsgUXC9UD2"
  ]
}
```

### Headless mode (set and forget)
```bash
./metaverse-relay --config relay.json --headless
```

Logs to stdout, no TUI. Use with `systemd`, `screen`, or `nohup`.

### Connect to an existing relay mesh
```bash
./metaverse-relay --port 4001 --peer /ip4/<relay-ip>/tcp/4001/p2p/<peer-id>
```

---

## Deploying a relay node

1. Open ports `4001/tcp` (P2P) and `9001/tcp` (WebSocket) on your firewall
2. Copy the binary to the server:
   ```bash
   scp target/release/metaverse-relay user@yourserver:~/
   ```
   Or use the install script on the server directly.
3. Run headless:
   ```bash
   ./metaverse-relay --config relay.json --headless
   ```
4. Add your relay's multiaddr to `bootstrap.json` in the client repo so other clients find it

**Relay public IP is auto-detected** via an external IP lookup at startup. The peer ID is derived from a key that's generated on first run and saved to disk.

### systemd service (optional)
```ini
[Unit]
Description=mverse relay
After=network.target

[Service]
ExecStart=/home/user/metaverse-relay --config /home/user/relay.json --headless
Restart=always
User=user

[Install]
WantedBy=multi-user.target
```

---

## Publishing a release

```bash
./scripts/release.sh v0.1.0 "Release notes here"
```

This builds both binaries in release mode, creates a GitHub release tag, and uploads them. The install script pulls from the latest release.

---

## bootstrap.json

The **authoritative bootstrap config** is maintained as a GitHub Gist and updated manually until relay propagation is built in:

**https://gist.github.com/PaddyOhFurnature/e5b7fc9c077016682d8eb27abd7cca17#file-bootstrap-json**

The `bootstrap.json` in this repo is a local copy — if you're getting connection issues, grab the latest from the Gist above.

WebSocket entries (`/tcp/9001/ws/`) work through VPN, CGNAT, and most firewalls. TCP entries are faster when reachable directly.

---

## Repo structure

```
src/          Library source — networking, world, rendering, multiplayer
src/bin/      Product binaries (metaworld_alpha, metaverse-relay, metaverse-server, ...)
examples/     Compatibility/demo entrypoints (legacy client shim remains temporarily)
tests/        Integration tests
scripts/      install.sh, release.sh
bootstrap.json  Relay node list for client bootstrap
```
