# Signup & Key Propagation — Complete Design

## READ THIS FIRST

> The key is not an account on a server. It is a cryptographic sovereign identity
> that exists independently of any server. Servers are helpers — they cache,
> propagate, and serve keys — but they cannot create, modify, or revoke a key
> that isn't theirs. The network's trust lives in the math, not in any machine.

This document covers:
1. How a key comes into existence
2. What the user experiences during signup
3. How the key propagates across the decentralised network
4. How servers participate without being authoritative
5. How keys are kept alive when servers go down
6. Migration paths (Guest → Personal, key export, identity portability)

---

## 1. The Decentralised Trust Model (Re-stated for Signup)

When you create a key, four things happen simultaneously:

```
Your machine         Server(s)              Other Peers (DHT nodes)
    │                    │                       │
    │ Generate keypair   │                       │
    │ Create KeyRecord   │                       │
    │ Sign with privkey  │                       │
    │                    │                       │
    │──── gossipsub ────▶│ Verify self_sig       │
    │                    │ Store in SQLite DB     │
    │                    │ Advertise as DHT       │
    │                    │  provider              │
    │                    │                       │
    │──── gossipsub ────────────────────────────▶│ Verify self_sig
    │                                            │ Cache to disk
    │◀─────────────── DHT record available ──────│
```

**No server issued your key. No server approved it. You signed it with your
own private key. Any peer can verify it. The servers are just well-connected
cache nodes that help new peers find records they missed.**

### 1.1 What "Decentralised" Actually Means Here

- Your private key **never leaves your machine**
- Your KeyRecord is self-signed — no authority countersigns it (except Server/Relay keys)
- Any peer can verify your KeyRecord independently — no phone-home required
- If every server went offline tonight, existing players would still have each
  other's keys in their local cache and could continue playing
- When a new player joins, they need *someone* to give them the KeyRecord for
  the peers they'll encounter — that's the job of servers and the DHT
- Servers replicate keys between each other — no single server is the master

### 1.2 Server Role (Helper, Not Authority)

```
SERVER IS:                          SERVER IS NOT:
─────────────────────────────────   ─────────────────────────────────
A well-connected cache node         The issuer of user keys
A persistent DHT provider           The approver of registrations  
A gossipsub relay for new records   Able to forge or revoke user keys
An HTTP lookup endpoint             A required dependency for gameplay
A long-term persistence layer       The only source of truth
```

Multiple servers exist. They replicate to each other. Any server can answer a
key lookup. If one dies, the others still have the data. If all die, local
caches keep the game running for existing players.

---

## 2. Key Generation — What Happens On-Machine

### 2.1 First Run (Automatic Guest)

On first startup, if `~/.metaverse/identity.key` does not exist:

```
1.  Generate Ed25519 keypair (OsRng — cryptographically secure)
2.  Derive PeerId from public key (libp2p standard derivation)
3.  Construct KeyRecord:
      version:      1
      peer_id:      <derived>
      public_key:   <32 bytes>
      key_type:     Guest
      display_name: None
      created_at:   <now unix millis>
      expires_at:   <now + 30 days>
      self_sig:     <Ed25519Sign(private_key, canonical_bytes)>
4.  Save keypair to ~/.metaverse/identity.key   (chmod 600)
5.  Save KeyRecord to ~/.metaverse/identity.keyrec
6.  Do NOT publish to network (Guest keys are local-only by default)
7.  Launch into game immediately — zero friction
```

The user plays. They have full access to free-build zones. They experience the
game before committing to anything.

### 2.2 Why Guest Keys Are Local-Only By Default

- Guest keys expire in 30 days — publishing and then expiring would litter the
  DHT with dead records
- No display name = nothing meaningful to share
- Upgrading to Anonymous or Personal triggers the publish

### 2.3 Key File Format

```
~/.metaverse/identity.key      — raw 32-byte Ed25519 seed (chmod 600)
~/.metaverse/identity.keyrec   — bincode-serialized KeyRecord (public, shareable)
```

The `.key` file is the crown jewel. The `.keyrec` file is the public card. Anyone
can have your `.keyrec`. Nobody but you should ever see your `.key`.

---

## 3. Signup UI Flows

