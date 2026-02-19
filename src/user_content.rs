//! User-generated content layer
//!
//! Separates user modifications from base terrain generation.
//! Implements CRDT-based conflict resolution and operation logging.
//!
//! # Chunk-Based Storage
//!
//! Operations are stored per-chunk for spatial sharding:
//! - `world_data/chunks/chunk_X_Y_Z/operations.json`
//! - Only load operations for nearby chunks
//! - Scales to infinite world size
//! - Foundation for DHT replication

use crate::{
    chunk::ChunkId,
    messages::VoxelOperation,
    voxel::VoxelCoord,
};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuration flags for verification (can be toggled for testing)
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Verify Ed25519 signatures on operations
    pub verify_signatures: bool,
    
    /// Verify parcel ownership permissions
    pub verify_permissions: bool,
    
    /// Enable operation logging
    pub enable_logging: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            verify_signatures: true,
            verify_permissions: false, // Not implemented yet
            enable_logging: true,
        }
    }
}

/// Parcel ownership bounds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParcelBounds {
    pub min: VoxelCoord,
    pub max: VoxelCoord,
}

impl ParcelBounds {
    pub fn new(min: VoxelCoord, max: VoxelCoord) -> Self {
        Self { min, max }
    }
    
    pub fn contains(&self, coord: &VoxelCoord) -> bool {
        coord.x >= self.min.x && coord.x <= self.max.x
            && coord.y >= self.min.y && coord.y <= self.max.y
            && coord.z >= self.min.z && coord.z <= self.max.z
    }
}

/// User content layer - stores modifications separate from base terrain
///
/// This layer manages the operation log and permission checking.
/// Actual voxel storage still happens in the main Octree for now.
#[derive(Debug, Clone)]
pub struct UserContentLayer {
    /// Operation log for this chunk (append-only)
    op_log: Vec<VoxelOperation>,
    
    /// Parcel ownership map
    parcels: HashMap<ParcelBounds, PeerId>,
    
    /// Access grants
    access_grants: HashMap<(ParcelBounds, PeerId), bool>,
    
    /// Verification configuration
    config: VerificationConfig,
}

impl UserContentLayer {
    /// Create a new empty user content layer
    pub fn new() -> Self {
        Self::with_config(VerificationConfig::default())
    }
    
    /// Create with custom verification config
    pub fn with_config(config: VerificationConfig) -> Self {
        Self {
            op_log: Vec::new(),
            parcels: HashMap::new(),
            access_grants: HashMap::new(),
            config,
        }
    }
    
    /// Apply a voxel operation to the user content layer
    ///
    /// Returns Ok(true) if operation was applied,
    /// Ok(false) if operation was rejected (conflict resolution),
    /// Err if operation is invalid (bad signature, unauthorized, etc.)
    pub fn apply_operation(
        &mut self,
        op: VoxelOperation,
        local_ops: &HashMap<VoxelCoord, VoxelOperation>,
    ) -> Result<bool, ApplyError> {
        // 1. Verify signature (if enabled)
        if self.config.verify_signatures {
            if !op.verify_signature() {
                return Err(ApplyError::InvalidSignature);
            }
        }
        
        // 2. CRDT conflict resolution: check for conflicting local operation
        if let Some(local_op) = local_ops.get(&op.coord) {
            if !op.wins_over(local_op) {
                // Local operation wins, reject remote
                return Ok(false);
            }
        }
        
        // 3. Check permission (if enabled)
        if self.config.verify_permissions {
            if !self.check_permission(&op) {
                return Err(ApplyError::Unauthorized);
            }
        }
        
        // 4. Append to operation log (if enabled)
        if self.config.enable_logging {
            self.op_log.push(op);
        }
        
        Ok(true)
    }
    
    /// Get the operation log
    pub fn op_log(&self) -> &[VoxelOperation] {
        &self.op_log
    }
    
    /// Get operation count
    pub fn op_count(&self) -> usize {
        self.op_log.len()
    }
    
    /// Clear operation log (for testing/reset)
    pub fn clear(&mut self) {
        self.op_log.clear();
    }
    
    /// Claim a parcel (for future permission system)
    pub fn claim_parcel(&mut self, owner: PeerId, bounds: ParcelBounds) -> Result<(), ClaimError> {
        // Check if any part of this parcel is already claimed
        for (existing_bounds, _) in &self.parcels {
            if bounds_overlap(&bounds, existing_bounds) {
                return Err(ClaimError::AlreadyClaimed);
            }
        }
        
        self.parcels.insert(bounds, owner);
        Ok(())
    }
    
