# WORLDNET OS — Design Document

## What It Is

WORLDNET is the distributed operating system running on the P2P layer.
Not a web app. Not HTML served from a server. Not a browser.

A pixel-buffer renderer that runs on the **local node**, reads from the
**local P2P cache** (gossipsub + DHT), and displays on whatever physical
surface is in front of you — a terminal screen in the lobby, a tablet in
your hand, a PC in your home chunk, or your phone at a bus stop.

Same data. Same renderer. Same address system. Different surface.

---

## The Address System

Every object, room, user, setting, piece of content, and world event
has a WORLDNET address:

```
worldnet://                          root
worldnet://forums                    forums section
worldnet://forums/thread/abc123      specific thread
worldnet://wiki/terrain              wiki article
worldnet://marketplace               marketplace root
worldnet://user/<peer_id>            user profile
worldnet://world/construct           the construct lobby
worldnet://world/construct/rooms     room list
worldnet://world/chunk/0/0           chunk at 0,0
worldnet://world/objects             all placed objects
worldnet://admin/keys                key registry (admin only)
worldnet://admin/regions             region management (admin only)
worldnet://admin/moderation          moderation queue (admin only)
worldnet://settings                  your personal settings
worldnet://settings/identity         your key/identity
worldnet://settings/property         your parcels/chunks
worldnet://settings/inventory        your items
```

Addresses are:
- **Linkable** — share a worldnet:// address, anyone with access opens it
- **Searchable** — full-text search across all cached content
- **Navigable** — back/forward history, bookmarks, typed navigation
- **Permission-gated** — address exists or doesn't based on your key type

---

## Access Tiers

One system. One renderer. Key type determines what you see.

### Tier 0 — No key (at public terminal)
- Read-only public content (forums, wiki, world map)
- Signup page only — cannot post, cannot navigate further
- worldnet://signup is the landing page

### Tier 1 — Guest key
- Read all public content
- Post to forums, wiki edits
- Browse marketplace (no purchase)
- Own profile page
- No admin paths, no intranet

### Tier 2 — Certified User key
- Full public access — post, interact, trade, build
- Personal tablet / home PC unlocked
- DMs, inventory, property, settings
- No admin paths

### Tier 3+ — Admin key
- Everything above PLUS the intranet:
  - worldnet://admin/* routes available
  - World configuration (rooms, layouts, placed objects)
  - Moderation queue, bans, key registry approvals
  - Server/relay management pages
  - Region governance
- All admin actions are **signed ops** — auditable, propagated via gossipsub

---

## Physical Surfaces

The WORLDNET renderer outputs a **pixel buffer** → **wgpu texture**.
That texture is applied to a physical surface in the 3D world:

| Surface            | Where                          | Default address              |
|--------------------|--------------------------------|------------------------------|
| Public terminal    | Construct lobby                | worldnet:// (key-gated view) |
| Personal tablet    | Inventory item, all reg. users | worldnet://settings          |
| Home PC            | Placed in home chunk           | worldnet://                  |
| Room wall          | Each construct room            | worldnet://forums etc.       |
| Billboard          | Anywhere placed                | worldnet://<assigned address>|
| Phone (external)   | Real phone/browser             | worldnet:// via local relay  |

---

## Renderer Architecture

```
WORLDNET address
    ↓
Address resolver (checks local DHT/cache + key permissions)
    ↓
Content fetcher (local cache first, DHT fallback, gossipsub subscribe)
    ↓
Page renderer (layout engine → pixel buffer)
    ↓
wgpu Texture upload
    ↓
Applied to surface mesh in 3D world
```

The renderer is NOT a browser engine. It is:
- A layout engine for structured content (lists, grids, text, images)
- Driven by the content type at the address (thread list, wiki page, map, settings form)
- Font rendering via `fontdue` or `ab_glyph` → RGBA pixels → wgpu Texture
- Interactive: cursor position on the surface → hover/click events → navigation

---

## Data Backend

Every WORLDNET address maps to a **signed data structure** on the P2P layer:

- Content (posts, wiki, listings) → gossipsub + DHT + SQLite persistence
- World config (rooms, objects, layouts) → DHT key `world/construct/config`, signed by admin key
- User data (settings, inventory, property) → DHT key `user/<peer_id>/...`, signed by user key
- Admin data (key registry, bans) → DHT + server SQLite, admin-signed ops

Nothing is hardcoded. Everything is a signed op. Change it with the right key,
it propagates to every node via gossipsub, every surface re-renders from new data.

---

## Build Order

**Step 1: WorldnetAddress type + resolver**
- `worldnet://` URL parser → `WorldnetAddress` struct
- Permission check: `can_access(key_type, address) -> bool`
- Map address → content fetcher function

**Step 2: Pixel buffer renderer**
- `WorldnetRenderer` — takes `WorldnetAddress` + `&[ContentItem]` → `Vec<u8>` RGBA
- Start with text-only: title list, body text, navigation breadcrumb
- Font: `fontdue` for pixel-accurate rendering at game texture resolution

**Step 3: wgpu texture surface**
- Upload pixel buffer → `wgpu::Texture`
- Apply to existing billboard/terminal mesh in the world
- Refresh when content cache updates

**Step 4: Public terminal**
- Walk up, E key → terminal becomes active surface
- Renderer shows key-gated view (signup / public browse)
- Keyboard input routed to terminal when active

**Step 5: Personal tablet item**
- Inventory item given to all registered users on first login
- T key (or menu) → tablet visible in hands, renders WORLDNET
- Keyboard/mouse input routed to tablet surface

**Step 6: Admin intranet pages**
- worldnet://admin/* routes, admin key check
- World config editor — rooms, placed objects
- Moderation queue

**Step 7: External access**
- Local relay serves a thin bridge: real browser → relay → WORLDNET renderer
- Phone/laptop access to same data via browser hitting local relay port
- Not a web app — just a viewport into the same pixel-buffer renderer output

---

## What This Replaces

- `GameMode::ConstructModule` — gone, was always wrong
- `render_module_overlay()` — gone
- `egui` overlays for in-world content — gone
- Hardcoded `MODULES` array — replaced by signed world config from DHT
- `xdg-open` browser — gone (that was a cop-out)

The WORLDNET renderer IS the interface. One renderer, all surfaces.
