# Decentralised Platform — Complete Vision

## READ THIS FIRST

> The servers don't just host the game. They host everything.
> Forums, wikis, blogs, the marketplace, identity verification, payment records —
> all of it sharded, replicated, and encrypted across every participating node.
> No single company owns it. No single server holds it. It exists everywhere
> and nowhere at the same time. Take 90% of the nodes offline and the remaining
> 10% still serve it all.

This document covers:
1. The decentralised web platform (forums, wiki, blog, marketplace)
2. Real-world identity verification tiers — what's actually required per key type
3. How sensitive data (verification docs, payment info) is sharded and encrypted
4. How it all connects to the key system

---

## 1. The Platform Is the Game, and the Game Is the Platform

The metaverse is not just a 3D world. It's a complete platform:

```
┌─────────────────────────────────────────────────────────────────┐
│                      The Metaverse Platform                      │
├─────────────────┬───────────────────┬───────────────────────────┤
│   3D World      │   Web Platform    │   Infrastructure          │
│─────────────────│───────────────────│───────────────────────────│
│ Terrain         │ Forums            │ Relay nodes               │
│ Voxel building  │ Wiki / docs       │ Server nodes              │
│ Objects         │ Blogs             │ DHT (Kademlia)            │
│ Physics         │ Marketplace       │ Gossipsub                 │
│ Parcels         │ User profiles     │ Content-addressed storage │
│ Scripts         │ Groups / orgs     │ Sharded encrypted data    │
│ Governance      │ Voting / polls    │ Bootstrap mesh            │
└─────────────────┴───────────────────┴───────────────────────────┘

ALL of it shares the same key system. Your identity in the forum IS your
identity in the world. Your marketplace listing IS a signed op. Your wiki
edit IS a signed op. One key. Everything signed.
```

The web platform and the 3D world are two interfaces to the same underlying
signed op-log. A forum post is a `SignedOperation { action: Action::PostContent }`.
A parcel claim is a `SignedOperation { action: Action::ClaimParcel }`. Same
infrastructure, same trust model, same replication.

---

## 2. The Decentralised Web — How It Works

### 2.1 Architecture (No Central Server)

```
Traditional web:           This platform:
───────────────            ──────────────
Browser → CDN → Server     Browser → Any Node → Content
                           (content is the same on all nodes)

The "website" is not on a server.
The "website" is the current state of the replicated content store,
reconstructed on-the-fly by whichever node you connect to.
```

Every node (server, relay, even clients) participates in serving content.
A forum post written in Sydney is readable from a node in London within seconds —
not because it was sent to London specifically, but because London's node
subscribes to the content gossipsub topics and caches what it receives.

### 2.2 Content Addressing

Every piece of content is identified by its hash, not its location:

```
Forum post by PaddyOh
  → SHA-256(canonical_bytes(post)) = abc123...
  → Stored at key abc123... on every node that received it
  → Retrievable from ANY node: "give me content abc123..."
  → If the node doesn't have it, it asks the DHT: "who has abc123?"
  → Any node that has it can serve it
  → Signature verifies it's authentic regardless of who served it
```

Content can't be tampered with in transit — the hash is the address, and
the hash won't match if the content was modified.

### 2.3 What "90% Offline" Means

If 90% of all nodes disappear right now:
- The remaining 10% have cached copies of all recently active content
- New content can still be created and propagates to whoever is online
- When nodes come back online they sync from peers (same CRDT merge as terrain)
- Nothing is "lost" as long as at least one node has a copy
- The system degrades gracefully: slower propagation, smaller audiences, but
  everything still works

This is not theoretical — it's the same model that makes BitTorrent and IPFS
resilient. We're just applying it to a richer content type.

### 2.4 Content Types on the Platform

| Content Type    | Action                  | Who Can Post       | Persisted By     |
|-----------------|-------------------------|--------------------|------------------|
| Forum post      | PostContent             | Anonymous+         | All nodes        |
| Forum reply     | PostReply               | Anonymous+         | All nodes        |
| Wiki page       | PublishWikiPage         | Personal+          | All nodes        |
| Wiki edit       | EditWikiPage            | Personal+          | All nodes        |
| Blog post       | PublishBlogPost         | Personal+          | Author's peers   |
| Marketplace listing | CreateListing       | Personal+          | All nodes        |
| Marketplace offer   | AcceptListing       | Personal+          | All nodes        |
| User profile    | UpdateProfile           | All (own profile)  | All nodes        |
| Group / org     | CreateGroup             | Personal+          | All nodes        |
| Poll / vote     | CreatePoll / CastVote   | Personal+          | All nodes        |
| Report / flag   | ReportContent           | Guest+             | Server nodes     |
| Moderation act  | ModerateContent         | Admin+             | Server nodes     |

