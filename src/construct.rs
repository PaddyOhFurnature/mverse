//! The Construct — a bundled local scene that is always available.
//!
//! The Construct is the entry point for every player. It loads from data
//! defined here in the binary — no network, no chunk streaming, no terrain
//! generation required. The player stands on solid ground from frame 1.
//!
//! # Layout
//!
//! ```text
//!                    [WORLD PORTAL]
//!                         |
//!    [BANK]  [MARKET]  [LOBBY FLOOR]  [FORUMS]  [POST]
//!                         |
//!              [SIGNUP / IDENTITY TERMINAL]
//! ```
//!
//! The floor is a 40×40 unit plaza. Spawn is at the centre (0, 0, 0).
//! The world portal is at +Z. Construct module doors ring the perimeter.
//! The signup terminal is near the spawn point.
//!
//! All coordinates are in local physics space (metres, Y-up).

use crate::mesh::{Mesh, Triangle, Vertex};
use glam::Vec3;

// ── Colours ───────────────────────────────────────────────────────────────────

const FLOOR_COLOUR:    Vec3 = Vec3::new(0.18, 0.18, 0.22);
const WALL_COLOUR:     Vec3 = Vec3::new(0.25, 0.25, 0.30);
const PILLAR_COLOUR:   Vec3 = Vec3::new(0.30, 0.30, 0.38);
const TERMINAL_COLOUR: Vec3 = Vec3::new(0.10, 0.40, 0.55);
const PORTAL_COLOUR:   Vec3 = Vec3::new(0.20, 0.60, 0.90);
const GLOW_COLOUR:     Vec3 = Vec3::new(0.40, 0.85, 1.00);

// ── Spawn / interactive points ────────────────────────────────────────────────

/// Where the player spawns in the construct (local space, Y-up).
pub const SPAWN_POINT: Vec3 = Vec3::new(0.0, 0.1, 0.0);

/// Where the signup terminal stands (player walks here on first run).
pub const SIGNUP_TERMINAL_POS: Vec3 = Vec3::new(0.0, 0.0, -6.0);

/// Where the world portal stands (player walks through to enter the world).
pub const WORLD_PORTAL_POS: Vec3 = Vec3::new(0.0, 0.0, 14.0);

/// Radius within which a player "activates" a terminal (metres).
pub const INTERACT_RADIUS: f32 = 2.0;

// ── Scene builder ─────────────────────────────────────────────────────────────

/// All static meshes that make up the construct scene.
pub struct ConstructScene {
    /// The ground floor and low perimeter curb (with gaps for module rooms).
    pub floor: Mesh,
    /// Decorative pillars around the perimeter.
    pub pillars: Mesh,
    /// Signup / identity terminal kiosk.
    pub signup_terminal: Mesh,
    /// World portal arch.
    pub world_portal: Mesh,
    /// Module rooms — corridors + enclosed rooms with screen walls.
    pub module_doors: Mesh,
}

impl ConstructScene {
    /// Build the full construct scene from hardcoded geometry.
    pub fn build() -> Self {
        Self {
            floor:            build_floor(),
            pillars:          build_pillars(),
            signup_terminal:  build_terminal(SIGNUP_TERMINAL_POS, TERMINAL_COLOUR),
            world_portal:     build_portal_arch(WORLD_PORTAL_POS),
            module_doors:     build_module_rooms(),
        }
    }
}

// ── Floor ─────────────────────────────────────────────────────────────────────

