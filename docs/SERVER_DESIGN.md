# metaverse_server — Design Document

## What It Is

A NEW binary (`examples/metaverse_server.rs`) that replaces neither the relay nor the game
client, but adds a third role: always-on world authority + integrated relay.

```
metaverse_relay   — P2P relay only. Lightweight. No world state. Stays as-is.
metaverse_server  — P2P relay + world state authority + TUI + web dashboard.
metaworld_alpha   — Game client (graphical). Player-facing. Stays as-is.
```

The `--headless` flag added to metaworld_alpha was the wrong approach and should be removed
once the server binary is working. The game client should stay as a game client.

---

## Config File: server.json

Location priority: `./server.json` > `~/.metaverse/server.json`
CLI args override config values.

```json
{
  // ─── Network / Relay ───────────────────────────────────────
  "port": 4001,
  "ws_port": 9001,
  "external_addr": null,
  "node_name": "my-server",
  "node_type": "server",              // advertised in DHT: "relay" | "server"

  // Relay limits
  "max_circuits": 100,
  "max_circuit_duration_secs": 3600,
  "max_circuit_bytes": 1073741824,    // 1 GB per circuit

  // Peer access control
  "peers": [],                        // known peers to always dial
  "blacklist": [],                    // blocked peer IDs
  "whitelist": [],                    // if non-empty: ONLY these peers allowed
  "priority_peers": [],               // always prefer these for relay slots

  // Bandwidth / load limits
  "max_bandwidth_mbps": 0,            // 0 = unlimited
  "max_peers": 500,
  "max_ping_ms": 0,                   // 0 = no limit; drop peers over this RTT
  "max_retries": 5,

  // Load shedding thresholds
  "cpu_shed_threshold_pct": 90,       // start dropping Low-priority traffic above this
  "ram_shed_threshold_pct": 85,

  // ─── World State ────────────────────────────────────────────
  "world_enabled": true,
  "world_dir": "~/.metaverse/world_data",
  "max_world_data_gb": 10,            // hard cap on world_data folder
  "max_loaded_chunks": 1000,
  "chunk_load_radius_m": 500.0,
  "chunk_unload_radius_m": 600.0,
  "world_save_interval_secs": 300,    // save every 5 min

  // ─── Node Priority ─────────────────────────────────────────
  // Clients choose routes based on: node_type, ping, load, priority_score
  // server > relay for world data requests
  // relay may be preferred for pure connection bouncing when server is loaded
  "priority_score": 100,              // advertised in DHT records; higher = preferred

  // ─── Identity ───────────────────────────────────────────────
  "identity_file": "~/.metaverse/server.key",
  "temp_identity": false,

  // ─── TUI ────────────────────────────────────────────────────
  "tui_enabled": true,                // auto-detected if false not set

  // ─── Web Dashboard ──────────────────────────────────────────
  "web_enabled": true,
  "web_port": 8080,
  "web_bind": "0.0.0.0",             // set to 127.0.0.1 for local only
  "web_auth": false,                 // enable HTTP basic auth
  "web_username": "admin",
  "web_password": "",                // empty = no auth

  // ─── Logging ────────────────────────────────────────────────
  "headless": false,                  // force plain-log mode even in terminal
  "log_level": "info"                 // "trace" | "debug" | "info" | "warn" | "error"
}
```

---

## TUI Dashboard (ratatui)

Pages (keyboard shortcuts same as relay):
- **[m] Main** — peer count, circuit count, bandwidth in/out, world chunks, CPU/RAM, uptime
- **[p] Peers** — table: PeerID (short), type (relay/client/server), ping, circuits, location
- **[w] World** — world data size, loaded chunks, ops/s, pending saves
- **[l] Log** — scrollable log with level filter
- **[c] Config** — show current config (read-only in TUI; edit via web or file)
- **[h] Help** — keybindings, config path, version

Auto-detected: TUI when stdin is a terminal, plain log when piped/redirected.

---

## Web Dashboard (axum or tiny_http)

Port: configurable (default 8080).