### 2.5 The Web Interface

The web interface is served by any node you connect to. It's a static app
(HTML/JS/WASM) that talks to the local node's API. The app itself is also
content-addressed and served from the network:

```
User opens browser → navigates to any relay/server address
  → Node serves the web app (static files, content-addressed)
  → App connects to that node's local API (same REST endpoints as the game uses)
  → App displays forum/wiki/marketplace/profile from that node's local content store
  → App signs any new content with user's key (key lives in browser local storage
    or hardware wallet / native app bridge)
```

The "website" has no canonical domain that can be taken down. Any node is an
access point. Nodes are listed in the bootstrap Gist (same one the game uses).
Communities can run their own nodes and provide access to their members.

---

## 3. Real-World Verification Tiers

### 3.1 The Core Problem

You cannot give an Admin key to a random stranger. You cannot give a Business
key to someone who won't be accountable for what their "business" does in the world.
But you also can't require a passport scan to play a game as a guest.

**Solution: tiered verification matched to capability.**

The higher the capability, the more verification required. Verification is
handled through the platform itself — forms, documents, and payment details
stored encrypted and sharded, reviewed by trusted operators.

### 3.2 Verification Tiers

#### Tier 0: None (Guest, Anonymous)

```
Required:  Nothing
Process:   Auto-generated (Guest) or one-click (Anonymous)
Trust:     Zero real-world trust. Pseudonymous. Ephemeral (Guest) or persistent (Anon).
Limits:    No property. No contracts. No commerce. No permanent builds.
Use case:  Try the game. Read forums. Post in open areas. Report content.
```

**The game is fully playable at Tier 0.** You just can't own anything.

#### Tier 1: Email Verification (Personal)

```
Required:  Valid email address
Process:   Enter email → receive verification link → click → verified
           Email is hashed and stored encrypted — not plaintext in the DB
Trust:     Proves you control an email address. Light accountability.
           Not government ID. Not your real name. Just an email.
Limits:    Standard gameplay. Can own parcels, trade, build permanently.
Use case:  The default for regular players.
```

Email is the minimum bar because:
- It prevents trivial key spam (one email per Personal key)
- It provides a recovery path for banned accounts (can contact operator)
- It does not reveal your real identity unless you choose to (use a throwaway)

**Important:** The email is NOT stored on any single server. It is encrypted
with the user's own public key and then sharded (see Section 4). An operator
can request the decryption key from the user if needed for account recovery,
but cannot read it without the user's cooperation.

#### Tier 2: Phone Verification (Business)

```
Required:  Valid phone number (SMS verification)
Process:   Enter phone → receive SMS code → enter code → verified
           Phone number hashed and stored encrypted+sharded
Trust:     Stronger than email. Harder to abuse at scale.
           Not your real name. Just a phone.
Limits:    Full commerce capabilities. Can own regions. Can employ (delegate) staff.
Use case:  Shops, studios, guilds, organisations operating in-world.
```

Business keys are for entities that will conduct commerce and own significant
property. SMS verification is a meaningful friction point — harder to fake
at scale than email, low friction for legitimate operators.

#### Tier 3: Identity Verification (Admin)

```
Required:  Government-issued ID document
           Operator review (a human reads and approves)
Process:   Submit document scan via encrypted form
           Document encrypted with operator's server key + applicant's key
           Document sharded across N server nodes (no single server has full doc)
           Operator reviews their shard context, approves or denies
           Once approved, Admin KeyRecord issued (see relay key flow)
Trust:     Real identity behind the key. Accountable person.
Limits:    Region moderation powers. Can kick, ban, moderate content.
Use case:  Community moderators. Regional governance. Trusted contributors.
```

Admin keys have power over other people (ban/kick/moderate). That power requires
accountability. The operator is taking on responsibility when they approve an Admin.

**The document is never stored in full on any single node.** Shamir's Secret
Sharing (or simpler: split into N chunks, each encrypted, each stored on a
different server). An operator cannot access the document alone — it requires
the applicant's cooperation OR a quorum of N server operators (for legal requests).