fn build_floor() -> Mesh {
    let mut mesh = Mesh::new();
    // 40×40 plaza floor, 9 tiles of ~13 units each for visual grid
    let size = 20.0_f32;
    let tiles = 4;
    let step = size * 2.0 / tiles as f32;

    for row in 0..tiles {
        for col in 0..tiles {
            let x0 = -size + col as f32 * step;
            let x1 = x0 + step;
            let z0 = -size + row as f32 * step;
            let z1 = z0 + step;
            let y  = 0.0_f32;

            // Alternate tile shade for grid effect
            let shade = if (row + col) % 2 == 0 { FLOOR_COLOUR } else {
                Vec3::new(FLOOR_COLOUR.x * 1.15, FLOOR_COLOUR.y * 1.15, FLOOR_COLOUR.z * 1.15)
            };
            let up = Vec3::Y;

            let v0 = mesh.add_vertex(Vertex::new(Vec3::new(x0, y, z0), shade));
            let v1 = mesh.add_vertex(Vertex::new(Vec3::new(x1, y, z0), shade));
            let v2 = mesh.add_vertex(Vertex::new(Vec3::new(x1, y, z1), shade));
            let v3 = mesh.add_vertex(Vertex::new(Vec3::new(x0, y, z1), shade));
            // CCW from above → normal faces +Y (upward, toward player)
            mesh.add_triangle(Triangle::new(v0, v2, v1));
            mesh.add_triangle(Triangle::new(v0, v3, v2));
            let _ = up; // normal used by renderer implicitly from vertex colour
        }
    }

    // Low perimeter curb — 0.5 m high, with gaps where module rooms attach.
    let wall_h = 0.5_f32;
    let s = size;
    let hw = DOOR_WIDTH * 0.5;

    // North wall (z = -s): segments around each module offset on this side
    add_wall_with_gaps(&mut mesh, 'x', -s, s, -s, wall_h, WALL_COLOUR,
        &module_gaps_on_side(WallSideId::North), hw);
    // South wall (z = +s)
    add_wall_with_gaps(&mut mesh, 'x', -s, s, s, wall_h, WALL_COLOUR,
        &module_gaps_on_side(WallSideId::South), hw);
    // East wall (x = +s)
    add_wall_with_gaps(&mut mesh, 'z', -s, s, s, wall_h, WALL_COLOUR,
        &module_gaps_on_side(WallSideId::East), hw);
    // West wall (x = -s)
    add_wall_with_gaps(&mut mesh, 'z', -s, s, -s, wall_h, WALL_COLOUR,
        &module_gaps_on_side(WallSideId::West), hw);

    mesh
}

/// Identifies a wall side (used for gap calculations — avoids re-exporting WallSide).
#[derive(PartialEq)]
enum WallSideId { North, South, East, West }

/// Returns the list of door-center offsets for all modules on the given side.
fn module_gaps_on_side(side: WallSideId) -> Vec<f32> {
    MODULES.iter()
        .filter(|m| match (&m.side, &side) {
            (WallSide::North, WallSideId::North) => true,
            (WallSide::South, WallSideId::South) => true,
            (WallSide::East,  WallSideId::East)  => true,
            (WallSide::West,  WallSideId::West)  => true,
            _ => false,
        })
        .map(|m| m.offset)
        .collect()
}

/// Build a wall along one axis, leaving gaps at each `gap_center` ± `half_gap`.
///
/// `axis` = 'x' → wall runs along X (constant Z = `fixed`).
/// `axis` = 'z' → wall runs along Z (constant X = `fixed`).
fn add_wall_with_gaps(
    mesh: &mut Mesh, axis: char, from: f32, to: f32, fixed: f32,
    height: f32, colour: Vec3, gap_centers: &[f32], half_gap: f32,
) {
    // Collect sorted gap intervals [center - half_gap, center + half_gap]
    let mut gaps: Vec<(f32, f32)> = gap_centers.iter()
        .map(|&c| (c - half_gap, c + half_gap))
        .collect();
    gaps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut cursor = from;
    for (g0, g1) in &gaps {
        if *g0 > cursor {
            // Solid segment before the gap
            let (a, b) = wall_pts(axis, cursor, *g0, fixed);
            add_wall_strip(mesh, a, b, height, colour);
        }
        cursor = g1.max(cursor);
    }
    if cursor < to {
        let (a, b) = wall_pts(axis, cursor, to, fixed);
        add_wall_strip(mesh, a, b, height, colour);
    }
}

fn wall_pts(axis: char, t0: f32, t1: f32, fixed: f32) -> (Vec3, Vec3) {
    if axis == 'x' {
        (Vec3::new(t0, 0.0, fixed), Vec3::new(t1, 0.0, fixed))
    } else {
        (Vec3::new(fixed, 0.0, t0), Vec3::new(fixed, 0.0, t1))
    }
}