### Pages (Pi-hole style)
- `/` — status summary (peers, uptime, bandwidth, world, load)
- `/peers` — connected peers table with kick/ban buttons
- `/world` — world state: chunks loaded, data size, ops history
- `/config` — editable config form (POST saves to server.json, server hot-reloads)
- `/logs` — live log stream (EventSource / SSE)

### API Endpoints (JSON)
- `GET /api/status` — health + stats
- `GET /api/peers` — peer list
- `POST /api/peers/{id}/kick` — disconnect peer
- `POST /api/peers/{id}/ban` — add to blacklist
- `GET /api/config` — current config
- `POST /api/config` — update config (hot reload)
- `POST /api/shutdown` — graceful shutdown (auth required)

---

## Node Priority / Routing

The server advertises `node_type = "server"` in its DHT record alongside its `priority_score`
and current load (CPU%, active circuits, bandwidth usage).

Client routing logic:
1. For **world data requests** (chunk state, op history): prefer `server` nodes first, nearest by ping
2. For **relay connections** (CGNAT hole punch): prefer whichever node has fewest active circuits
3. If server is overloaded (above shed thresholds): deprioritise below relays for new circuits
4. Geographic routing: same region first, cross-region only if no closer option

This is the same `BandwidthProfile` + `MessagePriority` system defined in NETWORKING_ARCHITECTURE.md —
the server participates as a peer in that system.

---

## Relay Role

The server runs a full libp2p relay (same as metaverse_relay).

- Same circuit limits, whitelist/blacklist
- Advertises itself as both relay and world server
- Relay traffic and world data share the same bandwidth budget
- Load shedding: under high CPU/RAM, drop relay circuits before dropping world data

The relay binary stays separate and lightweight — for cases where you want a relay
with no world state (e.g. a VPS with 512MB RAM, no disk for world data).

---

## Identity & Security

- Ed25519 keypair stored at `identity_file` path
- Same `Identity` struct used by game client
- `temp_identity`: generate throwaway key on each run (for testing)
- Player key whitelist: `whitelist` contains player PeerIDs that may request world data
  (empty = allow all)
- TLS on all libp2p connections (same as relay)

---

## Data Storage

```
world_dir/
  identity/          ← server.key lives here
  world_data/
    chunks/          ← chunk_XXXX_YYYY_ZZZZ files (voxel op logs)
    players/         ← player persistence (spawn point, inventory)
  logs/              ← optional structured logs
```

`max_world_data_gb`: when approaching limit:
1. Warn in TUI/web
2. Stop caching new chunks beyond existing loaded set
3. Never delete existing data (user must manually clear)

---

## Build / Deploy

```toml
# Cargo.toml — add new binary
[[bin]]
name = "metaverse-server"
path = "examples/metaverse_server.rs"
```

Deploy same way as relay:
```sh
cargo build --release --bin metaverse-server
scp target/release/metaverse-server user@host:~/
```

systemd unit, Docker image, Android APK — future work.

---

## What metaworld_alpha --headless Should Become

Once `metaverse_server` is working, the `--headless` flag in `metaworld_alpha` should be removed.
The game client is a game client. If someone wants a headless always-on node they run
`metaverse_server` instead.

---

## Implementation Order

1. **ServerConfig struct** — all config fields, load_config(), write_default_config_if_missing()
2. **CLI args (clap)** — --port, --web-port, --headless, --config, --world-dir, --temp-identity
3. **Identity + Networking** — same pattern as relay (libp2p swarm, relay behaviour, mDNS, DHT, bootstrap)
4. **World state integration** — ChunkStreamer, ChunkManager, UserContentLayer, PlayerPersistence
5. **TUI** — extend relay TUI with World and Peers pages
6. **Web dashboard** — axum, basic status + API endpoints
7. **Load shedding** — CPU/RAM thresholds triggering BandwidthProfile changes
8. **Node priority advertisement** — add node_type + load to DHT records
9. **Hot config reload** — SIGHUP or POST /api/config triggers reload
10. **Tests** — config parsing, load shedding thresholds, API endpoints
