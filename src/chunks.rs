// Quad-sphere chunking system for spherical Earth
// Hierarchical tile addressing using cube-to-sphere projection

use std::fmt;

use crate::coordinates::{EcefPos, GpsPos, gps_to_ecef};

/// Unique identifier for a chunk tile on the quad-sphere
///
/// The quad-sphere divides Earth into 6 cube faces, each recursively
/// subdivided into quadtree tiles. Each subdivision level is a "depth".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkId {
    /// Which cube face (0-5): 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
    pub face: u8,
    /// Quadtree path from root to this tile
    /// Each element is 0-3: 0=TL, 1=TR, 2=BL, 3=BR
    pub path: Vec<u8>,
}

impl ChunkId {
    /// Returns the depth (LOD level) of this chunk
    ///
    /// Depth equals the path length. Depth 0 = entire face, higher = more detailed.
    pub fn depth(&self) -> usize {
        self.path.len()
    }
    
    /// Creates a root (depth-0) tile for a given face
    ///
    /// # Arguments
    /// * `face` - Cube face index (0-5)
    ///
    /// # Returns
    /// ChunkId with empty path (depth 0)
    pub fn root(face: u8) -> Self {
        ChunkId {
            face,
            path: Vec::new(),
        }
    }
}

impl fmt::Display for ChunkId {
    /// Formats ChunkId as "F{face}/{path}"
    ///
    /// # Examples
    /// - `F2/0312` - face 2, path [0,3,1,2]
    /// - `F5/` - face 5, root (empty path)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "F{}/", self.face)?;
        for &p in &self.path {
            write!(f, "{}", p)?;
        }
        Ok(())
    }
}

/// Determines which cube face an ECEF position projects onto and its UV coordinates.
///
/// The cube is axis-aligned with origin at Earth's centre. Each face is identified
/// by which axis has the largest absolute value.
///
/// # Arguments
/// * `ecef` - ECEF position to map
///
/// # Returns
/// Tuple of (face_index, u, v) where:
/// - face_index: 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
/// - u, v: normalized coordinates in [-1, 1] on the cube face
///
/// # Examples
/// ```
/// use metaverse_core::chunks::ecef_to_cube_face;
/// use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
/// let gps = GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 };
/// let ecef = gps_to_ecef(&gps);
/// let (face, u, v) = ecef_to_cube_face(&ecef);
/// assert_eq!(face, 0); // Equator at prime meridian maps to +X face
/// ```
pub fn ecef_to_cube_face(ecef: &EcefPos) -> (u8, f64, f64) {
    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;
    
    let abs_x = x.abs();
    let abs_y = y.abs();
    let abs_z = z.abs();
    
    // Determine dominant axis (which face of the cube)
    let (face, u, v) = if abs_x >= abs_y && abs_x >= abs_z {
        // X is dominant
        if x > 0.0 {
            // Face 0: +X (prime meridian)
            (0, y / abs_x, z / abs_x)
        } else {
            // Face 1: -X (antimeridian)
            (1, -y / abs_x, z / abs_x)
        }
    } else if abs_y >= abs_x && abs_y >= abs_z {
        // Y is dominant
        if y > 0.0 {
            // Face 2: +Y (90° East)
            (2, -x / abs_y, z / abs_y)
        } else {
            // Face 3: -Y (90° West)
            (3, x / abs_y, z / abs_y)
        }
    } else {
        // Z is dominant
        if z > 0.0 {
            // Face 4: +Z (North Pole)
            (4, y / abs_z, -x / abs_z)
        } else {
            // Face 5: -Z (South Pole)
            (5, y / abs_z, x / abs_z)
        }
    };
    
    (face, u, v)
}