/// Emits both front and back faces so the wall is visible from either side.
fn add_wall_strip(mesh: &mut Mesh, a: Vec3, b: Vec3, height: f32, colour: Vec3) {
    let a_top = Vec3::new(a.x, height, a.z);
    let b_top = Vec3::new(b.x, height, b.z);
    let v0 = mesh.add_vertex(Vertex::new(a,     colour));
    let v1 = mesh.add_vertex(Vertex::new(b,     colour));
    let v2 = mesh.add_vertex(Vertex::new(b_top, colour));
    let v3 = mesh.add_vertex(Vertex::new(a_top, colour));
    // Front face
    mesh.add_triangle(Triangle::new(v0, v1, v2));
    mesh.add_triangle(Triangle::new(v0, v2, v3));
    // Back face (reverse winding)
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    mesh.add_triangle(Triangle::new(v0, v3, v2));
}

// ── Pillars ───────────────────────────────────────────────────────────────────

fn build_pillars() -> Mesh {
    let mut mesh = Mesh::new();
    let positions = [
        Vec3::new(-16.0, 0.0, -16.0),
        Vec3::new( 16.0, 0.0, -16.0),
        Vec3::new(-16.0, 0.0,  16.0),
        Vec3::new( 16.0, 0.0,  16.0),
        Vec3::new(  0.0, 0.0, -16.0),
        Vec3::new(  0.0, 0.0,  16.0),
        Vec3::new(-16.0, 0.0,   0.0),
        Vec3::new( 16.0, 0.0,   0.0),
    ];
    for pos in &positions {
        add_pillar(&mut mesh, *pos, 0.6, 5.0, PILLAR_COLOUR);
    }
    mesh
}

/// Add a rectangular pillar (box) at `base`, with given half-width and height.
fn add_pillar(mesh: &mut Mesh, base: Vec3, hw: f32, height: f32, colour: Vec3) {
    let x0 = base.x - hw; let x1 = base.x + hw;
    let z0 = base.z - hw; let z1 = base.z + hw;
    let y0 = base.y;      let y1 = base.y + height;

    // 4 sides
    add_wall_strip(mesh, Vec3::new(x0,y0,z0), Vec3::new(x1,y0,z0), y1-y0, colour);
    add_wall_strip(mesh, Vec3::new(x1,y0,z0), Vec3::new(x1,y0,z1), y1-y0, colour);
    add_wall_strip(mesh, Vec3::new(x1,y0,z1), Vec3::new(x0,y0,z1), y1-y0, colour);
    add_wall_strip(mesh, Vec3::new(x0,y0,z1), Vec3::new(x0,y0,z0), y1-y0, colour);
    // Top cap (CCW from above → faces +Y)
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(x0,y1,z0), colour));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(x1,y1,z0), colour));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(x1,y1,z1), colour));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(x0,y1,z1), colour));
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    mesh.add_triangle(Triangle::new(v0, v3, v2));
}

// ── Terminal kiosk ────────────────────────────────────────────────────────────

/// A simple L-shaped standing terminal kiosk at `pos`.
fn build_terminal(pos: Vec3, colour: Vec3) -> Mesh {
    let mut mesh = Mesh::new();
    // Base post: 0.3 wide, 1.1 high
    add_pillar(&mut mesh, Vec3::new(pos.x, pos.y, pos.z), 0.15, 1.1, colour);
    // Screen top: wider flat box
    let sw = 0.5_f32; let sh = 0.05_f32; let sd = 0.35_f32;
    let sx = pos.x; let sy = pos.y + 1.1; let sz = pos.z;
    let screen_colour = GLOW_COLOUR;
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(sx-sw, sy,    sz-sd), screen_colour));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(sx+sw, sy,    sz-sd), screen_colour));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(sx+sw, sy,    sz+sd), screen_colour));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(sx-sw, sy,    sz+sd), screen_colour));
    let v4 = mesh.add_vertex(Vertex::new(Vec3::new(sx-sw, sy+sh, sz-sd), screen_colour));
    let v5 = mesh.add_vertex(Vertex::new(Vec3::new(sx+sw, sy+sh, sz-sd), screen_colour));
    let v6 = mesh.add_vertex(Vertex::new(Vec3::new(sx+sw, sy+sh, sz+sd), screen_colour));
    let v7 = mesh.add_vertex(Vertex::new(Vec3::new(sx-sw, sy+sh, sz+sd), screen_colour));
    // Top face (both sides)
    mesh.add_triangle(Triangle::new(v4, v5, v6));
    mesh.add_triangle(Triangle::new(v4, v6, v7));
    mesh.add_triangle(Triangle::new(v4, v6, v5));
    mesh.add_triangle(Triangle::new(v4, v7, v6));
    // Front face (facing -Z, toward player spawn) — both sides
    mesh.add_triangle(Triangle::new(v0, v1, v5));
    mesh.add_triangle(Triangle::new(v0, v5, v4));
    mesh.add_triangle(Triangle::new(v0, v5, v1));
    mesh.add_triangle(Triangle::new(v0, v4, v5));
    mesh
}

