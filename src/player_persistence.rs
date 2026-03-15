//! Player state persistence (identity-bound)
//!
//! Player state is encrypted and bound to the player's identity key.
//! This makes it:
//! - **Portable:** Load your identity.key anywhere and resume where you left off
//! - **Secure:** State encrypted with identity private key
//! - **Multi-world:** Same identity can have different states in different worlds
//!
//! File location: `world_dir/{peer_id}/player_state.bin` (encrypted binary)

use crate::coordinates::{ECEF, GPS};
use crate::identity::Identity;
use crate::messages::MovementMode;
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Player state that persists across sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPersistence {
    /// Last known position (ECEF coordinates)
    pub position: ECEF,

    /// Last known GPS position (for human-readable debugging)
    pub gps: GPS,

    /// Camera yaw (horizontal rotation) in radians
    pub yaw: f32,

    /// Camera pitch (vertical rotation) in radians
    pub pitch: f32,

    /// Last movement mode
    pub movement_mode: MovementMode,

    /// Last session timestamp (for detecting stale data)
    pub last_updated: u64,
}

impl Default for PlayerPersistence {
    fn default() -> Self {
        // Default spawn: Brisbane, Australia (same as metaworld_alpha default)
        let gps = GPS::new(-27.4705, 153.0260, 50.0);
        let position = gps.to_ecef();

        Self {
            position,
            gps,
            yaw: 0.0,
            pitch: 0.0,
            movement_mode: MovementMode::Fly,
            last_updated: 0,
        }
    }
}

impl PlayerPersistence {
    /// Create from current player state
    pub fn from_state(position: ECEF, yaw: f32, pitch: f32, movement_mode: MovementMode) -> Self {
        Self {
            position,
            gps: position.to_gps(),
            yaw,
            pitch,
            movement_mode,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Save to disk (encrypted, identity-bound)
    pub fn save(&self, world_dir: &Path, identity: &Identity) -> Result<(), String> {
        let path = Self::persistence_path(world_dir, identity);

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create persistence directory: {}", e))?;
        }

        // Serialize to binary (compact)
        let plaintext = bincode::serialize(self)
            .map_err(|e| format!("Failed to serialize player state: {}", e))?;

        // Encrypt with identity-derived key
        let ciphertext = Self::encrypt(&plaintext, identity)
            .map_err(|e| format!("Failed to encrypt player state: {}", e))?;

        // Write to disk
        fs::write(&path, ciphertext).map_err(|e| format!("Failed to write player state: {}", e))?;

        Ok(())
    }

    /// Load from disk (encrypted, identity-bound), or return default if not found
    pub fn load(world_dir: &Path, identity: &Identity) -> Self {
        let path = Self::persistence_path(world_dir, identity);

        // Try to read file
        let ciphertext = match fs::read(&path) {
            Ok(content) => content,
            Err(_) => {
                println!("🆕 No saved player state found, using default spawn");
                return Self::default();
            }
        };

        // Decrypt with identity-derived key
        let plaintext = match Self::decrypt(&ciphertext, identity) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("⚠️  Failed to decrypt player state: {}, using default", e);
                return Self::default();
            }
        };

        // Deserialize
        match bincode::deserialize::<PlayerPersistence>(&plaintext) {
            Ok(state) => {
                println!(
                    "✅ Loaded player state: GPS({:.4}, {:.4}, {:.1}m) - last login {} seconds ago",
                    state.gps.lat,
                    state.gps.lon,
                    state.gps.alt,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        - state.last_updated
                );
                state
            }
            Err(e) => {
                eprintln!("⚠️  Failed to parse player state: {}, using default", e);
                Self::default()
            }
        }
    }

    /// Get path to persistence file (identity-bound)
    fn persistence_path(world_dir: &Path, identity: &Identity) -> PathBuf {
        let peer_id = identity.peer_id().to_string();
        world_dir.join(&peer_id).join("player_state.bin")
    }

    /// Returns `true` if a local persistence file exists for this identity.
    ///
    /// Use this before calling `load()` to decide whether to request a DHT
    /// session record as a fallback (new machine / first run).
    pub fn has_local_save(world_dir: &Path, identity: &Identity) -> bool {
        Self::persistence_path(world_dir, identity).exists()
    }

    /// Encrypt data with identity-derived key
    fn encrypt(plaintext: &[u8], identity: &Identity) -> Result<Vec<u8>, String> {
        // Derive encryption key from identity (hash of signing key)
        let key_material = identity.signing_key_bytes();
        let mut hasher = Sha256::new();
        hasher.update(b"metaverse-player-state-encryption-v1");
        hasher.update(key_material);
        let key_bytes = hasher.finalize();

        // Create cipher
        let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes[..32])
            .map_err(|e| format!("Failed to create cipher: {}", e))?;

        // Random nonce (96-bit for ChaCha20Poly1305)
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("Encryption failed: {}", e))?;

        // Prepend nonce to ciphertext (nonce is public, doesn't need to be secret)
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt data with identity-derived key
    fn decrypt(data: &[u8], identity: &Identity) -> Result<Vec<u8>, String> {
        if data.len() < 12 {
            return Err("Data too short to contain nonce".to_string());
        }

        // Extract nonce (first 12 bytes)
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        // Derive encryption key from identity (same as encrypt)
        let key_material = identity.signing_key_bytes();
        let mut hasher = Sha256::new();
        hasher.update(b"metaverse-player-state-encryption-v1");
        hasher.update(key_material);
        let key_bytes = hasher.finalize();

        // Create cipher
        let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes[..32])
            .map_err(|e| format!("Failed to create cipher: {}", e))?;

        // Decrypt
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {}", e))
    }

    /// Check if saved state is stale (older than N days)
    pub fn is_stale(&self, max_age_days: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let age_seconds = now.saturating_sub(self.last_updated);
        let age_days = age_seconds / (24 * 60 * 60);

        age_days > max_age_days
    }

    /// Update position and save to disk
    pub fn update_and_save(
        &mut self,
        position: ECEF,
        yaw: f32,
        pitch: f32,
        movement_mode: MovementMode,
        world_dir: &Path,
        identity: &Identity,
    ) -> Result<(), String> {
        self.position = position;
        self.gps = position.to_gps();
        self.yaw = yaw;
        self.pitch = pitch;
        self.movement_mode = movement_mode;
        self.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.save(world_dir, identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load() {
        let temp_dir = tempdir().unwrap();
        let world_dir = temp_dir.path();
        let identity = crate::identity::Identity::generate();

        // Create state
        let position = ECEF::new(1000.0, 2000.0, 6371000.0);
        let state = PlayerPersistence::from_state(position, 1.5, -0.5, MovementMode::Walk);

        // Save
        state.save(world_dir, &identity).unwrap();

        // Load
        let loaded = PlayerPersistence::load(world_dir, &identity);

        // Verify
        assert_eq!(loaded.position.x, position.x);
        assert_eq!(loaded.position.y, position.y);
        assert_eq!(loaded.position.z, position.z);
        assert_eq!(loaded.yaw, 1.5);
        assert_eq!(loaded.pitch, -0.5);
    }

    #[test]
    fn test_default_spawn() {
        let state = PlayerPersistence::default();

        // Should spawn at Brisbane, Australia (default spawn point)
        assert!((state.gps.lat - (-27.4705)).abs() < 0.01);
        assert!((state.gps.lon - 153.0260).abs() < 0.01);
        assert!((state.gps.alt - 50.0).abs() < 1.0);
    }
}
