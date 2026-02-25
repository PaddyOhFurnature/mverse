# Identity & Key System — Complete Design

## READ THIS FIRST

> "Everything in the game that is not core code will be added, deleted, modified,
> sold, traded, imported, designed, moved, interacted with — with a key attached
> to the process. Even your home, which can exist fully offline, won't exist
> without your key."

The key is not a login system. It is not an account. It is the **cryptographic
proof of sovereignty** over everything you do, own, and create in the metaverse.
Lose it and you don't just lose your account — you lose your home, your land,
your creations, your history. There is no recovery. No reset. No support ticket.
The key IS you.

This document specifies the complete identity and key system: structure, hierarchy,
permissions, propagation, signup, and every operation that requires a key.

---

## 1. The Key as Sovereign Identity

### 1.1 What a Key Is

An **Ed25519 keypair** (32-byte private seed, 32-byte public key):
- **Private key**: Never leaves your machine. Signs everything. Proves you authored it.
- **Public key**: Shared with the network. Anyone can verify your signatures.
- **PeerId**: Derived deterministically from public key. Your unique address in the P2P network.

The key file (`~/.metaverse/identity.key`) is your entire identity. One file.
Portable, self-contained, no server required to use it.

### 1.2 What the Key Governs

**Every non-deterministic operation in the metaverse requires a key signature:**

| Category | Operations |
|---|---|
| **Terrain** | Place voxel, remove voxel, fill region, clear region |
| **Objects** | Place object, remove object, move object, configure object |
| **Land** | Claim parcel, abandon parcel, subdivide parcel, merge parcels |
| **Ownership** | Transfer ownership, gift item, sell item, buy item |
| **Access** | Grant build rights, revoke rights, invite user, ban user |
| **Content** | Create blueprint, publish model, import asset, export content |
| **Commerce** | List item for sale, create contract, accept contract, cancel listing |
| **Scripts** | Deploy script, update script, revoke script, grant script permissions |
| **Social** | Send message, create group, invite to group, post to region |
| **Infrastructure** | Register as relay, register as server, update relay config |
| **Governance** | Vote on region proposal, create proposal, execute proposal |
| **Identity** | Publish KeyRecord, update display name, update avatar, revoke key |

**The deterministic world (terrain height, road layout from OSM) does not require
a key — it is verified by math. Everything humans add requires a key.**

### 1.3 Offline Operation

Your key works offline. Your home exists as a locally-stored chunk file containing
a signed op-log. Every voxel you placed, every object you put down — all signed
by your key. When you reconnect, peers verify your signatures and merge your
changes. The world doesn't need a server to know what belongs to you.

---

## 2. Key Hierarchy

### Tier 1: Infrastructure Keys

These keys have hardware roles. They are long-lived, carefully managed,
and countersigned by higher-tier keys.

#### 2.1 Genesis Key
- The original trust anchor for the network
- Signs the initial server key issuances
- MUST be kept in cold storage (hardware wallet or air-gapped machine)
- Ideally becomes a multi-signature threshold (3-of-5) over time
- Not a regular operating key — used only to issue and revoke server keys

#### 2.2 Server Key
- Issued by Genesis Key (countersignature required)
- Operates a world-state authority node (`metaverse-server`)
- Top-tier trust for chunk validation and conflict resolution
- Can issue Relay Keys
- Can issue Admin Keys for regions it hosts
- Stores its `server.key` in `~/.metaverse/server.key` — locked down, not `identity.key`
- Identified on the network by `node_type: "server"` in its KeyRecord

#### 2.3 Relay Key
- Issued by a Server Key (countersignature required)
- Operates a relay node (`metaverse-relay`)
- Trusted to route traffic — cannot alter or store world state
- Can be run on consumer hardware (phones, home PCs, VPS)
- Stores its `relay.key` in `~/.metaverse/relay.key`
- Hardware running a relay key gets elevated connection priority (not a regular peer)

### Tier 2: User Keys

User keys are self-registered. No central authority issues them. All tiers above
Trial are verified and published to the network's DHT, cached by servers, and
propagated via gossipsub.

