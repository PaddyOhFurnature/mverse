# CORRECT LAUNCHER TO USE

## ✅ CURRENT: metaworld_alpha.rs

**This is the actively developed launcher with all features.**

### Run command:
```bash
cargo run --release --example metaworld_alpha
```

Or with specific identity:
```bash
METAVERSE_IDENTITY_FILE=~/.metaverse/bob.key cargo run --release --example metaworld_alpha
```

### Features:
- ✅ Player persistence (spawn point saves/loads)
- ✅ Chunk streaming (continuous terrain loading)
- ✅ Voxel persistence (edits save to disk)
- ✅ P2P networking (mDNS discovery)
- ✅ Relay integration (NAT traversal)
- ✅ Physics and collision
- ✅ Dig/place blocks (E/Q keys)
- ✅ Chat (T key)

---

## ❌ OLD: phase1_multiplayer.rs

**This is an OLD demo file. DO NOT USE.**

It's missing:
- ❌ No player persistence integrated
- ❌ Outdated chunk streaming
- ❌ Missing relay features

**Status:** Kept for reference only, not actively maintained.

---

## Deploy Package

The `deploy/` directory contains portable binaries:
- `deploy/bin/metaworld_alpha` ← CORRECT binary
- `deploy/run_alice.sh` ← Uses metaworld_alpha
- `deploy/run_bob.sh` ← Uses metaworld_alpha

---

## Why The Confusion?

Both files exist because:
1. `phase1_multiplayer.rs` was the original multiplayer demo
2. `metaworld_alpha.rs` was created as the "real" launcher
3. Development continued on metaworld_alpha
4. phase1_multiplayer became outdated but wasn't deleted

**Going forward:** Only use and modify `metaworld_alpha.rs`

---

Last updated: 2026-02-21
