# Metaverse Platform REST API — Complete Design

## Core Principle

This API is the backbone that every interface uses — the meshsite (web),
the game client, other servers (sync), and operator tooling. There is no
separate "CLI path" or "server-only path." One API. Everything uses it.

Any server node can answer any request. A request that arrives at Server A
will propagate its side-effects (new keys, approvals, verifications) to all
other servers via gossipsub. The client does not need to know which server
it's talking to.

---

## Versioning and Content Negotiation

All endpoints are prefixed `/api/v1/`. Version in the path for strict
forward compatibility — nodes running different versions can coexist.

**Content negotiation (every endpoint supports both):**
```
Accept: application/json          → JSON response (meshsite, browser, debug)
Accept: application/x-bincode     → binary bincode response (game client, efficient)
Accept-Encoding: zstd, gzip       → compression always available
```

Game clients on constrained bandwidth use bincode + zstd. Web clients use JSON.
The server always honours the `Accept` header.

---

## Authentication Model

Three auth levels. No passwords. Everything is key-based.

```
PUBLIC          No auth. Read-only. Anyone.
USER-AUTH       Challenge-response with any valid key.
                Used for: submitting verification, self-revocation.
OPERATOR-AUTH   Challenge-response with this server's server.key.
                Used for: approving requests, revoking others, server config.
```

**Challenge flow:**
```
POST /api/v1/auth/challenge
  Body: { "peer_id": "12D3..." }
  Response: { "nonce": "abc123...", "expires_at": 1234567890 }

POST /api/v1/auth/verify
  Body: {
    "peer_id": "12D3...",
    "nonce": "abc123...",
    "signature": "<Ed25519Sign(private_key, nonce_bytes)>"
  }
  Response: { "token": "...", "expires_at": ... }
  
Bearer token in Authorization header for subsequent requests:
  Authorization: Bearer <token>
```

Tokens are short-lived (1 hour user, 8 hours operator). Stateless JWT-style
but signed with the server's own key — no shared secret, fully verifiable.

---

## 1. Key Registry (Read)

### `GET /api/v1/keys/{peer_id}`
Get a single KeyRecord by PeerId.
```
Response 200: KeyRecord (JSON or bincode)
Response 404: { "error": "not_found" }
```

### `POST /api/v1/keys/batch`
Get multiple KeyRecords in one round-trip. Essential for game clients
joining a session with 20+ peers — one request, not 20.
```
Body:    { "peer_ids": ["12D...", "12D...", ...] }
Response 200: { "12D...": <KeyRecord>, "12D...": <KeyRecord> }
             (missing peer_ids are simply omitted from the map)
```

### `GET /api/v1/keys?type=relay`
List keys by type. Supports: guest, anonymous, personal, business, admin, relay, server, genesis.
```
Query params:
  type=relay                 filter by KeyType
  updated_after=<unix_ms>    incremental sync — only records updated since this timestamp
  limit=<n>                  max results (default 100, max 1000)
  offset=<n>                 pagination

Response 200: { "records": [...], "total": 1234, "has_more": true }
```

### `GET /api/v1/keys/relays`
Shortcut: all active (non-revoked, non-expired) relay keys.
Used by clients to discover trusted relays beyond the bootstrap list.
```
Response 200: { "records": [...] }
```

### `GET /api/v1/keys/servers`
All active server keys. Used by clients to verify server authority and
find other servers for fallback.

---

## 2. Key Registry (Write)

### `POST /api/v1/keys`
Submit a self-signed KeyRecord. Any peer can do this. No auth required.
This is how clients register their own key with the network.

```
Body: KeyRecord (JSON or bincode)

Validation:
  - Verify self_sig (Ed25519 over canonical bytes)
  - Check version field is supported
  - Reject if revoked field is true (can't self-submit a revoked key)
  - Reject if updated_at <= existing record's updated_at (stale)

Response 201: { "status": "accepted" }
Response 400: { "error": "invalid_signature" | "invalid_format" }
Response 409: { "error": "stale", "current_updated_at": 1234567890 }

Side effects:
  - Stored in local SQLite key_registry.db
  - Broadcast via gossipsub "key-registry" topic
  - put_record to Kademlia DHT
  - start_providing on DHT
```

