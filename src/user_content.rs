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
    messages::{VoxelOperation, SignedOperation},
    permissions::{action_to_class, check_record_permission, PermissionConfig, PermissionResult},
    voxel::VoxelCoord,
};
use crate::identity::{KeyRecord};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

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
    op_log: Vec<SignedOperation>,
    
    /// Parcel ownership map
    parcels: HashMap<ParcelBounds, PeerId>,
    
    /// Access grants
    access_grants: HashMap<(ParcelBounds, PeerId), bool>,
    
    /// Permission configuration (replaces old VerificationConfig)
    pub config: PermissionConfig,
}

impl UserContentLayer {
    /// Create a new empty user content layer
    pub fn new() -> Self {
        Self::with_config(PermissionConfig::default())
    }
    
    /// Create with custom permission config
    pub fn with_config(config: PermissionConfig) -> Self {
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
        op: SignedOperation,
        local_ops: &HashMap<VoxelCoord, SignedOperation>,
    ) -> Result<bool, ApplyError> {
        // 1. Verify Ed25519 signature (if enabled)
        if self.config.verify_signatures {
            if !op.verify() {
                return Err(ApplyError::InvalidSignature);
            }
        }

        // 2. Resolve the coord for this op (non-terrain ops bypass octree conflict check)
        let op_coord = match op.coord() {
            Some(c) => c,
            None => {
                // Non-terrain op: add to log, no octree conflict
                self.op_log.push(op);
                return Ok(true);
            }
        };

        // 3. CRDT conflict resolution: check for conflicting local operation
        if let Some(local_op) = local_ops.get(&op_coord) {
            if !op.wins_over(local_op) {
                return Ok(false);
            }
        }
        
        // 4. Permission check via key-type table (if enabled)
        // Uses a synthetic Guest record when author's real record is unknown
        if self.config.verify_key_types || self.config.verify_expiry || self.config.verify_revocation {
            // Build a minimal guest record for checking key-type permissions
            // (full sig + registry check happens in multiplayer.rs before ops reach here)
            let guest_record = KeyRecord {
                version: 1,
                peer_id: op.author,
                public_key: op.public_key,
                key_type: crate::identity::KeyType::Personal, // assume Personal for layer check
                display_name: None,
                bio: None,
                avatar_hash: None,
                created_at: 0,
                expires_at: None,
                updated_at: 0,
                issued_by: None,
                issuer_sig: None,
                revoked: false,
                revoked_at: None,
                revoked_by: None,
                revocation_reason: None,
                self_sig: [0u8; 64],
            };
            let class = action_to_class(&op.action);
            let result = check_record_permission(&guest_record, class, &self.config);
            if result.is_denied() {
                return Err(ApplyError::Unauthorized);
            }
        }

        // 5. Spatial ownership check (if enabled)
        if self.config.verify_ownership {
            if !self.check_ownership(&op) {
                return Err(ApplyError::Unauthorized);
            }
        }
        
        // 6. Append to operation log
        self.op_log.push(op);
        
        Ok(true)
    }
    
    /// Get all operations affecting a chunk (for applying on load)
    pub fn operations_for_chunk(&self, chunk_id: &ChunkId) -> Vec<&SignedOperation> {
        self.op_log.iter()
            .filter(|op| {
                op.coord().map(|c| ChunkId::from_voxel(&c) == *chunk_id).unwrap_or(false)
            })
            .collect()
    }
    
    /// Add a local operation to the log
    ///
    /// Use this for operations created by the local player (already verified by
    /// the multiplayer layer before being passed here). For received operations
    /// from the network, use apply_operation().
    ///
    /// Returns `PermissionResult::Allowed` on success, or the denial reason.
    /// The caller is responsible for not applying denied ops to the octree.
    pub fn add_local_operation(&mut self, op: SignedOperation) -> PermissionResult {
        // Build a minimal record from the op's embedded public key for key-type check
        let record = KeyRecord {
            version: 1,
            peer_id: op.author,
            public_key: op.public_key,
            key_type: crate::identity::KeyType::Personal, // local ops are at least Personal
            display_name: None,
            bio: None,
            avatar_hash: None,
            created_at: 0,
            expires_at: None,
            updated_at: 0,
            issued_by: None,
            issuer_sig: None,
            revoked: false,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
            self_sig: [0u8; 64],
        };
        let class = action_to_class(&op.action);
        let result = check_record_permission(&record, class, &self.config);
        if result.is_allowed() {
            self.op_log.push(op);
        }
        result
    }
    