### 3.1 First-Run Screen (shown once, never again)

```
╔═══════════════════════════════════════════════════════╗
║              Welcome to the Metaverse                 ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  Your identity is a cryptographic key stored only     ║
║  on your machine. No account, no email, no server     ║
║  approval required.                                   ║
║                                                       ║
║  A temporary Guest key has been generated for you.    ║
║  You can play right now, or create a permanent        ║
║  identity.                                            ║
║                                                       ║
║  [ Play as Guest ]   [ Create My Identity ]           ║
║                                                       ║
║  Guest keys expire after 30 days of inactivity.      ║
║  Your builds in free zones will persist as long       ║
║  as other peers have seen them.                       ║
╚═══════════════════════════════════════════════════════╝
```

**"Play as Guest"** → dismiss dialog, enter game, Guest key is active.

**"Create My Identity"** → open Account Creation flow (3.2).

### 3.2 Account Creation Dialog

Step 1 — Choose identity type:

```
╔═══════════════════════════════════════════════════════╗
║              Create Your Identity                     ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  ◉  Personal      Standard user. Full capabilities.  ║
║     Name,land,trade,build,contracts.                  ║
║                                                       ║
║  ○  Anonymous     No name attached. Pseudonymous.    ║
║     Play freely, no permanent property.               ║
║                                                       ║
║  ○  Business      Organisation or brand.             ║
║     Own regions, large parcels, delegate to staff.   ║
║                                                       ║
║                          [ Next → ]                   ║
╚═══════════════════════════════════════════════════════╝
```

Step 2 — Personal: enter display name (and optionally bio/avatar hash):

```
╔═══════════════════════════════════════════════════════╗
║              Personal Identity                        ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  Display Name:  [___________________________]         ║
║  (shown to other players — choose carefully)          ║
║                                                       ║
║  Bio (optional): [___________________________]        ║
║                                                       ║
║  Avatar: [ Use default ]  [ Choose file... ]          ║
║                                                       ║
║  ⚠ Your name is NOT verified or unique.              ║
║    Anyone can pick any name. Your key IS unique.      ║
║                                                       ║
║  [ ← Back ]              [ Create Identity ]          ║
╚═══════════════════════════════════════════════════════╝
```

Step 3 — Key Backup Warning (mandatory read):

```
╔═══════════════════════════════════════════════════════╗
║  ⚠  IMPORTANT: Back Up Your Identity                 ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  Your identity is stored in ONE file:                 ║
║                                                       ║
║    ~/.metaverse/identity.key                         ║
║                                                       ║
║  If you lose this file:                              ║
║  • Your account is gone. Forever.                    ║
║  • Your land, buildings, and items are locked.       ║
║  • There is no recovery. No support ticket.          ║
║    No password reset. No backup on any server.       ║
║                                                       ║
║  Copy this file somewhere safe RIGHT NOW.            ║
║  USB drive. Encrypted cloud. Hardware wallet.        ║
║                                                       ║
║  [ Open Backup Location ]  [ I'll do it later ]      ║
║                                                       ║
║  ☐ I understand — there is no recovery if I lose    ║
║    this file.                                         ║
║                                                       ║
║  [ Enter World (checkbox required) ]                  ║
╚═══════════════════════════════════════════════════════╝
```

Checkbox must be checked to proceed. "Open Backup Location" opens the file manager
to `~/.metaverse/` so the user can copy the key file immediately.

### 3.3 Anonymous Signup

Much simpler — no name needed:

```
╔═══════════════════════════════════════════════════════╗
║              Anonymous Identity                       ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  You'll appear as "Anonymous-<short key hash>".       ║
║  No name, no profile. But your key is consistent —   ║
║  other players will recognise you by your key ID.    ║
║                                                       ║
║  You can own things and build. You cannot:           ║
║  • Claim permanent parcels                           ║
║  • Sign contracts or listings                        ║
║  • Use your real name (obviously)                    ║
║                                                       ║
║  [ ← Back ]              [ Create Anonymous Key ]    ║
╚═══════════════════════════════════════════════════════╝
```

Then skip straight to the backup warning.

### 3.4 Guest → Personal Migration

A Guest key's ops can be re-signed under a new Personal key via a **migration record**:

```
╔═══════════════════════════════════════════════════════╗
║              Upgrade Your Identity                    ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  You're currently using a Guest key.                 ║
║                                                       ║
║  Creating a Personal key will:                       ║
║  ✅ Give you a permanent identity                    ║
║  ✅ Unlock parcels, contracts, and commerce          ║
║  ✅ Migrate your existing builds to the new key      ║
║     (other peers will see a migration record)        ║
║                                                       ║
║  A new keypair will be generated. Your Guest key     ║
║  will sign a MigrateIdentity operation pointing      ║
║  to your new Personal key.                           ║
║                                                       ║
║  [ ← Cancel ]            [ Upgrade Identity ]        ║
╚═══════════════════════════════════════════════════════╝
```

The migration op is a `SignedOperation { action: Action::MigrateIdentity { old_key: PeerId, new_key: PeerId } }` signed by the OLD Guest key. This is published to the network so peers can update attribution on that player's old builds.

### 3.5 Settings — Identity Panel

Available at any time from the settings menu:

```
╔═══════════════════════════════════════════════════════╗
║              Identity & Keys                          ║
╠═══════════════════════════════════════════════════════╣
║                                                       ║
║  Active Identity                                     ║
║  ─────────────                                       ║
║  Name:     PaddyOh                                   ║
║  Type:     Personal                                  ║
║  Key ID:   12D3KooW...C9UD2  (short)                ║
║  Created:  2026-02-24                                ║
║  Status:   ✅ Active                                 ║
║                                                       ║
║  [ Copy Full Key ID ]  [ View Public Record ]        ║
║                                                       ║
║  Key File                                            ║
║  ─────────                                           ║
║  Location: ~/.metaverse/identity.key                ║
║  Backed up: ⚠ Not confirmed                         ║
║                                                       ║
║  [ Open Backup Location ]  [ Verify Backup ]         ║
║                                                       ║
║  Advanced                                            ║
║  ────────                                            ║
║  [ Switch Identity File... ]  [ Revoke This Key ]   ║
║  [ Export for Hardware Wallet ]                      ║
╚═══════════════════════════════════════════════════════╝
```

---

## 4. Key Propagation — How Records Spread

### 4.1 The Three Propagation Channels

Keys spread via three independent, redundant channels. A key published via any one
of them will eventually appear in all others.

```
Channel 1: Gossipsub "key-registry" topic
  ├─ Immediate broadcast to all currently connected peers
  ├─ Servers receive → store in SQLite
  ├─ Clients receive → store in local disk cache
  └─ Fast: < 1 second to all connected peers

Channel 2: Kademlia DHT (record + provider advertisement)
  ├─ put_record(sha256(peer_id_bytes), keyrec_bytes)
  ├─ start_providing(sha256(peer_id_bytes))
  ├─ Persistent: survives client disconnects
  └─ Discovery: any peer can query without prior contact

Channel 3: Server REST API
  ├─ GET /api/keys/<peer_id>  → single record
  ├─ GET /api/keys/batch      → multiple records by list of PeerIds
  ├─ GET /api/keys?type=relay → all relay keys
  ├─ GET /api/keys?updated_after=<timestamp> → incremental sync
  └─ Served from SQLite — always available even when gossipsub is quiet
```

### 4.2 Full Propagation Flow — New Personal Key

```
T=0   User clicks "Create Identity" on machine A
      → keypair generated locally
      → KeyRecord constructed and self-signed
      → saved to ~/.metaverse/identity.key + identity.keyrec

T=1ms Client connects to gossipsub network

T=2ms Client publishes KeyRecord to "key-registry" gossipsub topic

T=3ms Server S1 (connected to gossipsub) receives KeyRecord
      → verify self_sig (Ed25519 verify)
      → INSERT INTO key_registry.db WHERE peer_id = <new>
      → kademlia.put_record(sha256(peer_id), keyrec_bytes)
      → kademlia.start_providing(sha256(peer_id))
      
T=3ms Server S2 (also on gossipsub) receives KeyRecord
      → same as S1 — now TWO servers have it

T=4ms Other connected clients receive KeyRecord via gossipsub
      → verify self_sig
      → save to ~/.metaverse/key_cache/<prefix>/<peer_id>.keyrec

T=??  Peer B connects later (missed the gossipsub)
      → B connects to S1 on join
      → S1 sends batch of recently-seen KeyRecords
      → OR B needs to check A's key → DHT lookup → S1 responds as provider
      → B now has A's KeyRecord cached locally
```

