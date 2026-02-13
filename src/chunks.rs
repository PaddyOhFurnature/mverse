// Quad-sphere chunking system for spherical Earth
// Hierarchical tile addressing using cube-to-sphere projection

use std::fmt;

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