    /// Get the operation log
    pub fn op_log(&self) -> &[SignedOperation] {
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
    
    /// Check if operation is permitted based on parcel ownership
    fn check_ownership(&self, op: &SignedOperation) -> bool {
        op.coord().map(|c| self.has_access(op.author, &c)).unwrap_or(true)
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
        let ops: Vec<SignedOperation> = serde_json::from_str(&json)?;
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
        
        // Group operations by ALL affected chunks (not just the chunk containing the voxel)
        let mut ops_by_chunk: HashMap<ChunkId, Vec<&SignedOperation>> = HashMap::new();
        
        for op in &self.op_log {
            // Get all chunks that need this operation for proper mesh generation
            let affected_chunks = op.affecting_chunks();
            
            for chunk_id in affected_chunks {
                ops_by_chunk.entry(chunk_id).or_insert_with(Vec::new).push(op);
            }
        }
        
        let mut result = HashMap::new();
        
        // Save each chunk's operations to its own file (binary format)
        for (chunk_id, ops) in ops_by_chunk {
            let chunk_dir = chunks_dir.join(chunk_id.to_path_string());
            std::fs::create_dir_all(&chunk_dir)?;
            
            // Convert to owned Vec and sort in causal replay order
            let mut ops_owned: Vec<SignedOperation> = ops.iter().map(|&op| op.clone()).collect();
            ops_owned.sort_by(|a, b| a.replay_cmp(b));
            
            let ops_file = chunk_dir.join("operations.bin");
            let bytes = bincode::serialize(&ops_owned)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            std::fs::write(&ops_file, bytes)?;
            
            result.insert(chunk_id, ops_owned.len());
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
            .join("operations.bin");
        
        // If file doesn't exist, that's OK (chunk has no edits)
        if !ops_file.exists() {
            return Ok(0);
        }
        
        let bytes = std::fs::read(ops_file)?;

        // Try new SignedOperation format first
        if let Ok(ops) = bincode::deserialize::<Vec<SignedOperation>>(&bytes) {
            let count = ops.len();
            self.op_log.extend(ops);
            return Ok(count);
        }
        // Fall back: legacy VoxelOperation format (auto-migrate)
        #[allow(deprecated)]
        if let Ok(legacy_ops) = bincode::deserialize::<Vec<VoxelOperation>>(&bytes) {
            let count = legacy_ops.len();
            for op in legacy_ops {
                self.op_log.push(SignedOperation::from(op));
            }
            return Ok(count);
        }
        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Failed to deserialize operations"))
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
    /// Returns ops sorted in causal replay order (oldest first) so that callers
    /// can apply them deterministically to reach the correct final state.
    ///
    /// # Returns
    /// HashMap mapping chunk ID to operations in that chunk (sorted for replay)
    pub fn get_chunks_with_ops(&self) -> HashMap<ChunkId, Vec<SignedOperation>> {
        let mut chunks: HashMap<ChunkId, Vec<SignedOperation>> = HashMap::new();
        
        for op in &self.op_log {
            if let Some(coord) = op.coord() {
                let chunk_id = ChunkId::from_voxel(&coord);
                chunks.entry(chunk_id).or_insert_with(Vec::new).push(op.clone());
            }
        }
        
        // Sort ops within each chunk in causal replay order
        for ops in chunks.values_mut() {
            ops.sort_by(|a, b| a.replay_cmp(b));
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
    use crate::messages::{Material, Action, SignedOperation};
    
    #[test]
    fn test_user_content_layer() {
        let mut layer = UserContentLayer::new();
        let local_ops = HashMap::new();
        
        // Disable signature verification for test
        layer.config.verify_signatures = false;
        
        let coord = VoxelCoord::new(10, 20, 30);
        let op = SignedOperation::new(
            Action::SetVoxel { coord, material: Material::Stone },
            1,
            crate::vector_clock::VectorClock::new(),
            PeerId::random(),
            [0u8; 32],
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
        let local_op = SignedOperation::new(
            Action::SetVoxel { coord, material: Material::Stone },
            10,
            crate::vector_clock::VectorClock::new(),
            PeerId::random(),
            [0u8; 32],
        );
        let mut local_ops = HashMap::new();
        local_ops.insert(coord, local_op);
        
        // Remote operation with timestamp 5 (older)
        let remote_op = SignedOperation::new(
            Action::SetVoxel { coord, material: Material::Dirt },
            5,
            crate::vector_clock::VectorClock::new(),
            PeerId::random(),
            [0u8; 32],
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