#### 2.4 Admin Key
- Granted elevated moderation rights by a Server Key over a specific region
- Can moderate, kick, ban, approve builds in assigned regions
- Cannot operate infrastructure
- A single person can have an Admin Key AND a User Key (separate files)

#### 2.5 User Key  *(formerly Personal)*
- Full account — all gameplay capabilities unlocked
- Own land, build anywhere you have rights, trade, create content, DMs
- Display name declared in KeyRecord (self-declared, not verified)
- Earned: must have been a Guest for 30 days in good standing, OR
  invited by an existing User (each invite reduces wait by 5 days;
  Admin invites bypass the wait entirely)
- Requires additional verification beyond Guest: government/photo ID
- Address verification optional: locks your virtual home address to a real-world
  address (same house → same plot in-game)
- One person, one key — but nothing stops multiple keys (personas)
- Business account is created FROM a User account (not a separate signup)

#### 2.6 Business Key
- Organisation or brand identity — created under a User account, like a Facebook page
- User creates it, adds admins/mods via signed access grants
- Can own larger parcels and regions at scale
- Can delegate sub-permissions to staff
- Flagged distinctly in UI ("Business" badge)
- Cannot exist without a sponsoring User key

#### 2.7 Guest Key
- Free account — verified email address required
- Choose your own nickname (display name)
- Home plot assigned on first login
- Public text chat allowed — no DMs
- Can watch free content, receive free items from game and other players
- Distributed key — published to DHT (this is a real, persistent account)
- Account has standing: can be blacklisted, whitelisted, kicked, suspended, banned
- Key stored locally; expires after 30 days of inactivity
- Minimum 30 days good standing before upgrade to User
- Can be "upgraded" — generate User Key, sign migration record, Guest ops inherit

#### 2.8 Trial Key  *(formerly Anonymous)*
- No registration, no email, no name — walk-around only
- Keypair regenerated every hour; player is returned to the lobby for a new key
- Predefined safe chat options only — no free text chat
- Can walk freely and observe; basic non-verbal interaction (wave, accept items)
- Cannot build, own property, sign contracts, or trade
- Not published to DHT — entirely ephemeral, zero persistent identity
- Designed as a zero-friction "try before you register" experience
---

## 3. Key Record Structure

A `KeyRecord` is the public declaration of an identity. It is:
- Self-signed (proves you hold the private key)
- Published to DHT keyed by PeerId
- Broadcast via gossipsub `"key-registry"` topic
- Cached by servers and clients

```rust
/// The public declaration of an identity on the network.
/// Everything here is public. Never put private data here.
pub struct KeyRecord {
    /// Version for forward compatibility
    pub version: u8,

    // ── Core Identity ──────────────────────────────────────
    /// Unique identifier, derived from public key
    pub peer_id: PeerId,

    /// Ed25519 public key (32 bytes)
    pub public_key: [u8; 32],

    /// The type and trust tier of this key
    pub key_type: KeyType,

    // ── Human-Readable Metadata ────────────────────────────
    /// Chosen display name (None = anonymous)
    pub display_name: Option<String>,

    /// Short bio or description (None = no bio)
    pub bio: Option<String>,

    /// Content-addressed hash of avatar data (None = default avatar)
    pub avatar_hash: Option<[u8; 32]>,

    // ── Temporal ───────────────────────────────────────────
    /// Unix timestamp when key was first published
    pub created_at: u64,

    /// Expiry timestamp (None = never expires; Guest keys expire)
    pub expires_at: Option<u64>,

    /// Timestamp of last update to this record
    pub updated_at: u64,

    // ── Trust Chain (Infrastructure Keys Only) ────────────
    /// PeerId of the key that issued/countersigned this one
    pub issued_by: Option<PeerId>,

    /// Countersignature from the issuer (proves issuer authorised this key)
    pub issuer_sig: Option<[u8; 64]>,

    // ── Revocation ─────────────────────────────────────────
    /// True if this key has been revoked
    pub revoked: bool,

    /// Timestamp of revocation
    pub revoked_at: Option<u64>,

    /// Who revoked it: self-revoke, or revoked by an authority key
    pub revoked_by: Option<PeerId>,

    /// Reason for revocation (human-readable, optional)
    pub revocation_reason: Option<String>,

    // ── Self-Signature ─────────────────────────────────────
    /// Signs all above fields. Proves you hold the private key for this record.
    /// Computed as: Ed25519Sign(private_key, canonical_bytes(all_fields_above))
    pub self_sig: [u8; 64],
}

pub enum KeyType {
    Genesis,    // The network's root trust anchor
    Server,     // World-state authority node
    Relay,      // Routing infrastructure node
    Admin,      // Region moderator (granted by Server key)
    Business,   // Organisation/brand identity (created under a User account)
    User,       // Full account (earned from Guest after 30 days or invite)
    Guest,      // Free account — verified email, home plot, public chat, moderatable
    Trial,      // Walk-around only — hourly reset, no persistent identity
}
```