#### Tier 4: Business Registration (Business — elevated tier)

```
Required:  Registered company/organisation documents
           ABN, company number, or equivalent
           Primary contact (Tier 3 verified person)
Process:   Same sharded document storage as Tier 3
           Server operator reviews and approves
Trust:     Legal entity. Holds legal liability for in-world actions.
Limits:    Large parcel ownership. Official brand status. Can issue staff keys.
Use case:  Real companies, studios, brands operating in the metaverse.
```

#### Tier 5: Operator Trust (Server Key, Relay Key)

```
Required:  Known to the Genesis Key holder or existing Server Key holder personally
           Track record (prior verified participation in the network)
Process:   Direct relationship with issuing operator
           No automated process — pure human trust decision
Trust:     Highest tier. Infrastructure operator. Accountable to the network.
Limits:    Relay Key: routing trust. Server Key: world-state authority.
Use case:  Community members running infrastructure for the network's benefit.
```

### 3.3 Verification Matrix

```
Key Type     Tier   What's Required                      Process
──────────────────────────────────────────────────────────────────────────
Guest        0      Nothing                               Auto-generated
Anonymous    0      Nothing                               One-click
Personal     1      Email address                         Email verify link
Business     2      Phone number                          SMS code
Admin        3      Government ID + operator approval     Document + human review
Business★    4      Company registration + Tier 3 person  Documents + human review
Relay Key    5      Known to server operator personally   Direct relationship
Server Key   5      Known to Genesis holder personally    Direct relationship
```

★ Business (elevated) — for large-scale commercial operators.

### 3.4 How Verification Interacts With the Key System

Verification doesn't happen IN the key — the key is just a keypair. Verification
creates a **VerificationRecord** linked to the key:

```rust
pub struct VerificationRecord {
    pub peer_id:          PeerId,         // who this verifies
    pub tier:             VerificationTier,
    pub verified_at:      u64,            // unix millis
    pub verified_by:      PeerId,         // which server/operator verified
    pub verifier_sig:     [u8; 64],       // operator signs this record
    pub evidence_hash:    [u8; 32],       // SHA-256 of sharded evidence (not the evidence itself)
    pub evidence_shards:  u8,             // how many shards the evidence was split into
}

pub enum VerificationTier {
    None     = 0,
    Email    = 1,
    Phone    = 2,
    Identity = 3,
    Business = 4,
    Operator = 5,
}
```

The VerificationRecord is public (it proves you passed a tier without revealing
HOW you proved it). The evidence itself is encrypted and sharded — the record only
contains the hash of the evidence bundle.

Permissions for a given action check BOTH:
1. `key_type` — what the key is capable of
2. `verification_tier` — what the user has proved about themselves

A Business key with Tier 2 (phone) gets standard business capabilities.
A Business key with Tier 4 (company docs) gets elevated commercial capabilities
(larger parcels, official brand badge, etc.).

---

## 4. Sharded Encrypted Data — How Sensitive Info Is Stored

### 4.1 The Problem

