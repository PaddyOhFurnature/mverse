//! Vegetation placement for worldgen.
//!
//! Deterministic per-column tree/shrub placement based on biome type.
//! Called from terrain.rs Pass 2 after ground voxels are placed.
//!
//! Trees are placed entirely within the current Y chunk (max height ≤ 15 voxels,
//! well within the 200-voxel Y chunk height). Voxels outside chunk bounds are skipped.

use crate::biome::Biome;
use crate::materials::MaterialId;
use crate::voxel::{Octree, VoxelCoord};

// ── Crown shapes ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Crown {
    /// Rounded sphere — subtropical/rainforest/riparian.
    Sphere,
    /// Narrow upward cone — eucalyptus / dry sclerophyll.
    Cone,
    /// Irregular scattered — sparse grassland trees.
    Sparse,
    /// Low dome — shrubs with no significant trunk.
    Shrub,
}

// ── Per-biome parameters ──────────────────────────────────────────────────────

struct VegParams {
    /// Probability [0,1] that any given column grows vegetation.
    density: f32,
    /// Minimum trunk voxels above surface (0 = shrub, crown starts at surface).
    trunk_min: i64,
    /// Maximum trunk voxels above surface.
    trunk_max: i64,
    /// Crown radius in voxels.
    crown_r: i64,
    crown: Crown,
}

fn veg_params(biome: Biome) -> Option<VegParams> {
    Some(match biome {
        // Dense subtropical / riparian forest — tall, wide canopy
        Biome::SubtropicalForest | Biome::RiparianCorridor => VegParams {
            density: 0.30,
            trunk_min: 7,
            trunk_max: 14,
            crown_r: 4,
            crown: Crown::Sphere,
        },
        // Dry eucalypt forest — medium height, narrow cone crown
        Biome::DrySclerophyllForest => VegParams {
            density: 0.14,
            trunk_min: 5,
            trunk_max: 10,
            crown_r: 3,
            crown: Crown::Cone,
        },
        // Cool mountain forest
        Biome::MontaneForest | Biome::BorealForest => VegParams {
            density: 0.28,
            trunk_min: 6,
            trunk_max: 12,
            crown_r: 3,
            crown: Crown::Cone,
        },
        // Subtropical grassland — scattered gum trees
        Biome::SubtropicalGrassland => VegParams {
            density: 0.05,
            trunk_min: 4,
            trunk_max: 8,
            crown_r: 2,
            crown: Crown::Sparse,
        },
        // Mangrove coast — low squat mangroves above the waterline
        Biome::MangroveCoast => VegParams {
            density: 0.22,
            trunk_min: 2,
            trunk_max: 5,
            crown_r: 2,
            crown: Crown::Sphere,
        },
        // Wetlands — sparse sedge/reed tufts and occasional paperbark
        Biome::Wetland => VegParams {
            density: 0.10,
            trunk_min: 3,
            trunk_max: 6,
            crown_r: 2,
            crown: Crown::Sphere,
        },
        // Low shrubland / mallee — no trunk, dome-shaped crown
        Biome::Shrubland => VegParams {
            density: 0.18,
            trunk_min: 0,
            trunk_max: 1,
            crown_r: 2,
            crown: Crown::Shrub,
        },
        // Urban — occasional park/street trees
        Biome::Urban => VegParams {
            density: 0.012,
            trunk_min: 4,
            trunk_max: 6,
            crown_r: 2,
            crown: Crown::Sphere,
        },
        // Agricultural — hedgerow / field trees
        Biome::Agricultural => VegParams {
            density: 0.025,
            trunk_min: 4,
            trunk_max: 7,
            crown_r: 2,
            crown: Crown::Sphere,
        },
        // No vegetation
        Biome::Ocean
        | Biome::Lake
        | Biome::River
        | Biome::Beach
        | Biome::RockyCoast
        | Biome::AlpineGrassland
        | Biome::Desert
        | Biome::Tundra
        | Biome::IceCap => return None,
    })
}

// ── Fast deterministic column hash ───────────────────────────────────────────