/// Projects a cube face UV coordinate onto the sphere surface (Snyder's equal-area projection)
///
/// # Arguments
/// * `face` - Cube face index (0-5): 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
/// * `u` - Horizontal UV coordinate in [-1, 1]
/// * `v` - Vertical UV coordinate in [-1, 1]
/// * `radius` - Sphere radius (typically WGS84_A for Earth's equator)
///
/// # Returns
/// ECEF position on the sphere surface
///
/// Uses Snyder's equal-area cube-to-sphere projection to minimize distortion.
pub fn cube_to_sphere(face: u8, u: f64, v: f64, radius: f64) -> EcefPos {
    // Snyder's equal-area projection formulas
    let x_prime = u * (1.0 - v * v / 2.0).sqrt();
    let y_prime = v * (1.0 - u * u / 2.0).sqrt();
    let z_prime = (0.0_f64.max(1.0 - u * u / 2.0 - v * v / 2.0)).sqrt();
    
    // Apply face-specific permutation and signs
    let (x, y, z) = match face {
        0 => (z_prime, -x_prime, -y_prime),  // +X face
        1 => (-z_prime, x_prime, -y_prime),  // -X face
        2 => (x_prime, z_prime, -y_prime),   // +Y face
        3 => (-x_prime, -z_prime, -y_prime), // -Y face
        4 => (x_prime, y_prime, z_prime),    // +Z face
        5 => (-x_prime, y_prime, -z_prime),  // -Z face
        _ => panic!("Invalid face index: {}", face),
    };
    
    // Normalize to unit sphere (required for accurate projection)
    let magnitude = (x * x + y * y + z * z).sqrt();
    let x_norm = x / magnitude;
    let y_norm = y / magnitude;
    let z_norm = z / magnitude;
    
    // Scale to desired radius
    EcefPos {
        x: x_norm * radius,
        y: y_norm * radius,
        z: z_norm * radius,
    }
}

/// Inverse of cube_to_sphere: projects ECEF position back to cube face + UV
///
/// # Arguments
/// * `ecef` - Position in ECEF coordinates
///
/// # Returns
/// Tuple of (face_index, u, v) where face is 0-5 and u,v are in [-1, 1]
///
/// Uses iterative optimization to find (u,v) that produces the given ECEF point.
pub fn sphere_to_cube(ecef: &EcefPos) -> (u8, f64, f64) {
    // Normalize to unit sphere
    let radius = (ecef.x * ecef.x + ecef.y * ecef.y + ecef.z * ecef.z).sqrt();
    let x_target = ecef.x / radius;
    let y_target = ecef.y / radius;
    let z_target = ecef.z / radius;
    
    // Determine which face by finding dominant axis
    let abs_x = x_target.abs();
    let abs_y = y_target.abs();
    let abs_z = z_target.abs();
    
    let face = if abs_x >= abs_y && abs_x >= abs_z {
        if x_target > 0.0 { 0 } else { 1 }
    } else if abs_y >= abs_x && abs_y >= abs_z {
        if y_target > 0.0 { 2 } else { 3 }
    } else {
        if z_target > 0.0 { 4 } else { 5 }
    };
    
    // Find (u, v) that produces this ECEF via cube_to_sphere
    // Use iterative refinement starting from planar projection guess
    
    // Simple initial guess based on planar projection
    let (u_init, v_init) = match face {
        0 => (y_target / abs_x, z_target / abs_x),
        1 => (-y_target / abs_x, z_target / abs_x),
        2 => (-x_target / abs_y, z_target / abs_y),
        3 => (x_target / abs_y, z_target / abs_y),
        4 => (y_target / abs_z, -x_target / abs_z),
        5 => (y_target / abs_z, x_target / abs_z),
        _ => (0.0, 0.0),
    };
    
    let mut u = u_init.clamp(-0.99, 0.99);
    let mut v = v_init.clamp(-0.99, 0.99);
    
    // Iteratively refine to match target
    for _ in 0..20 {
        // Forward pass: compute where current (u, v) maps to
        let x_prime = u * (1.0 - v * v / 2.0).sqrt();
        let y_prime = v * (1.0 - u * u / 2.0).sqrt();
        let z_prime = (0.0_f64.max(1.0 - u * u / 2.0 - v * v / 2.0)).sqrt();
        
        let (x_pre, y_pre, z_pre) = match face {
            0 => (z_prime, -x_prime, -y_prime),
            1 => (-z_prime, x_prime, -y_prime),
            2 => (x_prime, z_prime, -y_prime),
            3 => (-x_prime, -z_prime, -y_prime),
            4 => (x_prime, y_prime, z_prime),
            5 => (-x_prime, y_prime, -z_prime),
            _ => (0.0, 0.0, 0.0),
        };
        
        // Normalize
        let mag = (x_pre * x_pre + y_pre * y_pre + z_pre * z_pre).sqrt();
        let x_current = x_pre / mag;
        let y_current = y_pre / mag;
        let z_current = z_pre / mag;
        
        // Compute error
        let error_x = x_target - x_current;
        let error_y = y_target - y_current;
        let error_z = z_target - z_current;
        
        let error_mag = (error_x * error_x + error_y * error_y + error_z * error_z).sqrt();
        
        if error_mag < 1e-10 {
            break; // Converged
        }
        
        // Compute Jacobian using finite differences
        let delta = 0.0001;
        
        // Perturb u
        let ecef_plus_u = cube_to_sphere(face, u + delta, v, 1.0);
        let dxdu = (ecef_plus_u.x - x_current) / delta;
        let dydu = (ecef_plus_u.y - y_current) / delta;
        let dzdu = (ecef_plus_u.z - z_current) / delta;
        
        // Perturb v
        let ecef_plus_v = cube_to_sphere(face, u, v + delta, 1.0);
        let dxdv = (ecef_plus_v.x - x_current) / delta;
        let dydv = (ecef_plus_v.y - y_current) / delta;
        let dzdv = (ecef_plus_v.z - z_current) / delta;
        
        // Solve least-squares problem (3 equations, 2 unknowns)
        // Minimize ||J * [du, dv]^T - error||^2
        // Normal equations: J^T J [du, dv]^T = J^T error
        
        let j11 = dxdu * dxdu + dydu * dydu + dzdu * dzdu;
        let j12 = dxdu * dxdv + dydu * dydv + dzdu * dzdv;
        let j22 = dxdv * dxdv + dydv * dydv + dzdv * dzdv;
        
        let r1 = dxdu * error_x + dydu * error_y + dzdu * error_z;
        let r2 = dxdv * error_x + dydv * error_y + dzdv * error_z;
        
        let det = j11 * j22 - j12 * j12;
        
        if det.abs() > 1e-10 {
            let du = (r1 * j22 - r2 * j12) / det;
            let dv = (r2 * j11 - r1 * j12) / det;
            
            u += du;
            v += dv;
            
            u = u.clamp(-1.0, 1.0);
            v = v.clamp(-1.0, 1.0);
        } else {
            break;
        }
    }
    
    (face, u, v)
}