### 4.3 Server-to-Server Replication

Servers replicate to each other so no single server is the source of truth.

**Two mechanisms:**

**A) Push on receive (immediate):**
```
Server S1 receives new KeyRecord from gossipsub
  → validates it
  → stores locally
  → republishes to its own gossipsub peers (including other servers)
  → S2 receives it via gossipsub from S1
```

**B) Pull on startup / periodic sync (eventual consistency):**
```
Server S2 starts up
  → S2 queries S1: GET /api/keys?updated_after=<S2's last sync timestamp>
  → S1 returns all records updated since then
  → S2 merges: keeps record with higher updated_at for each peer_id
  → S2 now has full picture
  → S2 runs this sync against all known servers every ~10 minutes
```

**Conflict resolution:** `updated_at` wins. Newer record supersedes older one.
Server re-verifies self_sig before storing any record received from another server
— a compromised server cannot inject fake records because it can't forge signatures.

### 4.4 New Player Joins — Key Discovery Flow

When player A joins and needs to see player B's KeyRecord:

```
A needs to know who B is:

1. Check A's in-memory cache → not found (new connection)
2. Check A's disk cache ~/.metaverse/key_cache/ → not found (never seen B)
3. Query DHT: get_record(sha256(B.peer_id))
   ├─ DHT returns B's KeyRecord from closest node
   └─ A verifies self_sig, caches to disk
4. If DHT fails (young DHT, peer just joined):
   a. Ask any connected server: GET /api/keys/<B.peer_id>
   b. Server responds with B's record from SQLite
   c. A verifies self_sig, caches to disk
5. If server not reachable:
   a. B publishes own KeyRecord on connection (existing behaviour)
   b. A receives via gossipsub, verifies, caches
6. If none of the above work:
   → Treat B as Guest (minimum permissions) until record arrives
   → Retry lookup after 30 seconds
```

### 4.5 Key Lifetime and Expiry

| Key Type  | Default Expiry | Renewal                              |
|-----------|---------------|--------------------------------------|
| Guest     | 30 days        | Auto-renewed on each active session  |
| Anonymous | Never          | Manual revocation only               |
| Personal  | Never          | Manual revocation only               |
| Business  | Never          | Manual revocation only               |
| Admin     | Set by Server  | Renewed by Server key countersignature|
| Relay     | 1 year         | Renewed by Server key countersignature|
| Server    | 2 years        | Renewed by Genesis key               |

Guest key renewal: when a Guest key expires, a new Guest key is auto-generated
on next startup. Old Guest key's ops remain in the op-log but are attributed to
a now-expired key (lowest permission level, cannot do new ops).

### 4.6 Revocation Propagation

Revocations are `SignedOperation { action: Action::RevokeKey { ... } }` and propagate
identically to any other signed operation:

```
Revocation signed and published
  → gossipsub broadcasts immediately
  → servers receive, update key_registry.db: SET revoked=true
  → servers push revoked record to DHT (same key, new value with revoked=true)
  → peers receive, update local cache
  → next permission check on any op from that key → PermissionResult::Revoked
  → ops signed before revocation: kept in op-log (history is immutable)
    but new ops from that key are blocked
```

There is a propagation window (seconds to minutes) during which some peers may
not yet have received the revocation. This is acceptable — the worst case is that
a revoked key gets one more op through before the revocation arrives. CRDT merge
handles this gracefully: the revocation record's timestamp proves it predates any
post-revocation ops, so those can be tombstoned.

---

## 5. Server Key Registry — Database Schema

Each server maintains `~/.metaverse/key_registry.db` (SQLite):

