//! Test the identity system
//!
//! Demonstrates:
//! - Generating an Ed25519 keypair
//! - Deriving a PeerId
//! - Signing and verifying data
//! - Persisting and loading identity

use metaverse_core::identity::Identity;

fn main() {
    println!("=== Identity System Test ===\n");
    
    // Generate a new identity
    println!("1. Generating new identity...");
    let identity = Identity::generate();
    
    println!("   PeerId: {}", identity.peer_id());
    println!("   Verifying Key: {:02x?}...", &identity.verifying_key().to_bytes()[..8]);
    println!();
    
    // Sign some data
    println!("2. Signing test data...");
    let data = b"Voxel at coordinates (1000, 500, 2000) changed to STONE";
    let signature = identity.sign(data);
    
    println!("   Data: {}", String::from_utf8_lossy(data));
    println!("   Signature: {:02x?}...", &signature.to_bytes()[..16]);
    println!();
    
    // Verify signature
    println!("3. Verifying signature...");
    let valid = identity.verify_own(data, &signature);
    println!("   Valid: {}", valid);
    assert!(valid, "Signature should be valid!");
    
    // Try with wrong data
    let wrong_data = b"Different data";
    let invalid = identity.verify_own(wrong_data, &signature);
    println!("   Valid (wrong data): {}", invalid);
    assert!(!invalid, "Signature should be invalid for different data!");
    println!();
    
    // Test with explicit verifying key (simulating remote verification)
    println!("4. Simulating remote verification...");
    let remote_valid = Identity::verify_with_pubkey(
        identity.verifying_key(),
        data,
        &signature
    );
    println!("   Remote verification: {}", remote_valid);
    assert!(remote_valid, "Remote verification should succeed!");
    println!();
    
    // Test persistence
    println!("5. Testing persistence...");
    let temp_path = std::env::temp_dir().join("test_identity.key");
    
    identity.save_to_path(&temp_path)
        .expect("Failed to save identity");
    println!("   Saved to: {}", temp_path.display());
    
    let loaded_identity = Identity::load_from_path(&temp_path)
        .expect("Failed to load identity");
    println!("   Loaded from: {}", temp_path.display());
    
    assert_eq!(
        identity.peer_id(),
        loaded_identity.peer_id(),
        "PeerId should match after loading"
    );
    
    // Verify signature with loaded identity
    let loaded_valid = loaded_identity.verify_own(data, &signature);
    println!("   Loaded identity verifies signature: {}", loaded_valid);
    assert!(loaded_valid, "Loaded identity should verify original signature!");
    
    // Clean up
    std::fs::remove_file(&temp_path).ok();
    println!();
    
    // Test libp2p keypair conversion
    println!("6. Testing libp2p Keypair conversion...");
    let libp2p_keypair = identity.to_libp2p_keypair();
    let libp2p_peer_id = libp2p::PeerId::from(libp2p_keypair.public());
    println!("   libp2p PeerId: {}", libp2p_peer_id);
    assert_eq!(
        identity.peer_id(),
        &libp2p_peer_id,
        "libp2p PeerId should match derived PeerId"
    );
    println!();
    
    println!("✅ All identity tests passed!");
}