/// Converts GPS position to ChunkId at specified depth
///
/// # Arguments
/// * `gps` - GPS position (latitude, longitude, elevation)
/// * `depth` - Quadtree depth (0 = face-level, higher = more subdivisions)
///
/// # Returns
/// ChunkId identifying the tile containing this position
///
/// # Algorithm
/// 1. Convert GPS → ECEF
/// 2. Determine cube face and UV coordinates
/// 3. Recursively subdivide UV space into quadrants to specified depth
///    - Quadrant 0: top-left (u < mid, v < mid)
///    - Quadrant 1: top-right (u >= mid, v < mid)
///    - Quadrant 2: bottom-left (u < mid, v >= mid)
///    - Quadrant 3: bottom-right (u >= mid, v >= mid)
///
/// # Examples
/// ```
/// use metaverse_core::chunks::gps_to_chunk_id;
/// use metaverse_core::coordinates::GpsPos;
///
/// let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
/// let chunk = gps_to_chunk_id(&brisbane, 8);
/// assert_eq!(chunk.depth(), 8);
/// ```
pub fn gps_to_chunk_id(gps: &GpsPos, depth: u8) -> ChunkId {
    // Convert to ECEF
    let ecef = gps_to_ecef(gps);
    
    // Get cube face and UV coordinates
    let (face, mut u, mut v) = ecef_to_cube_face(&ecef);
    
    // UV coordinates are in [-1, 1], subdivide into quadtree
    let mut path = Vec::with_capacity(depth as usize);
    
    // Current UV bounds for subdivision
    let mut u_min = -1.0;
    let mut u_max = 1.0;
    let mut v_min = -1.0;
    let mut v_max = 1.0;
    
    // Recursively subdivide
    for _ in 0..depth {
        let u_mid = (u_min + u_max) / 2.0;
        let v_mid = (v_min + v_max) / 2.0;
        
        // Determine which quadrant (0-3)
        let quadrant = if u < u_mid {
            if v < v_mid {
                // Top-left
                u_max = u_mid;
                v_max = v_mid;
                0
            } else {
                // Bottom-left
                u_max = u_mid;
                v_min = v_mid;
                2
            }
        } else {
            if v < v_mid {
                // Top-right
                u_min = u_mid;
                v_max = v_mid;
                1
            } else {
                // Bottom-right
                u_min = u_mid;
                v_min = v_mid;
                3
            }
        };
        
        path.push(quadrant);
    }
    
    ChunkId { face, path }
}