```sql
-- Primary key records table
CREATE TABLE key_records (
    peer_id         TEXT PRIMARY KEY,   -- base58 PeerId string
    public_key      BLOB NOT NULL,      -- 32-byte Ed25519 pubkey
    key_type        TEXT NOT NULL,      -- "Guest","Anonymous","Personal","Business","Admin","Relay","Server","Genesis"
    display_name    TEXT,               -- null for Anonymous/Guest
    bio             TEXT,
    avatar_hash     BLOB,               -- 32-byte content hash
    created_at      INTEGER NOT NULL,   -- unix millis
    expires_at      INTEGER,            -- null = never
    updated_at      INTEGER NOT NULL,   -- unix millis (for sync)
    issued_by       TEXT,               -- null for user keys
    issuer_sig      BLOB,               -- null for user keys
    revoked         INTEGER NOT NULL DEFAULT 0,
    revoked_at      INTEGER,
    revoked_by      TEXT,
    revocation_reason TEXT,
    self_sig        BLOB NOT NULL,      -- 64-byte Ed25519 signature
    raw_bytes       BLOB NOT NULL,      -- full bincode-serialized KeyRecord (serve directly)
    received_at     INTEGER NOT NULL,   -- when THIS server first saw it (for sync window)
    received_from   TEXT                -- which peer sent it to us (audit trail)
);

-- Index for common lookups
CREATE INDEX idx_key_type      ON key_records(key_type);
CREATE INDEX idx_updated_at    ON key_records(updated_at);  -- incremental sync
CREATE INDEX idx_received_at   ON key_records(received_at);
CREATE INDEX idx_revoked       ON key_records(revoked);

-- Cross-server sync tracking
CREATE TABLE server_sync (
    server_peer_id  TEXT PRIMARY KEY,
    last_synced_at  INTEGER NOT NULL,   -- unix millis
    records_received INTEGER NOT NULL DEFAULT 0
);
```

### 5.1 REST Endpoints

```
GET  /api/keys/<peer_id>
     → 200 + KeyRecord JSON
     → 404 if unknown

GET  /api/keys?type=relay
     → 200 + array of relay KeyRecords

GET  /api/keys?type=server  
     → 200 + array of server KeyRecords

GET  /api/keys/batch
     Body: { "peer_ids": ["12D...", "12D...", ...] }
     → 200 + object mapping peer_id → KeyRecord (missing ones omitted)

GET  /api/keys?updated_after=<unix_millis>
     → 200 + array of records updated since that timestamp (server sync)

GET  /api/keys?updated_after=<unix_millis>&limit=1000&offset=0
     → paginated incremental sync

POST /api/keys
     Body: KeyRecord JSON (signed)
     → Server verifies self_sig, stores if valid
     → 201 Created | 400 Bad Request | 409 Conflict (older record)
     → This lets clients submit keys to servers directly over HTTP
       (fallback for peers that can't reach gossipsub)
```

---

## 6. Relay Key Issuance

See **Section 9** for the complete practical workflow — CLI commands, step-by-step
operator flow, renewal, and revocation.

The cryptographic structure:
- Relay operator generates their own keypair (their private key never leaves their machine)
- Relay operator sends their public key to a server operator (out of band)
- Server operator runs `metaverse-server issue-relay-key` → produces a signed `.keyrec`
- Signed `.keyrec` returned to relay operator — not secret, anyone can read it
- Relay operator installs it as `~/.metaverse/relay.keyrec` and starts the relay
- Relay broadcasts the signed record on connect — peers verify both the relay's
  self_sig AND the server's issuer_sig before trusting it as a routing node

---

## 7. Implementation Plan for Phase 5

### 7.1 Code Changes Required

**New Action variants in `src/messages.rs`:**
```rust
// Migration: Guest/Anonymous → Personal/Business
MigrateIdentity {
    old_key: PeerId,   // the key being retired
    new_key: PeerId,   // the key taking over
},

// Relay/Server key issuance
IssueRelayKey {
    relay_peer_id: PeerId,
    relay_public_key: [u8; 32],
    config: RelayConfig,
},
```

**New methods in `src/identity.rs`:**
```rust
impl Identity {
    /// Generate a fresh keypair and write identity.key (first run).
    pub fn generate_and_save(path: &Path) -> Result<Self>;

    /// Upgrade key_type in the KeyRecord and re-sign.
    /// Does NOT change the underlying keypair.
    pub fn upgrade_key_type(&mut self, new_type: KeyType, display_name: Option<String>) -> Result<KeyRecord>;

    /// Create a migration SignedOperation from old_identity to self.
    pub fn create_migration_op(&self, old_identity: &Identity) -> Result<SignedOperation>;

    /// Check file permissions — warn if key file is readable by others.
    pub fn check_file_security(path: &Path) -> SecurityStatus;
}
```