/// FNV-1a inspired mix — deterministic per (vx, vz) position.
fn column_hash(vx: i64, vz: i64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h ^= (vx as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h = h.wrapping_mul(0x0000_0001_0000_0001_u64.wrapping_add(h >> 33));
    h ^= (vz as u64).wrapping_mul(0x6c62_272e_07bb_0142);
    h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Try to place a tree or shrub on this column.
///
/// - Only placed when `slope_deg < 32°` (vegetation doesn't grow on steep terrain).
/// - Uses a deterministic hash of `(vx, vz)` for placement density + height variation.
/// - Voxels outside `[min_y, max_y)` are silently skipped (chunk boundary safety).
pub fn maybe_place_vegetation(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    surface_y: i64,
    biome: Biome,
    slope_deg: f32,
    min_y: i64,
    max_y: i64,
) {
    // No trees on steep slopes or cliffs.
    if slope_deg > 32.0 {
        return;
    }

    let params = match veg_params(biome) {
        Some(p) => p,
        None => return,
    };

    let h = column_hash(vx, vz);

    // Density check: use low 16 bits as a uniform [0,1).
    let density_roll = (h & 0xFFFF) as f32 / 65536.0;
    if density_roll > params.density {
        return;
    }

    // Pick trunk height from the range using bits 16-31.
    let trunk_range = (params.trunk_max - params.trunk_min).max(1);
    let trunk_h = params.trunk_min + ((h >> 16) as i64 % trunk_range);

    // Place trunk (WOOD voxels from surface+1 upward).
    for dy in 1..=trunk_h {
        let y = surface_y + dy;
        if y < min_y || y >= max_y {
            break;
        }
        let pos = VoxelCoord::new(vx, y, vz);
        if octree.get_voxel(pos) == MaterialId::AIR {
            octree.set_voxel(pos, MaterialId::WOOD);
        }
    }

    // Crown base: just above trunk tip.
    let crown_base = surface_y + trunk_h;
    let cr = params.crown_r;

    place_crown(
        octree,
        vx,
        vz,
        crown_base,
        cr,
        params.crown,
        h,
        min_y,
        max_y,
    );
}

fn place_crown(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    crown_base: i64,
    cr: i64,
    shape: Crown,
    seed: u64,
    min_y: i64,
    max_y: i64,
) {
    // Crown extent: dy ranges from -(cr/2) to +cr for most shapes.
    let dy_min: i64 = match shape {
        Crown::Shrub => -(cr / 2),
        Crown::Cone => 0,
        _ => -(cr / 2),
    };
    let dy_max: i64 = match shape {
        Crown::Shrub => cr / 2 + 1,
        _ => cr + 1,
    };

    for dy in dy_min..=dy_max {
        for dx in -cr..=cr {
            for dz in -cr..=cr {
                let in_crown = match shape {
                    Crown::Sphere | Crown::Shrub => {
                        // Sphere centred at (0, cr/2, 0) relative to crown_base.
                        let cy = cr / 2;
                        dx * dx + (dy - cy) * (dy - cy) + dz * dz <= cr * cr
                    }
                    Crown::Cone => {
                        // Cone: wide at base, narrow at top.
                        // At dy=0 radius=cr, at dy=cr radius=0.
                        let layer_r = cr - dy;
                        dy >= 0 && layer_r > 0 && dx * dx + dz * dz <= layer_r * layer_r
                    }
                    Crown::Sparse => {
                        // Sphere with ~30% of outer leaves randomly dropped.
                        let cy = cr / 2;
                        let in_sphere = dx * dx + (dy - cy) * (dy - cy) + dz * dz <= cr * cr;
                        if !in_sphere {
                            false
                        } else {
                            let inner =
                                dx * dx + (dy - cy) * (dy - cy) + dz * dz <= (cr - 1) * (cr - 1);
                            if inner {
                                true
                            } else {
                                // Outer shell — keep ~70%
                                let lh = column_hash(vx + dx * 7 + dy, vz + dz * 5 + dy);
                                (lh ^ seed) & 0x3 != 0
                            }
                        }
                    }
                };

                if !in_crown {
                    continue;
                }

                let y = crown_base + dy;
                if y < min_y || y >= max_y {
                    continue;
                }
                let pos = VoxelCoord::new(vx + dx, y, vz + dz);
                if octree.get_voxel(pos) == MaterialId::AIR {
                    octree.set_voxel(pos, MaterialId::LEAVES);
                }
            }
        }
    }
}