/// Returns the ECEF coordinates of the center of a chunk tile
///
/// # Arguments
/// * `id` - The ChunkId to get the center of
///
/// # Returns
/// ECEF position of the tile center on the sphere surface
pub fn chunk_center_ecef(id: &ChunkId) -> EcefPos {
    // Get the UV bounds of this tile
    let (u_min, u_max, v_min, v_max) = chunk_uv_bounds(id);
    
    // Center is at midpoint
    let u_center = (u_min + u_max) / 2.0;
    let v_center = (v_min + v_max) / 2.0;
    
    // Project to sphere
    cube_to_sphere(id.face, u_center, v_center, crate::coordinates::WGS84_A)
}

/// Returns the ECEF coordinates of the 4 corners of a chunk tile
///
/// # Arguments
/// * `id` - The ChunkId to get corners for
///
/// # Returns
/// Array of 4 ECEF positions (corners in order: TL, TR, BL, BR)
pub fn chunk_corners_ecef(id: &ChunkId) -> [EcefPos; 4] {
    use crate::coordinates::WGS84_A;
    
    let (u_min, u_max, v_min, v_max) = chunk_uv_bounds(id);
    
    // Four corners: TL, TR, BL, BR
    [
        cube_to_sphere(id.face, u_min, v_min, WGS84_A), // Top-left
        cube_to_sphere(id.face, u_max, v_min, WGS84_A), // Top-right
        cube_to_sphere(id.face, u_min, v_max, WGS84_A), // Bottom-left
        cube_to_sphere(id.face, u_max, v_max, WGS84_A), // Bottom-right
    ]
}

/// Returns the radius of the smallest sphere that contains the entire tile
///
/// # Arguments
/// * `id` - The ChunkId to compute bounding radius for
///
/// # Returns
/// Radius in meters from the tile center that contains all corners
pub fn chunk_bounding_radius(id: &ChunkId) -> f64 {
    let center = chunk_center_ecef(id);
    let corners = chunk_corners_ecef(id);
    
    // Find maximum distance from center to any corner
    let mut max_dist: f64 = 0.0;
    for corner in &corners {
        let dist = ((corner.x - center.x).powi(2)
                  + (corner.y - center.y).powi(2)
                  + (corner.z - center.z).powi(2)).sqrt();
        max_dist = max_dist.max(dist);
    }
    
    max_dist
}

/// Returns approximate width of the tile in meters
///
/// # Arguments
/// * `id` - The ChunkId to compute width for
///
/// # Returns
/// Approximate edge length in meters (average of distances between adjacent corners)
pub fn chunk_approximate_width(id: &ChunkId) -> f64 {
    let corners = chunk_corners_ecef(id);
    
    // Measure distances between adjacent corners
    // TL-TR, TR-BR, BR-BL, BL-TL
    let d1 = ecef_distance(&corners[0], &corners[1]); // TL-TR (top edge)
    let d2 = ecef_distance(&corners[1], &corners[3]); // TR-BR (right edge)
    let d3 = ecef_distance(&corners[3], &corners[2]); // BR-BL (bottom edge)
    let d4 = ecef_distance(&corners[2], &corners[0]); // BL-TL (left edge)
    
    // Return average edge length
    (d1 + d2 + d3 + d4) / 4.0
}