### `PUT /api/v1/keys/{peer_id}`
Update an existing KeyRecord. Must be signed by the same key (self_sig covers
new updated_at). Used to change display_name, bio, avatar, etc.
```
Same as POST /api/v1/keys but requires peer_id to exist.
Response 200: { "status": "updated" }
Response 404: { "error": "not_found" }  (use POST to create first)
```

---

## 3. Key Requests (Relay / Admin / Server Issuance)

This is the ecosystem-wide issuance flow. The applicant submits via the meshsite
or game client. The server operator approves via the meshsite or operator tooling.
The signed result propagates via gossipsub. No single step happens in isolation.

### `POST /api/v1/key-requests`
Submit a request for a countersigned key (Relay, Admin, or Server).
No auth required — but the request is signed by the applicant's own key.

```
Body: {
  "key_type": "relay" | "admin" | "server",
  "applicant_peer_id": "12D3...",
  "applicant_pubkey": "<hex>",
  "display_name": "Alice's Sydney Relay",
  "description": "Home server, 100Mbps fibre, Sydney AU, 24/7 uptime",
  "contact": "alice@example.com",       // encrypted, see below
  "requested_expiry_days": 365,
  "applicant_sig": "<signs all above fields>"  // proves applicant holds the key
}

Contact field:
  The contact info is encrypted with the server's public key before sending.
  Only the server operator can decrypt it. Never stored in plaintext.
  The server stores the encrypted blob — the applicant's contact
  is only decryptable by the operator who processes the request.

Response 201: {
  "request_id": "uuid",
  "status": "pending",
  "message": "Request received. You will be contacted via the provided address."
}
Response 400: { "error": "invalid_signature" | "invalid_key_type" }
Response 429: { "error": "rate_limited" }  // max 3 pending requests per peer_id
```

### `GET /api/v1/key-requests`
List pending key requests. **OPERATOR-AUTH required.**
```
Query params:
  status=pending|approved|denied|all   (default: pending)
  type=relay|admin|server
  
Response 200: {
  "requests": [{
    "request_id": "uuid",
    "key_type": "relay",
    "applicant_peer_id": "12D3...",
    "display_name": "Alice's Sydney Relay",
    "description": "...",
    "contact_encrypted": "<blob>",  // operator decrypts locally with server.key
    "submitted_at": 1234567890,
    "status": "pending"
  }, ...]
}
```

### `GET /api/v1/key-requests/{request_id}`
Get a specific request. **OPERATOR-AUTH required.**

### `POST /api/v1/key-requests/{request_id}/approve`
Approve and countersign a key request. **OPERATOR-AUTH required.**
This is the moment the entire ecosystem comes together to validate the key.

```
Body: {
  "expires_days": 365,
  "notes": "Verified via Discord — known community member"  // internal only
}

Server actions on approval:
  1. Construct KeyRecord for applicant with:
       key_type:    Relay (or Admin/Server)
       issued_by:   this server's peer_id
       issuer_sig:  Ed25519Sign(server.key, canonical_bytes(KeyRecord))
  2. Store in key_registry.db (marked as issued_by this server)
  3. Broadcast via gossipsub "key-registry" topic
  4. put_record to DHT
  5. Make signed .keyrec available for download

Response 200: {
  "status": "approved",
  "download_url": "/api/v1/key-requests/{request_id}/download",
  "keyrec_hash": "<sha256 of the signed .keyrec>"
}

Side effects:
  - Gossipsub broadcast means every connected peer receives the new Relay KeyRecord
    within seconds — no polling required
```

### `POST /api/v1/key-requests/{request_id}/deny`
Deny a request. **OPERATOR-AUTH required.**
```
Body: { "reason": "Unable to verify identity at this time" }
Response 200: { "status": "denied" }
```

### `GET /api/v1/key-requests/{request_id}/download`
Download the signed `.keyrec` file after approval.
Available to anyone who knows the request_id (applicant shares it).
```
Response 200: binary .keyrec file
              Content-Type: application/x-metaverse-keyrec
              Content-Disposition: attachment; filename="relay_12D3....keyrec"
Response 404: not found or not yet approved
```

---

## 4. Verification (Tiered Real-World Identity)

Verification adds a `VerificationRecord` linked to a KeyRecord.
The verification evidence itself is encrypted + sharded — see DECENTRALISED_PLATFORM.md.