### 3.1 Canonical Signing Format

The `self_sig` field signs a deterministic byte representation of all other fields.
Field order is fixed. This ensures any peer can verify a KeyRecord independently.

### 3.2 KeyRecord Updates

- Any field can be updated (display name, avatar, bio)
- A new `self_sig` covers the updated `updated_at` timestamp
- Old records are superseded; peers keep the highest `updated_at`
- Public key and `created_at` CANNOT be changed — they are the identity
- Key type CANNOT be self-upgraded (Guest → Personal requires a new key, not an edit)

---

## 4. Permission System

Permissions are evaluated **client-side** and **server-side** independently.
No central authority grants permissions — the signed op-log IS the authority.

### 4.1 Permission Tiers by Key Type

```
OPERATION                  Trial  Guest  User      Business  Admin  Relay  Server
─────────────────────────────────────────────────────────────────────────────────
Move / explore              ✅     ✅     ✅         ✅        ✅     -      -
Wave / accept gift          ✅     ✅     ✅         ✅        ✅     -      -
Predefined chat             ✅     ❌     ❌         ❌        ❌     -      -
Public text chat            ❌     ✅     ✅         ✅        ✅     -      -
Direct messages (DM)        ❌     ❌     ✅         ✅        ✅     -      -
Build in free-build zones   ❌     ✅     ✅         ✅        ✅     -      -
Build in owned parcel       ❌     ❌     ✅(own)    ✅(own)   ✅     -      -
Claim a parcel              ❌     ❌     ✅         ✅        ✅     -      -
Transfer ownership          ❌     ❌     ✅(own)    ✅(own)   ✅     -      -
Grant build access          ❌     ❌     ✅(own)    ✅(own)   ✅     -      -
Revoke build access         ❌     ❌     ✅(own)    ✅(own)   ✅     -      -
Watch free content          ✅     ✅     ✅         ✅        ✅     -      -
Create/publish content      ❌     ❌     ✅         ✅        ✅     -      -
Import asset                ❌     ❌     ✅         ✅        ✅     -      ✅
List item for sale          ❌     ❌     ✅         ✅        ✅     -      -
Receive free items          ✅     ✅     ✅         ✅        ✅     -      -
Sign a contract             ❌     ❌     ✅         ✅        ✅     -      ✅
Deploy a script             ❌     ❌     ✅(own)    ✅(own)   ✅     -      ✅
Vote in governance          ❌     ❌     ✅         ✅        ✅     -      -
Create governance proposal  ❌     ❌     ✅(owner)  ✅(owner) ✅     -      ✅
Kick/ban user from region   ❌     ❌     ❌         ❌        ✅     -      ✅
Account moderation          ❌     ✅*    ✅         ✅        ✅     -      ✅
Update relay config         -      -      -          -         ✅    ✅      ✅
Issue relay key             -      -      -          -         ✅    ❌      ✅
Issue admin key             -      -      -          -         ✅    -       ✅
Register as relay           ❌     ❌     ✅         ✅        ✅    ✅      ✅
```
*Guest accounts can be blacklisted/whitelisted/kicked/suspended/banned by Admins.
Trial keys expire naturally (hourly reset) and cannot be moderated.