/// Helper: compute UV bounds for a ChunkId
///
/// Returns (u_min, u_max, v_min, v_max) in the range [-1, 1]
fn chunk_uv_bounds(id: &ChunkId) -> (f64, f64, f64, f64) {
    let mut u_min = -1.0;
    let mut u_max = 1.0;
    let mut v_min = -1.0;
    let mut v_max = 1.0;
    
    // Apply each quadrant subdivision in the path
    for &quadrant in &id.path {
        let u_mid = (u_min + u_max) / 2.0;
        let v_mid = (v_min + v_max) / 2.0;
        
        match quadrant {
            0 => { // Top-left
                u_max = u_mid;
                v_max = v_mid;
            }
            1 => { // Top-right
                u_min = u_mid;
                v_max = v_mid;
            }
            2 => { // Bottom-left
                u_max = u_mid;
                v_min = v_mid;
            }
            3 => { // Bottom-right
                u_min = u_mid;
                v_min = v_mid;
            }
            _ => {} // Invalid, ignore
        }
    }
    
    (u_min, u_max, v_min, v_max)
}

/// Helper: compute distance between two ECEF positions
fn ecef_distance(a: &EcefPos, b: &EcefPos) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Returns the parent tile of a given ChunkId
///
/// # Arguments
/// * `id` - The ChunkId to get the parent of
///
/// # Returns
/// Some(parent) if depth > 0, None if depth == 0
///
/// The parent is simply the tile with path truncated by one element.
pub fn chunk_parent(id: &ChunkId) -> Option<ChunkId> {
    if id.path.is_empty() {
        // Depth 0 has no parent
        None
    } else {
        // Parent has same face, path with last element removed
        let mut parent_path = id.path.clone();
        parent_path.pop();
        Some(ChunkId {
            face: id.face,
            path: parent_path,
        })
    }
}

/// Returns the 4 children of a given ChunkId
///
/// # Arguments
/// * `id` - The ChunkId to get children for
///
/// # Returns
/// Array of 4 ChunkIds representing the quadrants (0=TL, 1=TR, 2=BL, 3=BR)
///
/// Each child has the same face, with path extended by one quadrant index.
pub fn chunk_children(id: &ChunkId) -> [ChunkId; 4] {
    let mut children = Vec::with_capacity(4);
    
    for quadrant in 0..4 {
        let mut child_path = id.path.clone();
        child_path.push(quadrant);
        children.push(ChunkId {
            face: id.face,
            path: child_path,
        });
    }
    
    [
        children[0].clone(),
        children[1].clone(),
        children[2].clone(),
        children[3].clone(),
    ]
}

/// Tests if a GPS position is contained within a given chunk tile
///
/// # Arguments
/// * `id` - The ChunkId to test
/// * `gps` - The GPS position to check
///
/// # Returns
/// true if the point is inside the tile, false otherwise
///
/// A point is inside a tile if converting it to a ChunkId at the same depth
/// produces the same ChunkId.
pub fn chunk_contains_gps(id: &ChunkId, gps: &GpsPos) -> bool {
    let point_chunk = gps_to_chunk_id(gps, id.depth() as u8);
    point_chunk == *id
}

// ============================================================================
// Phase 2.6: Neighbour queries
// ============================================================================

/// Face adjacency table for cube projection
/// Each face has 4 edges (top, right, bottom, left in UV space)
/// For each edge, we store: (adjacent_face, u_transform, v_transform)
/// 
/// UV space on each face goes from -1 to +1
/// Edges are at u=-1 (left), u=+1 (right), v=-1 (bottom), v=+1 (top)
///
/// Face layout (cube net):
///     [4]
/// [3] [0] [2] [1]
///     [5]
///
/// Faces: 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
#[derive(Debug, Copy, Clone)]
struct FaceEdge {
    face: u8,
    /// Transform u coordinate: 0=keep u, 1=keep v, 2=negate u, 3=negate v
    u_map: u8,
    /// Transform v coordinate: 0=keep v, 1=keep u, 2=negate v, 3=negate u
    v_map: u8,
}