**New server endpoints in `examples/metaverse_server.rs`:**
- Full schema above
- `GET /api/keys?updated_after=<ts>` for cross-server sync
- `POST /api/keys` for direct HTTP submission

**UI in `examples/metaworld_alpha.rs`:**
- First-run detection: `if !identity_path.exists()`
- Egui dialogs for each signup flow
- Settings panel with identity info

### 7.2 File Locations

```
~/.metaverse/
  identity.key          Ed25519 seed (32 bytes, chmod 600)
  identity.keyrec       Public KeyRecord (bincode, shareable)
  relay.key             (relay operators only)
  relay.keyrec          (relay operators only)
  server.key            (server operators only)
  server.keyrec         (server operators only)
  key_cache/
    <2-char prefix>/    (sharded to avoid huge flat directory)
      <peer_id>.keyrec  Cached remote KeyRecords
  key_registry.db       (servers only) SQLite key database
```

---

## 8. Resolved Design Decisions

**8.1 ALL Keys Are Published (Including Guest)**
Every key type — including Guest — is published to gossipsub and DHT immediately
on first connection. This is required so that report, ban, invite, message, and
all social operations work regardless of key type. A player you can't look up
is a player you can't interact with at all.

Guest keys are published with `key_type: Guest` so receiving peers know to apply
Guest-level permissions. The 30-day expiry is still in the record — the network
can prune expired Guest records from DHT provider lists over time, but they stay
in server DBs permanently (revocation/expiry history must be kept).

**8.2 Display Name Uniqueness — Not Enforced**
Display names are self-declared and NOT unique. This is intentional. The key IS
the identity — the display name is just a human-readable label. If two players
pick the same name, peers distinguish them by their Key ID (the short hash shown
in the UI). Think of it like two people named "John" in a room — you refer to
them as "John 3KooW..." and "John 7xFpQ...". No central registry, no first-claim
system, no disputes. The Key ID is the ground truth.

(Future: players can verify each other's identity by trading signed "I am who I
say I am" attestations, or by recognising the Key ID from a previous session.
But unique name enforcement would require a central authority, which we reject.)

**8.3 Key Expiry: 24h Grace → 7-Day Suspend → Hard Expire**
Instead of hard cutoff at expiry timestamp:

```
expires_at                  +24h            +7 days
    │                          │                 │
    ▼                          ▼                 ▼
    ├─── Active ───────────────┤─── Suspended ───┤─── Expired ───▶
                           Grace window     Can still read,
                           Full capability  can't write new ops.
                                           Key still visible for
                                           history/attribution.
                                           After 7 days: all new
                                           ops denied, key enters
                                           permanent expired state.
```

Grace window handles: clock skew between peers, key renewal in progress,
brief offline period during renewal. Suspended state: ops are queued locally
but blocked from network propagation until renewed. Adjust timing later.

**8.4 Key Backup Nag**
Nag on the load screen only (when we implement the proper load/title screen).
Not our problem until then. One persistent nag — dismiss = never shown again
ONLY after user checks the "I understand" checkbox. If they skip without the
checkbox, it shows again next session.

**8.5 Business Key Multi-Sig**
First version: single keypair, same as Personal structurally. Struct has a
placeholder `pub threshold_signers: Vec<PeerId>` field that's always empty for now.
Multi-sig implementation deferred — placeholder ensures we don't have to change
the wire format later.

**8.6 Relay Key Issuance — Practical Workflow**
See Section 9 below — this is the big one, documented in full.

---

## 9. Relay Key Issuance — Practical Workflow

This is the most operationally complex part of the key system because it requires
a human trust decision (a server operator choosing to vouch for a relay operator)
backed by cryptography. Here is the complete end-to-end flow — what the software
does, what the humans do, and how it all fits together.

### 9.1 Who's Involved

