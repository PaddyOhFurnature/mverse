// Quad-sphere chunking system for spherical Earth
// Hierarchical tile addressing using cube-to-sphere projection

use std::fmt;

use crate::coordinates::EcefPos;

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