### `POST /api/v1/verify/email/start`
Begin email verification. **USER-AUTH required** (must own this peer_id).
```
Body: {
  "peer_id": "12D3...",
  "email_encrypted": "<ChaCha20 encrypted with server pubkey>"
  // Email is never sent or stored in plaintext
}

Server actions:
  1. Decrypt email (only this server can)
  2. Send verification link to that email
  3. Store: pending verification token → peer_id mapping (expires 24h)

Response 200: { "status": "email_sent", "expires_in": 86400 }
```

### `GET /api/v1/verify/email/{token}`
Email verification callback (the link in the email).
```
Server actions on success:
  1. Create VerificationRecord { tier: Email, peer_id, verified_at, verifier_sig }
  2. Store in key_registry.db
  3. Broadcast via gossipsub "key-registry" topic (same topic — everything is a record)
  4. Delete the plaintext email from all storage

Response 200: { "status": "verified", "tier": "email" }
             (or redirect to meshsite success page)
```

### `POST /api/v1/verify/phone/start`
Begin SMS verification. **USER-AUTH required.**
```
Body: { "peer_id": "12D3...", "phone_encrypted": "<encrypted>" }
Response 200: { "status": "sms_sent", "expires_in": 600 }
```

### `POST /api/v1/verify/phone/confirm`
Confirm SMS code. **USER-AUTH required.**
```
Body: { "peer_id": "12D3...", "code": "123456" }
Response 200: { "status": "verified", "tier": "phone" }
Response 400: { "error": "invalid_code" | "expired" }
```

### `POST /api/v1/verify/identity/submit`
Submit encrypted identity document for manual review. **USER-AUTH required.**
```
Body: {
  "peer_id": "12D3...",
  "evidence_bundle_encrypted": "<ChaCha20 + server pubkey>",
  "evidence_hash": "<sha256 of plaintext bundle — for integrity>",
  "shard_count": 5,
  "threshold": 3
  // Server stores its assigned shard, distributes others to known servers
}

Response 202: {
  "status": "submitted",
  "review_estimated": "1-3 business days",
  "reference": "uuid"
}
```

### `POST /api/v1/verify/delete`
Delete all verification evidence (right to erasure). **USER-AUTH required.**
```
Body: { "peer_id": "12D3..." }

Server actions:
  1. Delete evidence shards this server holds
  2. Broadcast DeleteVerificationData op to other shard-holders
  3. VerificationRecord remains (historical fact) but evidence is gone

Response 200: { "status": "evidence_deleted" }
```

---

## 5. Revocation

### `POST /api/v1/keys/{peer_id}/revoke`
Revoke a key. Two cases:

**Self-revocation** (USER-AUTH with the key being revoked):
```
Body: { "reason": "Key compromised, migrating to new identity" }

Creates SignedOperation { action: Action::RevokeKey { target: peer_id, reason } }
signed by the user's own key. Broadcasts via gossipsub.
```

**Authority revocation** (OPERATOR-AUTH):
```
Body: {
  "reason": "Repeated violation of community standards",
  "scope": "global" | "region:<region_id>"
}

Creates SignedOperation signed by server.key. Broadcasts via gossipsub.
Sets revoked=true in key_registry.db.
```

```
Response 200: { "status": "revoked", "propagated": true }
```

---

## 6. Server Information

### `GET /api/v1/server/info`
Public info about this server node.
```
Response 200: {
  "peer_id": "12D3...",
  "version": "0.1.3",
  "key_type": "server",
  "key_record": <KeyRecord>,
  "capabilities": ["key_registry", "verification_email", "meshsite"],
  "connected_peers": 42,
  "keys_stored": 1893,
  "uptime_seconds": 86400
}
```

### `GET /api/v1/server/relays`
All active relay keys this server knows about, with their multiaddrs.
Used by clients to discover relay nodes beyond the bootstrap list.
```
Response 200: {
  "relays": [{
    "peer_id": "12D3...",
    "display_name": "Alice's Sydney Relay",
    "addrs": ["/ip4/203.x.x.x/tcp/4001"],
    "expires_at": 1234567890,
    "issuer": "12D3...Server"
  }]
}
```

### `GET /api/v1/server/stats`
Aggregate statistics. Public.
```
Response 200: {
  "keys_by_type": { "guest": 4821, "personal": 1203, "relay": 12, ... },
  "verifications_by_tier": { "email": 834, "phone": 203, "identity": 41 },
  "pending_requests": 3,
  "last_sync": 1234567890
}
```

