//! Player state persistence
//!
//! Saves and loads player position, rotation, and settings to/from disk.
//! Allows players to resume where they left off.

use crate::coordinates::{ECEF, GPS};
use crate::messages::MovementMode;
use serde::{Deserialize, Serialize};
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
        // Default spawn: Mount Everest summit
        let gps = GPS::new(27.9881, 86.9250, 8848.86);
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
    pub fn from_state(
        position: ECEF,
        yaw: f32,
        pitch: f32,
        movement_mode: MovementMode,
    ) -> Self {
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
    
    /// Save to disk
    pub fn save(&self, world_dir: &Path) -> Result<(), String> {
        let path = Self::persistence_path(world_dir);
        
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create persistence directory: {}", e))?;
        }
        
        // Serialize to JSON (human-readable for debugging)
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize player state: {}", e))?;
        
        // Write to disk
        fs::write(&path, json)
            .map_err(|e| format!("Failed to write player state: {}", e))?;
        
        Ok(())
    }
    
    /// Load from disk, or return default if not found
    pub fn load(world_dir: &Path) -> Self {
        let path = Self::persistence_path(world_dir);
        
        // Try to read file
        let json = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => {
                println!("No saved player position found, using default spawn");
                return Self::default();
            }
        };
        
        // Try to deserialize
        match serde_json::from_str::<PlayerPersistence>(&json) {
            Ok(state) => {
                println!("✅ Loaded player position: GPS({:.4}, {:.4}, {:.1}m)", 
                    state.gps.lat, state.gps.lon, state.gps.alt);
                state
            }
            Err(e) => {
                eprintln!("⚠️ Failed to parse player state: {}, using default", e);
                Self::default()
            }
        }
    }
    
    /// Get path to persistence file
    fn persistence_path(world_dir: &Path) -> PathBuf {
        world_dir.join("player_state.json")
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
        
        self.save(world_dir)
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
        
        // Create state
        let position = ECEF::new(1000.0, 2000.0, 6371000.0);
        let state = PlayerPersistence::from_state(
            position,
            1.5,
            -0.5,
            MovementMode::Walk,
        );
        
        // Save
        state.save(world_dir).unwrap();
        
        // Load
        let loaded = PlayerPersistence::load(world_dir);
        
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
        
        // Should spawn at Mount Everest
        assert!((state.gps.lat - 27.9881).abs() < 0.01);
        assert!((state.gps.lon - 86.9250).abs() < 0.01);
        assert!((state.gps.alt - 8848.86).abs() < 1.0);
    }
}