### 4.2 Permission Checking Logic

```rust
fn check_permission(world_state: &WorldState, key_registry: &KeyRegistry, op: &SignedOperation) -> PermissionResult {
    // 1. Parse and validate the KeyRecord
    let record = key_registry.get(&op.author)?;
    if record.revoked {
        return PermissionResult::Revoked;
    }
    if record.expires_at.map(|e| e < op.timestamp).unwrap_or(false) {
        return PermissionResult::Expired;
    }

    // 2. Verify signature (the private key actually signed this op)
    if !verify_signature(&record.public_key, &op.payload_bytes(), &op.signature) {
        return PermissionResult::InvalidSignature;
    }

    // 3. Check key type allows this operation class
    if !record.key_type.allows(op.action.class()) {
        return PermissionResult::KeyTypeNotAllowed;
    }

    // 4. Check ownership / access rights for spatial operations
    match &op.action {
        Action::SetVoxel { coord, .. } | Action::PlaceObject { coord, .. } => {
            let zone = world_state.get_zone(coord);
            match zone {
                Zone::FreeBuild => PermissionResult::Allowed,
                Zone::OwnedParcel(owner) => {
                    if owner == op.author || world_state.has_access(op.author, coord) {
                        PermissionResult::Allowed
                    } else {
                        PermissionResult::NotOwner
                    }
                }
                Zone::AdminRestricted(admin) => {
                    if record.key_type == KeyType::Server || admin == op.author {
                        PermissionResult::Allowed
                    } else {
                        PermissionResult::Restricted
                    }
                }
            }
        }
        // ... other action types
    }
}
```

### 4.3 Toggleable Enforcement (Development Phases)

```rust
pub struct PermissionConfig {
    pub verify_signatures: bool,   // default: true
    pub verify_key_types: bool,    // default: true in prod, false in dev
    pub verify_ownership: bool,    // default: true in prod, false in dev
    pub verify_revocation: bool,   // default: true
}
```

Each flag can be disabled independently for testing. Production always has all enabled.

---

## 5. Key Registry — The P2P Database

### 5.1 Architecture

The key registry is distributed across the network. There is no master database.

```
New KeyRecord published
        │
        ▼
Gossipsub "key-registry" topic
  ├─ Server nodes receive → store in local DB → re-serve via DHT
  ├─ Relay nodes receive → forward only (don't store)
  └─ Client nodes receive → cache in memory + ~/.metaverse/key_cache/

DHT (Kademlia) — keyed by PeerId
  ├─ Servers advertise as providers for keys they've seen
  └─ Anyone can query: "give me KeyRecord for PeerId X"

Local cache: ~/.metaverse/key_cache/<peer_id_prefix>/<peer_id>.keyrec
  ├─ Used when offline or DHT lookup fails
  └─ Stale cache is acceptable (worst case: use slightly old display name)
```

### 5.2 Propagation Flow — New Key Registration

```
1. User generates keypair locally
2. Client constructs KeyRecord, signs it (self_sig)
3. Client publishes to gossipsub "key-registry" topic
4. Any connected server receives it:
   a. Verifies self_sig
   b. Stores record in server's key DB
   c. Advertises as DHT provider for this PeerId
5. Other peers receive it:
   a. Verify self_sig
   b. Cache in local key_cache/
6. Later: offline peer connects → server pushes KeyRecord to them
7. Fully propagated when: available via DHT lookup from any peer
```

### 5.3 Lookup Flow — Finding Someone's Key

```
Peer A needs KeyRecord for Peer B:
  1. Check in-memory cache → found? return immediately
  2. Check ~/.metaverse/key_cache/ → found? return (may be stale)
  3. Query DHT: get_providers(peer_b_id) → get list of servers that have it
  4. Connect to one server → fetch KeyRecord → verify self_sig → cache it
  5. Not found anywhere? → treat as Guest (minimum permissions)
```