/// Adjacency table: [face][edge] -> FaceEdge
/// Edge order: 0=left (-u), 1=right (+u), 2=bottom (-v), 3=top (+v)
const FACE_ADJACENCY: [[FaceEdge; 4]; 6] = [
    // Face 0 (+X): left=3(-Y), right=2(+Y), bottom=5(-Z), top=4(+Z)
    [
        FaceEdge { face: 3, u_map: 1, v_map: 0 },  // left -> -Y (v->u, v->v)
        FaceEdge { face: 2, u_map: 3, v_map: 0 },  // right -> +Y (-v->u, v->v)
        FaceEdge { face: 5, u_map: 0, v_map: 1 },  // bottom -> -Z (u->u, u->v)
        FaceEdge { face: 4, u_map: 0, v_map: 3 },  // top -> +Z (u->u, -u->v)
    ],
    // Face 1 (-X): left=2(+Y), right=3(-Y), bottom=5(-Z), top=4(+Z)
    [
        FaceEdge { face: 2, u_map: 1, v_map: 0 },  // left -> +Y (v->u, v->v)
        FaceEdge { face: 3, u_map: 3, v_map: 0 },  // right -> -Y (-v->u, v->v)
        FaceEdge { face: 5, u_map: 2, v_map: 1 },  // bottom -> -Z (-u->u, u->v)
        FaceEdge { face: 4, u_map: 2, v_map: 3 },  // top -> +Z (-u->u, -u->v)
    ],
    // Face 2 (+Y): left=0(+X), right=1(-X), bottom=5(-Z), top=4(+Z)
    [
        FaceEdge { face: 0, u_map: 1, v_map: 0 },  // left -> +X (v->u, v->v)
        FaceEdge { face: 1, u_map: 3, v_map: 0 },  // right -> -X (-v->u, v->v)
        FaceEdge { face: 5, u_map: 1, v_map: 2 },  // bottom -> -Z (v->u, -v->v)
        FaceEdge { face: 4, u_map: 1, v_map: 0 },  // top -> +Z (v->u, v->v)
    ],
    // Face 3 (-Y): left=1(-X), right=0(+X), bottom=5(-Z), top=4(+Z)
    [
        FaceEdge { face: 1, u_map: 1, v_map: 0 },  // left -> -X (v->u, v->v)
        FaceEdge { face: 0, u_map: 3, v_map: 0 },  // right -> +X (-v->u, v->v)
        FaceEdge { face: 5, u_map: 3, v_map: 0 },  // bottom -> -Z (-v->u, v->v)
        FaceEdge { face: 4, u_map: 3, v_map: 2 },  // top -> +Z (-v->u, -v->v)
    ],
    // Face 4 (+Z): left=3(-Y), right=2(+Y), bottom=0(+X), top=1(-X)
    [
        FaceEdge { face: 3, u_map: 0, v_map: 2 },  // left -> -Y (u->u, -v->v)
        FaceEdge { face: 2, u_map: 0, v_map: 0 },  // right -> +Y (u->u, v->v)
        FaceEdge { face: 0, u_map: 0, v_map: 1 },  // bottom -> +X (u->u, u->v)
        FaceEdge { face: 1, u_map: 2, v_map: 1 },  // top -> -X (-u->u, u->v)
    ],
    // Face 5 (-Z): left=3(-Y), right=2(+Y), bottom=1(-X), top=0(+X)
    [
        FaceEdge { face: 3, u_map: 0, v_map: 0 },  // left -> -Y (u->u, v->v)
        FaceEdge { face: 2, u_map: 0, v_map: 2 },  // right -> +Y (u->u, -v->v)
        FaceEdge { face: 1, u_map: 2, v_map: 3 },  // bottom -> -X (-u->u, -u->v)
        FaceEdge { face: 0, u_map: 0, v_map: 3 },  // top -> +X (u->u, -u->v)
    ],
];

