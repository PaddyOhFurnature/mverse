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
    /// The ground floor and low walls — used for physics collision too.
    pub floor: Mesh,
    /// Decorative pillars around the perimeter.
    pub pillars: Mesh,
    /// Signup / identity terminal kiosk.
    pub signup_terminal: Mesh,
    /// World portal arch.
    pub world_portal: Mesh,
    /// Module doors (bank, marketplace, forums, post, etc.).
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
            module_doors:     build_module_doors(),
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

    // Low perimeter wall — 0.5 m high, keeps players from walking off
    let wall_h = 0.5_f32;
    let s = size;
    add_wall_strip(&mut mesh, Vec3::new(-s, 0.0, -s), Vec3::new( s, 0.0, -s), wall_h, WALL_COLOUR);
    add_wall_strip(&mut mesh, Vec3::new( s, 0.0, -s), Vec3::new( s, 0.0,  s), wall_h, WALL_COLOUR);
    add_wall_strip(&mut mesh, Vec3::new( s, 0.0,  s), Vec3::new(-s, 0.0,  s), wall_h, WALL_COLOUR);
    add_wall_strip(&mut mesh, Vec3::new(-s, 0.0,  s), Vec3::new(-s, 0.0, -s), wall_h, WALL_COLOUR);

    mesh
}

/// Add a vertical wall quad between two floor-level points, extruded up by `height`.
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
        Vec3::new(pos.x - arch_w, pos.y, pos.z),
        thickness, arch_h, PORTAL_COLOUR);
    // Right pillar
    add_pillar(&mut mesh,
        Vec3::new(pos.x + arch_w, pos.y, pos.z),
        thickness, arch_h, PORTAL_COLOUR);
    // Lintel (top bar)
    add_wall_strip(&mut mesh,
        Vec3::new(pos.x - arch_w - thickness, arch_h, pos.z - thickness),
        Vec3::new(pos.x + arch_w + thickness, arch_h, pos.z - thickness),
        thickness, PORTAL_COLOUR);
    // Glowing fill inside the arch
    let gx0 = pos.x - arch_w + thickness;
    let gx1 = pos.x + arch_w - thickness;
    let gz  = pos.z;
    let gy0 = 0.05_f32;
    let gy1 = arch_h;
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(gx0, gy0, gz), GLOW_COLOUR));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(gx1, gy0, gz), GLOW_COLOUR));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(gx1, gy1, gz), GLOW_COLOUR));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(gx0, gy1, gz), GLOW_COLOUR));
    mesh.add_triangle(Triangle::new(v0, v1, v2));
    mesh.add_triangle(Triangle::new(v0, v2, v3));
    // Back face (visible when entering from the world side)
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    mesh.add_triangle(Triangle::new(v0, v3, v2));

    mesh
}

// ── Module doors ──────────────────────────────────────────────────────────────

/// Small door frames around the perimeter leading to construct modules.
fn build_module_doors() -> Mesh {
    let mut mesh = Mesh::new();
    // Each entry: (position, label_colour) — one door per module for now
    let doors = [
        (Vec3::new(-14.0, 0.0, -19.0), Vec3::new(0.9, 0.7, 0.1)),  // bank
        (Vec3::new( -7.0, 0.0, -19.0), Vec3::new(0.5, 0.9, 0.3)),  // marketplace
        (Vec3::new(  7.0, 0.0, -19.0), Vec3::new(0.3, 0.6, 0.9)),  // forums
        (Vec3::new( 14.0, 0.0, -19.0), Vec3::new(0.9, 0.4, 0.2)),  // post
        (Vec3::new(-19.0, 0.0,   0.0), Vec3::new(0.8, 0.2, 0.2)),  // emergency
        (Vec3::new( 19.0, 0.0,   0.0), Vec3::new(0.6, 0.3, 0.8)),  // government
    ];
    for (pos, colour) in &doors {
        add_door_frame(&mut mesh, *pos, *colour);
    }
    mesh
}

fn add_door_frame(mesh: &mut Mesh, pos: Vec3, colour: Vec3) {
    let w = 0.8_f32; let h = 2.2_f32; let t = 0.2_f32;
    // Left post
    add_pillar(mesh, Vec3::new(pos.x - w, pos.y, pos.z), t, h, colour);
    // Right post
    add_pillar(mesh, Vec3::new(pos.x + w, pos.y, pos.z), t, h, colour);
    // Top bar
    add_wall_strip(mesh,
        Vec3::new(pos.x - w - t, h, pos.z - t),
        Vec3::new(pos.x + w + t, h, pos.z - t),
        t, colour);
}

// ── Physics collision data ────────────────────────────────────────────────────

/// A flat collision plane for the construct floor.
/// Returns vertices + triangle indices in the format expected by
/// `physics::create_collision_from_mesh`.
///
/// We generate a simple subdivided floor plane that rapier can use as a
/// trimesh collider — same approach as terrain chunks.
pub fn build_floor_collision_mesh() -> crate::mesh::Mesh {
    // Re-use the visual floor mesh for collision — it's already flat quads.
    // The perimeter walls act as invisible barriers.
    build_floor()
}