### 5.4 Server Key Storage

Servers maintain a persistent database of all KeyRecords they've seen:
- SQLite at `~/.metaverse/key_registry.db`
- Index on: peer_id, key_type, created_at
- Never delete records (even revoked ones — revocation needs to propagate)
- Serve any record on request via `/api/keys/<peer_id>`

---

## 6. Signup Flows

### 6.1 Trial (Zero-friction, no registration)

```
1. Player arrives in the lobby for the first time
2. Chooses "Try It Now" — no form, no email
3. Ed25519 keypair generated locally using OsRng
4. KeyRecord: key_type=Trial, no display_name, expires_at=now+1hour
5. NOT published to network — entirely local, ephemeral
6. Player can walk, observe, use predefined chat, accept items handed to them
7. After 1 hour: key expires, player is returned to the lobby for a fresh key
```

### 6.2 Guest (Free account — verified email)

```
1. Player chooses "Free Account" in the lobby UI
2. Enters email address + chosen nickname
3. Ed25519 keypair generated locally using OsRng
4. KeyRecord: key_type=Guest, display_name=<nickname>, expires_at=now+30days(inactivity)
5. Email verification sent (out of band — future implementation)
6. Signed and published to gossipsub → propagates to DHT
7. Home plot assigned
8. Player can: public text chat, build in free zones, watch free content, receive items
9. Account has standing — can be blacklisted/whitelisted/kicked/suspended/banned
10. Minimum 30 days good standing before eligible for User upgrade
```

### 6.3 User (Full account — earned)

```
1. Player is eligible if:
   a. Has been a Guest for 30 days in good standing, OR
   b. Invited by an existing User (each invite -5 days off wait), OR
   c. Invited by an Admin (bypasses wait entirely)
2. Chooses "Full Account" — enters display_name (required)
3. Additional ID verification (out of band — future implementation)
4. Optional: address verification to tie virtual home to real-world address
5. KeyRecord: key_type=User, display_name=<chosen>, no expiry
6. Signed and published to gossipsub → propagates to DHT
7. UI prompts: "Back up your identity file now. There is no recovery."
8. All features unlocked: DMs, commerce, contracts, parcel ownership, full build rights
```

### 6.4 Business (Organisation — created under a User account)

```
1. Existing User chooses "Create Business" from their account settings
2. Generates a new keypair for the business identity
3. display_name is the organisation name
4. KeyRecord: key_type=Business, issued_by=<user_peer_id>
5. User's key countersigns the Business key record
6. User can then add admins/mods via signed access grants
7. Future: multi-sig threshold to prevent single-point-of-failure
```

### 6.5 Relay Key (Hardware role)

```
1. Operator generates a relay keypair locally
2. Sends their public key to a server operator (out of band)
3. Server operator signs a relay KeyRecord as issued_by=<server_peer_id>
4. Signed record delivered back to relay operator
5. Relay operator saves as ~/.metaverse/relay.key
6. Relay starts, advertises with relay KeyRecord
7. Network recognises relay as trusted routing infrastructure
```

### 6.6 Key Backup and Portability

```bash
# Backup (copy the file — that's it)
cp ~/.metaverse/identity.key /safe/backup/location/

# Restore on new machine
cp /backup/identity.key ~/.metaverse/identity.key

# Use a different identity (personas)
METAVERSE_IDENTITY_FILE=~/.metaverse/work.key ./bin/metaworld_alpha

# Export for hardware wallet (future)
metaworld_alpha --export-key-qr  # generates QR code of key bytes
```

---

## 7. Signed Operation Framework

Every mutable action in the game creates a `SignedOperation`. This is not optional.
Even in single-player offline mode, ops are signed — because when you reconnect,
other peers need to verify you authored your own changes.

### 7.1 Core Structure