/// Returns the 4 edge-adjacent neighbours of a chunk
///
/// Neighbours are at the same depth and share an edge with the input chunk.
/// This includes cross-face neighbours at face boundaries.
///
/// # Arguments
/// * `id` - The chunk to find neighbours for
///
/// # Returns
/// A vector of exactly 4 ChunkIds representing the edge-adjacent neighbours
pub fn chunk_neighbors(id: &ChunkId) -> Vec<ChunkId> {
    let mut neighbors = Vec::with_capacity(4);
    let (u_min, u_max, v_min, v_max) = chunk_uv_bounds(id);
    let u_mid = (u_min + u_max) / 2.0;
    let v_mid = (v_min + v_max) / 2.0;
    let u_size = u_max - u_min;
    let v_size = v_max - v_min;
    
    // Try to build 4 neighbours: left, right, bottom, top
    
    // Left neighbour (u - size)
    if u_min > -1.0 + 1e-9 {
        // Same face
        neighbors.push(build_chunk_from_uv(id.face, u_mid - u_size, v_mid, id.depth()));
    } else {
        // Cross-face: left edge
        neighbors.push(cross_face_neighbor(id, 0));
    }
    
    // Right neighbour (u + size)
    if u_max < 1.0 - 1e-9 {
        // Same face
        neighbors.push(build_chunk_from_uv(id.face, u_mid + u_size, v_mid, id.depth()));
    } else {
        // Cross-face: right edge
        neighbors.push(cross_face_neighbor(id, 1));
    }
    
    // Bottom neighbour (v - size)
    if v_min > -1.0 + 1e-9 {
        // Same face
        neighbors.push(build_chunk_from_uv(id.face, u_mid, v_mid - v_size, id.depth()));
    } else {
        // Cross-face: bottom edge
        neighbors.push(cross_face_neighbor(id, 2));
    }
    
    // Top neighbour (v + size)
    if v_max < 1.0 - 1e-9 {
        // Same face
        neighbors.push(build_chunk_from_uv(id.face, u_mid, v_mid + v_size, id.depth()));
    } else {
        // Cross-face: top edge
        neighbors.push(cross_face_neighbor(id, 3));
    }
    
    neighbors
}

/// Helper: builds a ChunkId from a UV position on a given face
fn build_chunk_from_uv(face: u8, u: f64, v: f64, depth: usize) -> ChunkId {
    let mut path = Vec::with_capacity(depth);
    let mut u_min = -1.0;
    let mut u_max = 1.0;
    let mut v_min = -1.0;
    let mut v_max = 1.0;
    
    for _ in 0..depth {
        let u_mid = (u_min + u_max) / 2.0;
        let v_mid = (v_min + v_max) / 2.0;
        
        let quadrant = if v >= v_mid {
            if u < u_mid { 0 } else { 1 }
        } else {
            if u < u_mid { 2 } else { 3 }
        };
        
        path.push(quadrant);
        
        if u < u_mid { u_max = u_mid; } else { u_min = u_mid; }
        if v < v_mid { v_max = v_mid; } else { v_min = v_mid; }
    }
    
    ChunkId { face, path }
}

/// Helper: finds neighbour across a face boundary
fn cross_face_neighbor(id: &ChunkId, edge: usize) -> ChunkId {
    let adjacency = FACE_ADJACENCY[id.face as usize][edge];
    let (u_min, u_max, v_min, v_max) = chunk_uv_bounds(id);
    let u_mid = (u_min + u_max) / 2.0;
    let v_mid = (v_min + v_max) / 2.0;
    
    // Transform UV coordinates to adjacent face's coordinate system
    let new_u = match adjacency.u_map {
        0 => u_mid,      // keep u
        1 => v_mid,      // v -> u
        2 => -u_mid,     // negate u
        3 => -v_mid,     // negate v
        _ => unreachable!(),
    };
    
    let new_v = match adjacency.v_map {
        0 => v_mid,      // keep v
        1 => u_mid,      // u -> v
        2 => -v_mid,     // negate v
        3 => -u_mid,     // negate u
        _ => unreachable!(),
    };
    
    build_chunk_from_uv(adjacency.face, new_u, new_v, id.depth())
}