```
Genesis Key holder     — You (PaddyOh). Cold storage. Signs new Server Keys.
Server Key holder      — Whoever runs a metaverse-server instance. Signs Relay Keys.
Relay Key applicant    — Anyone wanting to run a relay node. Gets countersigned.
```

In the initial network, you hold the Genesis Key AND the Server Key. As the network
grows, you may issue Server Keys to other trusted operators who can then issue
their own Relay Keys independently.

### 9.2 What a Relay Key Actually Does

A Relay Key is not just permission to run the relay software — it's a public
commitment to the network that:
- This node is trusted to route traffic
- A Server Key holder has verified (out of band) that the operator is legitimate
- If the relay misbehaves, the Server Key holder can revoke the Relay Key
- All clients preferentially connect to relays with valid, non-revoked Relay Keys

Without a Relay Key, the relay software still runs, but the network treats it
as an untrusted peer (same as any client). With a Relay Key, it gets elevated
routing priority and is listed in the bootstrap config.

### 9.3 The Issuance Flow — Step by Step

#### Step 1: Relay operator initialises their relay key

The person wanting to run a relay, on their machine:

```bash
# Generates a relay keypair — separate from their personal identity key
./metaverse-relay --init-key

# Output:
# ✅ Generated relay key: ~/.metaverse/relay.key
# 
# Your relay identity:
#   Public key (hex): a3f8c2d1e4b97f3...
#   PeerId:           12D3KooWXyz...
# 
# Send the following to a server operator to get your relay key signed:
# ─────────────────────────────────────────────────────
# pubkey:   a3f8c2d1e4b97f3...
# peer_id:  12D3KooWXyz...
# ─────────────────────────────────────────────────────
```

#### Step 2: Relay operator contacts a server operator

Out-of-band contact — Discord, email, GitHub issue, whatever. The relay operator
sends their pubkey + peer_id and describes what they're running (location, hardware,
bandwidth available). The server operator decides: **do I trust this person to
run a relay?** This is a human decision. Code enforces the outcome but cannot
make the judgement call.

#### Step 3: Server operator signs the relay key

On the server machine (does NOT need the server to be running — offline operation):

```bash
./metaverse-server issue-relay-key \
  --pubkey a3f8c2d1e4b97f3... \
  --peer-id 12D3KooWXyz... \
  --display-name "Alice's Sydney Relay" \
  --expires 365d

# Output:
# ✅ Relay KeyRecord signed.
#
# Relay:   Alice's Sydney Relay (12D3KooWXyz...)
# Issuer:  12D3KooW...ServerPeerId (this server)
# Expiry:  2027-02-24
#
# Signed record saved to: ./relay_12D3KooWXyz.keyrec
#
# Send this file to the relay operator.
# It is NOT secret — anyone can read it. It just proves this server
# countersigned it. The relay operator installs it as ~/.metaverse/relay.keyrec
```

The `.keyrec` file is public. No secrets in it. It's a signed certificate saying
"Server X vouches for Relay Y." The relay operator's private key stays on their
own machine and is never shared.

#### Step 4: Relay operator installs the signed record and starts

```bash
# On the relay machine
cp relay_12D3KooWXyz.keyrec ~/.metaverse/relay.keyrec

./metaverse-relay

# Output:
# 🔑 Relay identity: Alice's Sydney Relay
# 🔑 PeerId:         12D3KooWXyz...
# ✅ Relay KeyRecord: countersigned by 12D3KooW...Server (valid)
# ✅ Expiry:          2027-02-24 (365 days remaining)
# 🌐 Listening:       /ip4/0.0.0.0/tcp/4001
# 📡 Publishing relay KeyRecord to network...
```

On first gossipsub connection the relay broadcasts its relay.keyrec. Any peer
that receives it:
1. Verifies the relay's self_sig (relay holds the private key)
2. Verifies the issuer_sig using the server's public key
3. Confirms the server is a valid Server Key in their registry
4. If all pass → treats this peer as trusted relay, elevated priority

#### Step 5: Adding to Bootstrap

Once running, server operator adds the relay to the bootstrap Gist:

```json
{
  "relays": [
    {
      "peer_id": "12D3KooWXyz...",
      "addrs": ["/ip4/203.x.x.x/tcp/4001"],
      "display_name": "Alice's Sydney Relay",
      "key_type": "Relay",
      "issuer": "12D3KooW...Server"
    }
  ]
}
```