// ── World portal arch ─────────────────────────────────────────────────────────

fn build_portal_arch(pos: Vec3) -> Mesh {
    let mut mesh = Mesh::new();
    let arch_w = 2.5_f32;
    let arch_h = 4.0_f32;
    let thickness = 0.4_f32;

    // Left pillar
    add_pillar(&mut mesh,
        Vec3::new(pos.x - arch_w, pos.y, pos.z - thickness * 0.5),
        thickness, arch_h, PORTAL_COLOUR);
    // Right pillar
    add_pillar(&mut mesh,
        Vec3::new(pos.x + arch_w, pos.y, pos.z - thickness * 0.5),
        thickness, arch_h, PORTAL_COLOUR);
    // Lintel (top bar) — spans full width, centred on pos.z
    add_wall_strip(&mut mesh,
        Vec3::new(pos.x - arch_w - thickness, arch_h, pos.z - thickness),
        Vec3::new(pos.x + arch_w + thickness, arch_h, pos.z + thickness),
        thickness, PORTAL_COLOUR);
    // Glowing fill inside the arch — faces -Z (toward player spawn at 0,0,0)
    let gx0 = pos.x - arch_w + thickness;
    let gx1 = pos.x + arch_w - thickness;
    let gz  = pos.z;
    let gy0 = 0.05_f32;
    let gy1 = arch_h;
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(gx0, gy0, gz), GLOW_COLOUR));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(gx1, gy0, gz), GLOW_COLOUR));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(gx1, gy1, gz), GLOW_COLOUR));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(gx0, gy1, gz), GLOW_COLOUR));
    // Front face (toward spawn, -Z normal = CCW from -Z side)
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    mesh.add_triangle(Triangle::new(v0, v3, v2));
    // Back face
    mesh.add_triangle(Triangle::new(v0, v1, v2));
    mesh.add_triangle(Triangle::new(v0, v2, v3));

    mesh
}

// ── Meshsite module rooms ─────────────────────────────────────────────────────

/// A single section of the Meshsite that exists as both a virtual page and a
/// physical room in the Construct.  Adding an entry here auto-generates:
///   • a gap in the plaza perimeter wall
///   • a 3 m corridor leading outward
///   • an enclosed room with a glowing "screen" wall (content rendered later)
pub struct MeshsiteModule {
    /// Human-readable label (used for door markings later)
    pub name:         &'static str,
    /// Unique slug — becomes the in-game URL path (/forums, /wiki, etc.)
    pub slug:         &'static str,
    /// Structural / door colour
    pub colour:       Vec3,
    /// Screen wall accent colour
    pub screen_colour: Vec3,
    /// Which perimeter wall this room attaches to
    pub side:         WallSide,
    /// Position along the wall (the axis perpendicular to the normal).
    /// E.g. on North/South walls this is the X offset; on East/West it's Z.
    pub offset:       f32,
}

/// Which face of the plaza perimeter a module room extends from.
pub enum WallSide {
    /// Attaches to the -Z wall (front, toward signup terminal)
    North,
    /// Attaches to the +Z wall (back, toward world portal)
    South,
    /// Attaches to the +X wall (right)
    East,
    /// Attaches to the -X wall (left)
    West,
}