User verification documents, payment details, and contact information must:
- Be verifiable by operators when needed
- Be inaccessible to any single party without authorisation
- Survive node failures (can't be lost if a server goes offline)
- Be deletable on user request (GDPR / right to erasure)
- Not be readable by attackers who compromise a single node

### 4.2 The Solution: Encrypt → Shard → Scatter

```
User submits evidence (e.g., passport scan, email address, phone number)

Step 1: ENCRYPT
  User's browser/client encrypts the evidence with:
  a) User's own public key  (user can always recover their own data)
  b) Operator's server public key  (operator can review for verification)
  Combined: encrypt(evidence, [user_pubkey, operator_pubkey])
  → encrypted_blob (only these two keys can decrypt)

Step 2: SHARD
  encrypted_blob → split into N shards using Shamir's Secret Sharing
  (or simpler: split into N equal chunks)
  Each shard is independently encrypted with a random ephemeral key
  The ephemeral keys are stored separately from the shards
  
  N = 5 shards, threshold = 3 (any 3 of 5 reconstruct the blob)

Step 3: SCATTER
  Shard 1 → Server AU (Sydney)
  Shard 2 → Server EU (London)
  Shard 3 → Server US (New York)
  Shard 4 → Server SEA (Singapore)
  Shard 5 → Trusted peer node (high uptime, Tier 5 verified)

Step 4: RECORD
  VerificationRequest stored in the public key registry:
    peer_id:       <applicant>
    evidence_hash: SHA-256(encrypted_blob)
    shard_count:   5
    threshold:     3
    shard_nodes:   [list of PeerIds holding shards]
    submitted_at:  <timestamp>
    status:        Pending | Approved | Denied | Withdrawn

Step 5: OPERATOR REVIEW
  Operator queries their shard from their node
  Combines with 2 other shards (requests from other server operators)
  Decrypts with their server key → reads the evidence
  Approves or denies → signs a VerificationRecord
  VerificationRecord propagates via gossipsub (same as KeyRecord)
```

### 4.3 Deletion (Right to Erasure)

The user can instruct all shard-holding nodes to delete their shards:
- `Action::DeleteVerificationData { peer_id, evidence_hash }` — signed by user
- Each shard-holding node receives this op and deletes their shard
- The VerificationRecord remains (it's public proof of past verification)
  but the evidence is gone — no server can reconstruct it
- The user's public key in the VerificationRecord still proves they were
  verified at that tier at that time (historical fact), but no personal data remains

If a user deletes their evidence AND the VerificationRecord is revoked,
they must re-verify to get that tier back.

### 4.4 Payment Information

Payment details (for in-world commerce, marketplace, or subscription features)
follow the same shard model:

```
Payment info encrypted + sharded across 5+ nodes.
No single node holds a complete payment record.
Payment processor (Stripe / crypto / whatever) handles the actual charge —
the platform stores only:
  - Hashed card identifier (last 4 digits + hash of full number)
  - Proof of payment (transaction ID, signed by payment processor)
  - Whether the user has an active subscription tier

The platform NEVER stores raw card numbers — those are handled by the
payment processor. What's stored and sharded is: proof that payment occurred,
and the subscription/capability status that results from it.
```

---

## 5. The Reputation System (Connected to Verification)

Verification tier is the MINIMUM trust level. On top of it, reputation
accumulates through actions:

```rust
pub struct ReputationRecord {
    pub peer_id:           PeerId,
    pub total_ops:         u64,      // total signed ops (builds, posts, trades)
    pub age_days:          u32,      // how long this key has been active
    pub trade_volume:      u64,      // value of completed contracts
    pub report_count:      u32,      // times reported (by others)
    pub ban_count:         u32,      // times banned from regions
    pub endorsements:      Vec<PeerId>, // other keys vouching for this one
    pub computed_score:    f32,      // composite — computed, not stored directly
}
```

Reputation is COMPUTED from the op-log — not self-reported. You can't fake it.
You earn it by doing things in the world over time, and you lose it by being
reported and banned. It's a signal layered on top of verification.

---

## 6. Moderation — Report, Ban, Invite, Message

These all work through the key system. A player you can't look up (no KeyRecord)
is a player you can't interact with. This is why all key types are published.

### 6.1 Report

```rust
Action::ReportContent {
    target_peer_id: PeerId,
    content_op_id:  [u8; 16],  // the op being reported
    reason:         ReportReason,
    description:    Option<String>,
}
```

Any key type (including Guest) can file a report. Report is signed and sent
to the nearest Admin key holder for the region. Admins aggregate reports.
Patterns of reports trigger review. Single reports go into a queue.

### 6.2 Ban

```rust
Action::BanFromRegion {
    target_peer_id: PeerId,
    region_id:      RegionId,
    duration:       BanDuration,  // Temporary | Permanent
    reason:         String,
}
```

Only Admin+ keys can issue region bans. Server keys can issue global bans.
Ban is a signed op — propagates via gossipsub, enforced by all nodes in that region.
Banned player's ops for that region are silently dropped at the network level.

### 6.3 Invite

```rust
Action::InviteToGroup {
    target_peer_id: PeerId,
    group_id:       GroupId,
    role:           GroupRole,
    message:        Option<String>,
}
```

Invite is routed directly to the target's known peer addresses (direct message
over gossipsub "dm/<peer_id>" topic, or stored on a server for offline delivery).

### 6.4 Message

Direct messages are signed ops routed via dedicated gossipsub topics:

```
Topic: "dm/<sha256(sorted_pair_of_peer_ids)>"

Sender subscribes to this topic, sends encrypted message:
  plaintext → encrypt(recipient_public_key) → SignedOperation
  
If recipient is offline: message stored on nearest server, 
delivered on next connection (same "store-and-forward" model as email).
```

Messages are end-to-end encrypted — servers route and store them but
cannot read them.

### 6.5 Display Names — Nickname Policy

Display names are self-declared nicknames. Not unique. Not verified.
The Key ID is the ground truth identity.

**The only enforcement:** Admins and Server operators can flag a display name
as "Reserved" or "Prohibited" via a signed op:

```rust
Action::ReserveDisplayName {
    name:   String,       // reserved for official use (e.g. "Metaverse Team")
}
Action::ProhibitDisplayName {
    name:   String,       // blocked — vulgar, slur, impersonation of official name
    reason: String,
}
```

These ops propagate and are enforced at the display-name-render layer:
- Reserved names show with a special badge (only the key that reserved them
  shows without the [RESERVED] warning)
- Prohibited names are shown as [BLOCKED NAME] to all peers
- No ban required — just the name is blocked, the key still functions normally

---

## 7. The Full Platform Stack

```
┌──────────────────────────────────────────────────────────────────────┐
│  CLIENT INTERFACES                                                    │
│  ─────────────────                                                    │
│  3D Game (Rust/WGPU)    Web Browser    Mobile App    CLI Tools       │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │ same REST + gossipsub API
┌────────────────────────────────▼─────────────────────────────────────┐
│  NODE LAYER (server / relay / client — all participate)              │
│  ─────────────────────────────────────────────────────               │
│  Content Store (content-addressed, IPFS-like)                        │
│  Key Registry (gossipsub + DHT + SQLite)                             │
│  Verification Records (public) + Evidence Shards (encrypted)        │
│  Signed Op Log (terrain, forum posts, marketplace, everything)       │
│  DHT (Kademlia) — routing, discovery, record storage                 │
│  Gossipsub — real-time broadcast of all event types                  │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
┌────────────────────────────────▼─────────────────────────────────────┐
│  PERSISTENCE LAYER                                                    │
│  ──────────────────                                                   │
│  Server nodes:  SQLite (key_registry.db, content.db, ops.db)        │
│  Relay nodes:   Forward only — no persistent storage (by default)   │
│  Client nodes:  Local files — chunks, key cache, op logs            │
│  DHT:           Distributed — any node holds any subset of records  │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 8. Implementation Phases for the Platform

This is a multi-year roadmap. Phases are sequenced so each builds on the last.

### Phase A — Key + Signup (now building)
Core identity, all key types published, verification tier stubs

### Phase B — Direct Messaging
Encrypted DMs, offline delivery via server store-and-forward

### Phase C — User Profiles + Reports
Public profile pages, report/ban/moderation actions

### Phase D — Forum / Wiki
Forum posts, wiki pages, content-addressed storage, web interface

### Phase E — Marketplace
Listings, offers, contracts, trade history — all signed ops

### Phase F — Verification Infrastructure
Email/phone verification flows, sharded encrypted evidence storage

### Phase G — Decentralised Web Serving
Content-addressed static site delivery, any node serves the web app

### Phase H — Blog / Social Features
Blogs, following, feeds, endorsed posts, community spaces

### Phase I — Governance
Region proposals, voting, weighted stake, proposal execution

### Phase J — Full Commerce
Subscriptions, payment integration, business accounts, multi-sig

---

## 9. Open Design Questions for Discussion

**9.1 Shard Count and Threshold**
How many shards for verification evidence? 5-of-3? 7-of-4?
More shards = more resilient but more coordination for operator review.
Start with 3-of-2 (simple) and increase as the server network grows.

**9.2 Verification Payment**
Should Personal key email verification be free forever?
Or is there a small fee (crypto microtransaction) at the Business tier and above
to prevent spam account creation?

**9.3 Who Runs the Verification Process**
Initially: you (PaddyOh) are the only operator, so you review Admin and Business
applications manually. As the network grows, trusted server operators can run
their own verification queues for their regions.

**9.4 Web Interface Technology**
WASM app in the browser (Rust → WASM) for consistency with the game client,
OR standard HTML/JS React app that talks to the same REST API?
WASM is consistent but more work. React is faster to ship.

**9.5 Store-and-Forward Message Retention**
How long does a server hold an undelivered DM? 30 days? 90 days?
After that: message expires, sender is notified (if they're still online).

---

*Status: Vision document — captures the full scope*
*Author: Copilot (structure/technical), PaddyOh (vision/direction)*
*This doc should be updated as each phase moves from vision to design to implementation*