Now any new client bootstrapping from the Gist finds Alice's relay immediately,
dials it, receives the relay.keyrec, verifies the countersignature, and trusts it.

### 9.4 Renewal (Before Expiry)

```bash
# Server operator renews before the 365-day expiry
./metaverse-server renew-relay-key \
  --peer-id 12D3KooWXyz... \
  --extends 365d

# Output:
# ✅ Renewed KeyRecord for Alice's Sydney Relay
# New expiry: 2028-02-24
# Saved: ./relay_12D3KooWXyz_renewed.keyrec
```

Relay operator installs the renewed `.keyrec`. The running relay picks it up on
next publish cycle (no restart needed — it re-reads the file periodically).

### 9.5 Revocation (Relay Goes Rogue or Unresponsive)

```bash
./metaverse-server revoke-relay-key \
  --peer-id 12D3KooWXyz... \
  --reason "Relay offline > 30 days, bootstrap entry removed"

# Output:
# ✅ Revocation signed and broadcast.
# Revoked: Alice's Sydney Relay (12D3KooWXyz...)
# Reason:  Relay offline > 30 days, bootstrap entry removed
# 
# The revocation will propagate via gossipsub within ~60 seconds.
# Peers will stop treating 12D3KooWXyz... as a trusted relay.
```

### 9.6 CLI Commands Required in `metaverse-server`

These are all offline operations — read server.key, sign, write .keyrec file.
The server process doesn't need to be running.

```
metaverse-server init-server-key
    Generate server.key (first time setup)

metaverse-server show-server-key
    Print this server's own KeyRecord info

metaverse-server issue-relay-key --pubkey <hex> --peer-id <id> --display-name <str> --expires <duration>
    Sign a new Relay KeyRecord, output .keyrec file

metaverse-server renew-relay-key --peer-id <id> --extends <duration>
    Re-sign with new expiry, output .keyrec file

metaverse-server revoke-relay-key --peer-id <id> --reason <str>
    Sign and broadcast a RevokeKey op

metaverse-server list-relay-keys [--expired] [--revoked]
    Show all relay keys this server has issued

metaverse-server issue-server-key --pubkey <hex> --peer-id <id> --display-name <str>
    (Genesis Key holder only) Issue a new Server Key
```

### 9.7 Trust Chain Summary

```
Genesis Key  (cold storage — YOU, air-gapped or hardware wallet)
    │
    └── countersigns ──▶  Server Key  (metaverse-server, your VPS)
                              │
                              ├── countersigns ──▶  Relay Key  (trusted relay operators)
                              │
                              └── countersigns ──▶  Admin Key  (region moderators)

User Keys (self-signed — no countersignature needed):
    Personal Key     ← full capabilities
    Business Key     ← organisations
    Anonymous Key    ← pseudonymous
    Guest Key        ← auto-generated, ephemeral
```

Genesis Key: NEVER on an internet-connected machine after the initial server key
issuances. Generate it once, issue the server key, put it in cold storage. Use
it only to issue new server keys or to revoke compromised ones. Consider making
it a 2-of-3 threshold (you + two trusted people) once the network is established.

---

## 10. What This Unlocks (Downstream Features)

Once signup and propagation are working:

| Feature                  | Unblocked By                                          |
|--------------------------|-------------------------------------------------------|
| Parcel ownership         | Personal key exists + ClaimParcel action wired        |
| Trading / Marketplace    | Personal key + CreateListing / AcceptListing actions  |
| Access grants            | GrantAccess action + ownership check                  |
| Region moderation        | Admin key issuance flow                               |
| Trusted relay mesh       | Relay key issuance flow                               |
| Blueprints / assets      | PublishBlueprint action + content-addressed storage   |
| Scripts                  | DeployScript action + key-type check                  |
| Governance votes         | Personal key exists + Vote action                     |
| Guest migration          | MigrateIdentity action + migration op handling        |
| Multi-server federation  | Server sync endpoints + cross-server key replication  |

---

*Status: Design draft — for review and discussion before implementation*
*Author: Copilot (initial draft), PaddyOh (to add/modify)*