    /// Get parcel owner for a coordinate (if in a parcel)
    pub fn get_parcel_owner(&self, coord: &VoxelCoord) -> Option<PeerId> {
        for (bounds, owner) in &self.parcels {
            if bounds.contains(coord) {
                return Some(*owner);
            }
        }
        None
    }
    
    /// Grant access to a parcel
    pub fn grant_access(&mut self, parcel: ParcelBounds, grantee: PeerId) {
        self.access_grants.insert((parcel, grantee), true);
    }
    
    /// Check if a peer has access to a coordinate
    pub fn has_access(&self, peer: PeerId, coord: &VoxelCoord) -> bool {
        for (bounds, owner) in &self.parcels {
            if bounds.contains(coord) {
                // Owner always has access
                if *owner == peer {
                    return true;
                }
                
                // Check for granted access
                return self.access_grants.get(&(*bounds, peer)).copied().unwrap_or(false);
            }
        }
        
        // Not in any parcel - assume free-build zone
        true
    }
    
    /// Check if operation is permitted
    fn check_permission(&self, op: &VoxelOperation) -> bool {
        // Check if author has permission to edit this coordinate
        self.has_access(op.author, &op.coord)
    }
    
    /// Save operation log to disk (legacy single-file format)
    ///
    /// **Deprecated:** Use `save_chunks()` for chunk-based storage
    pub fn save_op_log<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.op_log)?;
        std::fs::write(path, json)?;
        Ok(())
    }
    
    /// Load operation log from disk (legacy single-file format)
    ///
    /// **Deprecated:** Use `load_chunks()` for chunk-based storage
    ///
    /// Note: Operations are loaded but NOT applied - caller must replay them
    pub fn load_op_log<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<usize> {
        let json = std::fs::read_to_string(path)?;
        let ops: Vec<VoxelOperation> = serde_json::from_str(&json)?;
        let count = ops.len();
        self.op_log = ops;
        Ok(count)
    }
    
    /// Save operations organized by chunk
    ///
    /// Creates directory structure:
    /// ```text
    /// {base_dir}/
    ///   chunks/
    ///     chunk_0_0_0/
    ///       operations.json
    ///     chunk_1_0_0/
    ///       operations.json
    /// ```
    ///
    /// # Arguments
    /// * `base_dir` - Base directory (e.g., "world_data")
    ///
    /// # Returns
    /// HashMap mapping chunk ID to number of operations saved
    pub fn save_chunks<P: AsRef<Path>>(&self, base_dir: P) -> std::io::Result<HashMap<ChunkId, usize>> {
        let chunks_dir = base_dir.as_ref().join("chunks");
        
        // Group operations by chunk
        let mut ops_by_chunk: HashMap<ChunkId, Vec<&VoxelOperation>> = HashMap::new();
        for op in &self.op_log {
            let chunk_id = ChunkId::from_voxel(&op.coord);
            ops_by_chunk.entry(chunk_id).or_insert_with(Vec::new).push(op);
        }
        
        let mut result = HashMap::new();
        
        // Save each chunk's operations to its own file
        for (chunk_id, ops) in ops_by_chunk {
            let chunk_dir = chunks_dir.join(chunk_id.to_path_string());
            std::fs::create_dir_all(&chunk_dir)?;
            
            let ops_file = chunk_dir.join("operations.json");
            let json = serde_json::to_string_pretty(&ops)?;
            std::fs::write(&ops_file, json)?;
            
            result.insert(chunk_id, ops.len());
        }
        
        Ok(result)
    }
    
    /// Load operations from a specific chunk
    ///
    /// # Arguments
    /// * `base_dir` - Base directory (e.g., "world_data")
    /// * `chunk_id` - Which chunk to load
    ///
    /// # Returns
    /// Number of operations loaded
    pub fn load_chunk<P: AsRef<Path>>(&mut self, base_dir: P, chunk_id: &ChunkId) -> std::io::Result<usize> {
        let ops_file = base_dir.as_ref()
            .join("chunks")
            .join(chunk_id.to_path_string())
            .join("operations.json");
        
        // If file doesn't exist, that's OK (chunk has no edits)
        if !ops_file.exists() {
            return Ok(0);
        }
        
        let json = std::fs::read_to_string(ops_file)?;
        let ops: Vec<VoxelOperation> = serde_json::from_str(&json)?;
        let count = ops.len();
        
        // Append to existing op log
        self.op_log.extend(ops);
        
        Ok(count)
    }
    
    /// Load operations from multiple chunks
    ///
    /// Useful for loading all chunks in player's view radius.
    ///
    /// # Arguments
    /// * `base_dir` - Base directory (e.g., "world_data")
    /// * `chunk_ids` - List of chunks to load
    ///
    /// # Returns
    /// HashMap mapping chunk ID to number of operations loaded
    pub fn load_chunks<P: AsRef<Path>>(&mut self, base_dir: P, chunk_ids: &[ChunkId]) -> std::io::Result<HashMap<ChunkId, usize>> {
        let mut result = HashMap::new();
        
        for chunk_id in chunk_ids {
            match self.load_chunk(base_dir.as_ref(), chunk_id) {
                Ok(count) => {
                    if count > 0 {
                        result.insert(*chunk_id, count);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Chunk file doesn't exist - that's OK
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        
        Ok(result)
    }
    
    /// Get all chunks that have operations
    ///
    /// Useful for discovering which chunks need to be saved.
    ///
    /// # Returns
    /// HashMap mapping chunk ID to operations in that chunk
    pub fn get_chunks_with_ops(&self) -> HashMap<ChunkId, Vec<VoxelOperation>> {
        let mut chunks: HashMap<ChunkId, Vec<VoxelOperation>> = HashMap::new();
        
        for op in &self.op_log {
            let chunk_id = ChunkId::from_voxel(&op.coord);
            chunks.entry(chunk_id).or_insert_with(Vec::new).push(op.clone());
        }
        
        chunks
    }
}

impl Default for UserContentLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Error when applying an operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Signature verification failed
    InvalidSignature,
    
    /// Author doesn't have permission to edit this coordinate
    Unauthorized,
}

/// Error when claiming a parcel
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// Parcel area already claimed by someone else
    AlreadyClaimed,
}

/// Check if two parcel bounds overlap
fn bounds_overlap(a: &ParcelBounds, b: &ParcelBounds) -> bool {
    !(a.max.x < b.min.x || a.min.x > b.max.x
        || a.max.y < b.min.y || a.min.y > b.max.y
        || a.max.z < b.min.z || a.min.z > b.max.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::Material;
    
    #[test]
    fn test_user_content_layer() {
        let mut layer = UserContentLayer::new();
        let local_ops = HashMap::new();
        
        // Disable signature verification for test
        layer.config.verify_signatures = false;
        
        let op = VoxelOperation::new(
            VoxelCoord::new(10, 20, 30),
            Material::Stone,
            PeerId::random(),
            1,
            crate::vector_clock::VectorClock::new(),
        );
        
        let result = layer.apply_operation(op, &local_ops);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
        
        // Check that operation was logged
        assert_eq!(layer.op_count(), 1);
    }
    
    #[test]
    fn test_crdt_conflict_resolution() {
        let mut layer = UserContentLayer::new();
        layer.config.verify_signatures = false;
        
        let coord = VoxelCoord::new(5, 5, 5);
        
        // Local operation with timestamp 10
        let local_op = VoxelOperation::new(
            coord, 
            Material::Stone, 
            PeerId::random(), 
            10,
            crate::vector_clock::VectorClock::new(),
        );
        let mut local_ops = HashMap::new();
        local_ops.insert(coord, local_op);
        
        // Remote operation with timestamp 5 (older)
        let remote_op = VoxelOperation::new(
            coord, 
            Material::Dirt, 
            PeerId::random(), 
            5,
            crate::vector_clock::VectorClock::new(),
        );
        
        // Remote operation should be rejected (local wins)
        let result = layer.apply_operation(remote_op, &local_ops);
        assert_eq!(result.unwrap(), false);
    }
    
    #[test]
    fn test_parcel_overlap() {
        let bounds1 = ParcelBounds::new(
            VoxelCoord::new(0, 0, 0),
            VoxelCoord::new(10, 10, 10),
        );
        
        let bounds2 = ParcelBounds::new(
            VoxelCoord::new(5, 5, 5),
            VoxelCoord::new(15, 15, 15),
        );
        
        let bounds3 = ParcelBounds::new(
            VoxelCoord::new(20, 20, 20),
            VoxelCoord::new(30, 30, 30),
        );
        
        assert!(bounds_overlap(&bounds1, &bounds2));
        assert!(!bounds_overlap(&bounds1, &bounds3));
    }
}
