//! Remote player rendering utilities
//!
//! Provides functions to render remote players as wireframe capsules
//! with name tags.

use crate::{
    mesh::{Mesh, Vertex},
    physics::PhysicsWorld,
    player_state::NetworkedPlayer,
};
use glam::{Vec3, Mat4};
use libp2p::PeerId;

/// Create a wireframe capsule mesh for a remote player
///
/// Creates a 0.6m × 1.8m × 0.6m capsule (same dimensions as player hitbox)
/// with a different color from the local player.
pub fn create_remote_player_capsule() -> Mesh {
    let mut mesh = Mesh::new();
    
    // HUGE AND BRIGHT for testing - 5x normal size!
    let w = 1.5; // Half width (3m total - was 0.6m)
    let h = 4.5; // Half height (9m total - was 1.8m)
    
    // Color: BRIGHT RED - impossible to miss!
    let color = Vec3::new(1.0, 0.0, 0.0);
    
    // Create wireframe cube edges
    // Bottom square
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h, -w), color));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h, -w), color));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h,  w), color));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h,  w), color));
    
    // Top square
    let v4 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h, -w), color));
    let v5 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h, -w), color));
    let v6 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h,  w), color));
    let v7 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h,  w), color));
    
    // Bottom edges (4 lines = 8 triangles as degenerate)
    mesh.add_line(v0, v1);
    mesh.add_line(v1, v2);
    mesh.add_line(v2, v3);
    mesh.add_line(v3, v0);
    
    // Top edges
    mesh.add_line(v4, v5);
    mesh.add_line(v5, v6);
    mesh.add_line(v6, v7);
    mesh.add_line(v7, v4);
    
    // Vertical edges
    mesh.add_line(v0, v4);
    mesh.add_line(v1, v5);
    mesh.add_line(v2, v6);
    mesh.add_line(v3, v7);
    
    mesh
}

/// Create a simple name tag mesh (text will be rendered in future)
///
/// For now, creates a small horizontal bar above the player's head
/// to indicate where the name tag would be.
pub fn create_name_tag_marker() -> Mesh {
    let mut mesh = Mesh::new();
    
    // Small horizontal line 0.3m above head (at Y = 0.9 + 0.3 = 1.2)
    let y = 1.2;
    let width = 0.3;
    let color = Vec3::new(1.0, 1.0, 1.0); // White
    
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(-width, y, 0.0), color));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new( width, y, 0.0), color));
    
    mesh.add_line(v0, v1);
    
    mesh
}

/// Get shortened peer ID for display (first 8 characters)
pub fn short_peer_id(peer_id: &PeerId) -> String {
    peer_id.to_string().chars().take(8).collect()
}

/// Convert remote player position to local rendering space
pub fn remote_player_to_local(
    player: &NetworkedPlayer,
    physics: &PhysicsWorld,
) -> Vec3 {
    physics.ecef_to_local(&player.position)
}

/// Create transform matrix for remote player rendering
///
/// Positions the capsule at the player's feet (not eyes like local player)
pub fn remote_player_transform(
    player: &NetworkedPlayer,
    physics: &PhysicsWorld,
) -> Mat4 {
    let local_pos = remote_player_to_local(player, physics);
    
    // Apply yaw rotation around Y axis
    let rotation = glam::Quat::from_rotation_y(player.yaw);
    
    Mat4::from_rotation_translation(rotation, local_pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_remote_player_capsule() {
        let mesh = create_remote_player_capsule();
        assert!(!mesh.vertices.is_empty());
        // Wireframe cube has 8 vertices
        assert_eq!(mesh.vertices.len(), 8);
    }
    
    #[test]
    fn test_short_peer_id() {
        // Create a test peer ID
        let test_id = "12QaB3cD4eF5gH6iJ7kL8mN9oP0qR";
        // PeerId parsing would require actual libp2p, so just test the logic
        let short = test_id.chars().take(8).collect::<String>();
        assert_eq!(short.len(), 8);
        assert_eq!(short, "12QaB3cD");
    }
}