```rust
pub struct SignedOperation {
    // ── What happened ──────────────────────────────────────
    pub action: Action,

    // ── When ──────────────────────────────────────────────
    pub timestamp: u64,          // Unix millis (wall clock, not authoritative)
    pub lamport: u64,            // Lamport clock (causal ordering)
    pub vector_clock: VectorClock,

    // ── Who ───────────────────────────────────────────────
    pub author: PeerId,          // Derived from public key

    // ── Proof ─────────────────────────────────────────────
    pub signature: [u8; 64],     // Ed25519 signature over canonical bytes of above

    // ── Metadata ──────────────────────────────────────────
    pub op_id: [u8; 16],         // UUIDv4 or hash — globally unique op identifier
    pub chunk_id: ChunkId,       // Which chunk this op applies to
}

pub enum Action {
    // Terrain
    SetVoxel { coord: VoxelCoord, material: Material },
    RemoveVoxel { coord: VoxelCoord },
    FillRegion { bounds: Bounds3D, material: Material },

    // Objects
    PlaceObject { coord: VoxelCoord, object_type: ObjectType, config: ObjectConfig },
    RemoveObject { object_id: ObjectId },
    MoveObject { object_id: ObjectId, new_coord: VoxelCoord },
    ConfigureObject { object_id: ObjectId, config_delta: ConfigDelta },

    // Land
    ClaimParcel { bounds: ParcelBounds },
    AbandonParcel { parcel_id: ParcelId },

    // Ownership / Access
    TransferOwnership { item: ItemRef, to: PeerId },
    GrantAccess { scope: AccessScope, to: PeerId, permissions: PermissionSet },
    RevokeAccess { scope: AccessScope, from: PeerId },

    // Commerce
    CreateListing { item: ItemRef, price: Price, terms: SaleTerms },
    AcceptListing { listing_id: ListingId },          // buyer signs
    CancelListing { listing_id: ListingId },          // seller signs
    SignContract { contract_id: ContractId },

    // Content
    PublishBlueprint { blueprint: Blueprint },
    ImportAsset { asset_hash: [u8; 32], source: AssetSource },

    // Identity
    PublishKeyRecord { record: KeyRecord },
    RevokeKey { target: PeerId, reason: Option<String> },

    // Infrastructure
    RegisterRelay { config: RelayConfig },
    UpdateRelayConfig { config: RelayConfig },
}
```

### 7.2 Every Op is Immutable Once Signed

- An op is created, signed, broadcast, and then **never changed**
- Corrections are new ops (e.g., remove the voxel you just placed)
- The op-log is append-only — this enables distributed conflict resolution

---

## 8. Security Model

### 8.1 What You Can't Fake

| Attack | Why It Fails |
|---|---|
| Forge someone's signature | Ed25519 — computationally infeasible without private key |
| Claim someone's parcel | Ownership is derived from signed op-log; first valid claim wins |
| Replay an old op | Lamport clock + op_id prevent exact replays |
| Upgrade your own key type | Key type is in self-signed KeyRecord; type changes require new keypair |
| Revoke someone else's key | Only self-revoke or authority key with countersignature |
| Man-in-the-middle identity | PeerId derived from public key; transport encrypted |

### 8.2 What the System Accepts

- **Self-declared identity** — display names are not verified (same as any game)
- **Multiple keys per person** — this is by design (personas are valid)
- **Lost keys are gone** — there is no recovery path. This is intentional.
- **Key spam** — anyone can generate infinite keys; mitigation is rate-limiting
  new KeyRecord publications on servers, and Guest keys carrying no permissions

### 8.3 Key Revocation

```rust
// Self-revoke (I lost control of my key, or I'm abandoning this identity)
let revocation = SignedOperation {
    action: Action::RevokeKey {
        target: my_peer_id,
        reason: Some("Key compromised".to_string()),
    },
    author: my_peer_id,
    // ... signed by my own key
};

// Authority revoke (server key revokes a bad actor's key in a region)
let revocation = SignedOperation {
    action: Action::RevokeKey {
        target: bad_actor_peer_id,
        reason: Some("Repeated violation of region rules".to_string()),
    },
    author: server_peer_id,
    // ... signed by server key
};
```