/// The built-in Meshsite modules.  Adding an entry here is all that is needed
/// to make a new room appear in the Construct.
pub const MODULES: &[MeshsiteModule] = &[
    MeshsiteModule {
        name: "Login", slug: "login",
        colour:       Vec3::new(0.20, 0.70, 0.40),
        screen_colour: Vec3::new(0.30, 1.00, 0.60),
        side: WallSide::North, offset: -10.0,
    },
    MeshsiteModule {
        name: "Signup", slug: "signup",
        colour:       Vec3::new(0.20, 0.50, 0.90),
        screen_colour: Vec3::new(0.40, 0.70, 1.00),
        side: WallSide::North, offset:  10.0,
    },
    MeshsiteModule {
        name: "Forums", slug: "forums",
        colour:       Vec3::new(0.80, 0.55, 0.10),
        screen_colour: Vec3::new(1.00, 0.80, 0.20),
        side: WallSide::East,  offset: -8.0,
    },
    MeshsiteModule {
        name: "Wiki", slug: "wiki",
        colour:       Vec3::new(0.70, 0.20, 0.70),
        screen_colour: Vec3::new(1.00, 0.40, 1.00),
        side: WallSide::East,  offset:  8.0,
    },
    MeshsiteModule {
        name: "Marketplace", slug: "market",
        colour:       Vec3::new(0.80, 0.20, 0.20),
        screen_colour: Vec3::new(1.00, 0.40, 0.40),
        side: WallSide::West,  offset: -8.0,
    },
    MeshsiteModule {
        name: "Post Office", slug: "post",
        colour:       Vec3::new(0.60, 0.30, 0.10),
        screen_colour: Vec3::new(0.90, 0.55, 0.20),
        side: WallSide::West,  offset:  8.0,
    },
];

/// Width of the doorway cut into the perimeter wall for each module (metres).
const DOOR_WIDTH: f32 = 3.2;
/// How far the corridor extends beyond the perimeter before widening to the room.
const CORRIDOR_DEPTH: f32 = 3.0;
/// Full width of the enclosed module room.
const ROOM_WIDTH: f32 = 8.0;
/// Depth of the enclosed module room (not counting corridor).
const ROOM_DEPTH: f32 = 6.0;
/// Height of corridors and rooms.
const ROOM_HEIGHT: f32 = 3.0;

/// Build all module rooms as a single merged mesh.
fn build_module_rooms() -> Mesh {
    let mut mesh = Mesh::new();
    for m in MODULES {
        add_module_room(&mut mesh, m);
    }
    mesh
}

/// Append one module's corridor + room geometry to `mesh`.
fn add_module_room(mesh: &mut Mesh, m: &MeshsiteModule) {
    let plaza = 20.0_f32; // half-size of plaza floor
    let hw = DOOR_WIDTH * 0.5;
    let rh = ROOM_HEIGHT;

    // For each side, define:
    //   - perimeter point on the wall (where the door is cut)
    //   - normal direction (outward from plaza)
    //   - tangent direction (along the wall face, for room width)
    let (door_center, normal, tangent) = match m.side {
        WallSide::North => (Vec3::new(m.offset,  0.0, -plaza), Vec3::new(0.0,0.0,-1.0), Vec3::new(1.0,0.0,0.0)),
        WallSide::South => (Vec3::new(m.offset,  0.0,  plaza), Vec3::new(0.0,0.0, 1.0), Vec3::new(1.0,0.0,0.0)),
        WallSide::East  => (Vec3::new( plaza, 0.0, m.offset),  Vec3::new( 1.0,0.0,0.0), Vec3::new(0.0,0.0,1.0)),
        WallSide::West  => (Vec3::new(-plaza, 0.0, m.offset),  Vec3::new(-1.0,0.0,0.0), Vec3::new(0.0,0.0,1.0)),
    };

    let c = m.colour;
    let sc = m.screen_colour;

    // ── Corridor (from perimeter to room entrance) ────────────────────────────
    let corr_end = door_center + normal * CORRIDOR_DEPTH;
    // Floor
    add_horiz_quad(mesh, door_center - tangent*hw, corr_end - tangent*hw,
                         corr_end + tangent*hw, door_center + tangent*hw, c);
    // Ceiling
    let ceil_off = Vec3::new(0.0, rh, 0.0);
    add_horiz_quad(mesh,
        door_center + tangent*hw + ceil_off, corr_end + tangent*hw + ceil_off,
        corr_end - tangent*hw + ceil_off, door_center - tangent*hw + ceil_off, c);
    // Left wall
    add_wall_strip(mesh, door_center - tangent*hw, corr_end - tangent*hw, rh, c);
    // Right wall
    add_wall_strip(mesh, corr_end + tangent*hw, door_center + tangent*hw, rh, c);

    // ── Enclosed room ─────────────────────────────────────────────────────────
    let room_hw = ROOM_WIDTH * 0.5;
    let room_start = corr_end;
    let room_end   = corr_end + normal * ROOM_DEPTH;
    // Floor
    add_horiz_quad(mesh,
        room_start - tangent*room_hw, room_end - tangent*room_hw,
        room_end   + tangent*room_hw, room_start + tangent*room_hw, c);
    // Ceiling
    add_horiz_quad(mesh,
        room_start + tangent*room_hw + ceil_off, room_end + tangent*room_hw + ceil_off,
        room_end   - tangent*room_hw + ceil_off, room_start - tangent*room_hw + ceil_off, c);
    // Left wall
    add_wall_strip(mesh, room_start - tangent*room_hw, room_end - tangent*room_hw, rh, c);
    // Right wall
    add_wall_strip(mesh, room_end + tangent*room_hw, room_start + tangent*room_hw, rh, c);
    // Screen wall (far face — the "browser" surface, glowing accent colour)
    add_wall_strip(mesh, room_end + tangent*room_hw, room_end - tangent*room_hw, rh, sc);
    // Entry wall segments beside the corridor opening (back of the plaza wall thickness)
    // Left jamb
    add_wall_strip(mesh, room_start - tangent*room_hw, room_start - tangent*hw, rh, c);
    // Right jamb
    add_wall_strip(mesh, room_start + tangent*hw, room_start + tangent*room_hw, rh, c);
}