---

## 7. Server-to-Server Sync

Servers replicate to each other automatically. These endpoints are used by
the sync process — not typically called by clients.

### `GET /api/v1/sync/keys?since={unix_ms}&limit=1000&offset=0`
Incremental key sync. Returns all KeyRecords this server has received
since the given timestamp.
```
Response 200: {
  "records": [...],
  "has_more": true,
  "next_offset": 1000,
  "server_time": 1234567890
}
```

### `GET /api/v1/sync/verification-records?since={unix_ms}`
Incremental VerificationRecord sync.

### `POST /api/v1/sync/register`
Tell this server about another server (peer introduction).
```
Body: {
  "peer_id": "12D3...",
  "addrs": ["/ip4/..."],
  "server_key_record": <KeyRecord>
}
Response 200: { "status": "registered" }
```

---

## 8. Content (Meshsite — Phase D)

These endpoints support the decentralised web platform. Not built yet —
defined here so the wire format is reserved.

### `POST /api/v1/content`
Submit content-addressed data (forum post, wiki page, blog post, etc.).
```
Body: SignedOperation containing the content action
Response 201: { "content_hash": "<sha256>", "status": "accepted" }
```

### `GET /api/v1/content/{hash}`
Retrieve content by hash. Verifiable: hash of content must match the address.
```
Response 200: content bytes
Response 404: not held by this server (try DHT lookup)
```

### `GET /api/v1/content/search?q={query}&type={type}&limit=20`
Full-text search across content this server has indexed.
```
Response 200: { "results": [{ "hash": "...", "preview": "...", "author": "12D3..." }] }
```

---

## 9. Bandwidth Considerations

The API is designed to work efficiently at all bandwidth tiers:

**Constrained bandwidth (mobile, satellite):**
- Use `Accept: application/x-bincode` — ~40% smaller than JSON
- Use `Accept-Encoding: zstd` — further 50-70% compression
- Use batch endpoints (`POST /api/v1/keys/batch`) — one round-trip, not N
- Use incremental sync (`?updated_after=`) — only fetch what changed

**LoRa / minimal bandwidth:**
- REST API is NOT used over LoRa — too expensive
- Key propagation over LoRa uses gossipsub (presence beacon carries KeyRecord hash)
- Full KeyRecord sync happens when better bandwidth returns
- The API is a "connect when you can" supplement to gossipsub/DHT

---

## 10. Existing Endpoints (Already Implemented — Keep As-Is)

These exist in `metaverse_server.rs` and should be preserved:
```
GET  /                    web root
GET  /health              health check  
GET  /api/status          server status
GET  /api/peers           connected peers
GET  /api/config          server config
GET  /api/keys            list all keys (consolidate with GET /api/v1/keys)
GET  /api/keys/relays     relay keys shortcut
GET  /api/keys/servers    server keys shortcut
GET  /api/keys/:peer_id   get key by peer_id
```

Migration path: keep `/api/keys` working (existing clients use it), add
`/api/v1/keys` alongside. Deprecate `/api/keys` in a future version.

---

## Implementation Order

Build in this order — each phase is usable independently:

**Phase A: Key registry write + operator auth**
- `POST /api/v1/keys` — clients can register their key
- `POST /api/v1/auth/challenge` + `POST /api/v1/auth/verify`
- `POST /api/v1/keys/{peer_id}/revoke`

**Phase B: Key request flow (relay issuance)**
- `POST /api/v1/key-requests`
- `GET /api/v1/key-requests` (operator)
- `POST /api/v1/key-requests/{id}/approve`
- `POST /api/v1/key-requests/{id}/deny`
- `GET /api/v1/key-requests/{id}/download`
- `GET /api/v1/server/relays`

**Phase C: Server sync**
- `GET /api/v1/sync/keys`
- `POST /api/v1/sync/register`

**Phase D: Email verification**
- `POST /api/v1/verify/email/start`
- `GET /api/v1/verify/email/{token}`

**Phase E: Phone + Identity verification**
- SMS + document submission endpoints

**Phase F: Content (meshsite)**
- Content submission + retrieval + search

---

*Status: Design complete — ready for implementation*
*All interfaces (meshsite, game client, operator tooling) use this API*
*No special paths for any interface — one API, everything uses it*