Revocation propagates the same way as KeyRecord registration — via gossipsub and DHT.
Revoked keys stay in the registry (marked `revoked: true`) so peers know they're invalid.

### 8.4 Protecting the Key File

- Key file stored with `chmod 600` (owner read-only)
- Future: optional AES-256-GCM encryption with user passphrase
- Future: hardware wallet support (YubiKey / hardware signing device)
- Warned at startup if key file has loose permissions

---

## 9. Implementation Phases

### Phase 1 — Key Record & Registry Foundation
Extend `src/identity.rs`:
- Add `KeyType` enum
- Add `KeyRecord` struct with `self_sig`
- Implement `KeyRecord::new()`, `sign()`, `verify()`
- Implement `KeyRecord::to_canonical_bytes()` for deterministic signing

Create `src/key_registry.rs`:
- `KeyRegistry` struct (in-memory + disk cache)
- `publish(record)` — broadcast via gossipsub
- `lookup(peer_id)` — check memory → disk → DHT
- `load_cache()` / `save_cache()`
- `apply_update(record)` — merge newer record over older one

Wire into `src/multiplayer.rs`:
- On connect: publish own KeyRecord to "key-registry" topic
- On receive "key-registry" message: call `registry.apply_update()`
- Expose `KeyRegistry` on `Client` struct

### Phase 2 — Signed Operations
Extend `src/messages.rs` / `src/user_content.rs`:
- Define full `Action` enum (all operation types)
- Define `SignedOperation` struct
- Implement `sign(identity)` and `verify(registry)` on `SignedOperation`
- Replace current `VoxelOperation` with `SignedOperation`

### Phase 3 — Permission Checking
Create `src/permissions.rs`:
- `PermissionConfig` (toggleable flags)
- `check_permission(world_state, registry, op)` → `PermissionResult`
- Wire into op application in `src/user_content.rs`

### Phase 4 — Server Key Registry
Extend `examples/metaverse_server.rs`:
- SQLite-backed key registry (`key_registry.db`)
- REST endpoint: `GET /api/keys/<peer_id>`
- REST endpoint: `GET /api/keys?type=relay` (list by type)
- On gossipsub "key-registry" message: store in DB
- Serve stored records to new peers on connect

### Phase 5 — Signup UI
Client-side UI flow (after core engine is more stable):
- Guest auto-generation on first run
- "Create Account" dialog (display name, key type)
- "Back up your key" prompt with file picker
- Key type display next to player name in-world
- Settings page: view key info, export, switch identity

---

## 10. Files to Create / Modify

| File | Change |
|---|---|
| `src/identity.rs` | Add `KeyType`, `KeyRecord`, canonical signing |
| `src/key_registry.rs` | **New** — P2P key registry, DHT lookup, disk cache |
| `src/permissions.rs` | **New** — Permission checking, action classes, `PermissionConfig` |
| `src/messages.rs` | Add `KeyRegistryMessage` gossipsub message type |
| `src/user_content.rs` | Replace `VoxelOperation` with `SignedOperation`, add all `Action` variants |
| `src/lib.rs` | Export `key_registry`, `permissions` modules |
| `examples/metaverse_server.rs` | SQLite key DB, `/api/keys/` endpoints |
| `src/multiplayer.rs` | Publish KeyRecord on connect, handle key-registry topic |
| `docs/IDENTITY_SYSTEM.md` | This document |

---

## 11. What This Unlocks

Once Phase 1-3 are complete:
- **Parcel ownership** works (signed claims, checked on every build op)
- **Trading** works (signed transfers, both parties sign)
- **Permanent buildings** — gated behind Personal key (not Guest)
- **Access grants** — let friends build in your home
- **Ban/kick** — servers can revoke region access
- **Commerce** — list items, accept listings, sign contracts
- **Script deployment** — runs under author's key authority
- **Governance** — region votes, weighted by stake

The key is the unlock mechanism for every one of these systems.
Without a solid key system, NONE of those features can be built correctly.

---

*Document status: Design complete, implementation not started*
*Next action: Phase 1 implementation — extend identity.rs, create key_registry.rs*