/// Add a horizontal (XZ-plane) quad from four corner points (CCW from above).
fn add_horiz_quad(mesh: &mut Mesh, a: Vec3, b: Vec3, c_pt: Vec3, d: Vec3, colour: Vec3) {
    let v0 = mesh.add_vertex(Vertex::new(a,    colour));
    let v1 = mesh.add_vertex(Vertex::new(b,    colour));
    let v2 = mesh.add_vertex(Vertex::new(c_pt, colour));
    let v3 = mesh.add_vertex(Vertex::new(d,    colour));
    // Top face (CCW from above)
    mesh.add_triangle(Triangle::new(v0, v3, v2));
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    // Bottom face (for ceilings — CCW from below)
    mesh.add_triangle(Triangle::new(v0, v2, v3));
    mesh.add_triangle(Triangle::new(v0, v1, v2));
}

// ── Physics collision data ────────────────────────────────────────────────────

/// A flat collision plane covering the entire construct footprint.
///
/// Uses a single large quad (not the visual mesh) — simpler, more robust,
/// and covers both the plaza AND all module room floors.
///
/// Plaza is 40×40 (-20..+20).  Rooms extend ~9 m outward on each side.
/// 60×60 (-30..+30) covers everything with margin.
pub fn build_floor_collision_mesh() -> crate::mesh::Mesh {
    use crate::mesh::{Mesh, Triangle, Vertex};
    let mut mesh = Mesh::new();
    let s = 30.0_f32;
    let y = 0.0_f32;
    let col = glam::Vec3::ZERO; // colour irrelevant for collision mesh
    let v0 = mesh.add_vertex(Vertex::new(glam::Vec3::new(-s, y, -s), col));
    let v1 = mesh.add_vertex(Vertex::new(glam::Vec3::new( s, y, -s), col));
    let v2 = mesh.add_vertex(Vertex::new(glam::Vec3::new( s, y,  s), col));
    let v3 = mesh.add_vertex(Vertex::new(glam::Vec3::new(-s, y,  s), col));
    // CCW from above → normal faces +Y (collides from above)
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    mesh.add_triangle(Triangle::new(v0, v3, v2));
    // Also add reverse faces so Rapier treats it as two-sided
    mesh.add_triangle(Triangle::new(v0, v1, v2));
    mesh.add_triangle(Triangle::new(v0, v2, v3));
    mesh
}
