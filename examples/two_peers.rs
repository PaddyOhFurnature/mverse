/// Two-peer P2P connection test
/// 
/// Demonstrates:
/// - Two libp2p nodes connecting on localhost
/// - Gossipsub message exchange
/// - Peer discovery and connection lifecycle
/// 
/// Usage:
///   Terminal 1: cargo run --example two_peers -- peer1
///   Terminal 2: cargo run --example two_peers -- peer2

use metaverse_core::identity::Identity;
use metaverse_core::network::{NetworkNode, NetworkEvent};
use std::time::Duration;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let peer_name = args.get(1).map(|s| s.as_str()).unwrap_or("peer1");
    
    println!("=== {} Starting ===", peer_name);
    
    // Create or load identity (different for each peer)
    let identity = if peer_name == "peer1" {
        let path = std::path::PathBuf::from(shellexpand::tilde("~/.metaverse/peer1.key").to_string());
        if path.exists() {
            Identity::load_from_path(&path)?
        } else {
            let new_id = Identity::generate();
            std::fs::create_dir_all(path.parent().unwrap())?;
            new_id.save_to_path(&path)?;
            new_id
        }
    } else {
        let path = std::path::PathBuf::from(shellexpand::tilde("~/.metaverse/peer2.key").to_string());
        if path.exists() {
            Identity::load_from_path(&path)?
        } else {
            let new_id = Identity::generate();
            std::fs::create_dir_all(path.parent().unwrap())?;
            new_id.save_to_path(&path)?;
            new_id
        }
    };
    println!("{}: PeerId: {}", peer_name, identity.peer_id());
    
    // Create network node
    let mut node = NetworkNode::new(identity)?;
    
    // Subscribe to test topic
    let topic = "test-topic";
    node.subscribe(topic)?;
    println!("{}: Subscribed to '{}'", peer_name, topic);
    
    // Start listening
    let listen_addr = if peer_name == "peer1" {
        "/ip4/127.0.0.1/tcp/9000"
    } else {
        "/ip4/127.0.0.1/tcp/9001"
    };
    
    node.listen_on(listen_addr)?;
    println!("{}: Listening on {}", peer_name, listen_addr);
    
    // If peer2, connect to peer1
    if peer_name == "peer2" {
        tokio::time::sleep(Duration::from_millis(500)).await; // Give peer1 time to start
        println!("{}: Attempting to dial peer1...", peer_name);
        match node.dial("/ip4/127.0.0.1/tcp/9000") {
            Ok(_) => println!("{}: Dial initiated to peer1", peer_name),
            Err(e) => println!("{}: Dial failed: {}", peer_name, e),
        }
    }
    
    // Message counter
    let mut msg_count = 0;
    let mut last_publish = tokio::time::Instant::now();
    
    println!("{}: Entering event loop (Ctrl+C to exit)", peer_name);
    println!("----------------------------------------");
    
    loop {
        // Process network events
        while let Some(event) = node.poll() {
            match event {
                NetworkEvent::Message { data, peer_id, topic: _ } => {
                    let msg = String::from_utf8_lossy(&data);
                    println!("{}: 📨 Received from {}: '{}'", peer_name, peer_id, msg);
                }
                NetworkEvent::PeerConnected { peer_id, address } => {
                    println!("{}: ✅ Connected to peer: {} at {}", peer_name, peer_id, address);
                }
                NetworkEvent::PeerDisconnected { peer_id } => {
                    println!("{}: ❌ Disconnected from peer: {}", peer_name, peer_id);
                }
                NetworkEvent::PeerDiscovered { peer_id } => {
                    println!("{}: 🔍 Discovered peer: {}", peer_name, peer_id);
                }
                NetworkEvent::ListeningOn { address } => {
                    println!("{}: 🎧 Listening on: {}", peer_name, address);
                }
                NetworkEvent::TopicSubscribed { topic } => {
                    println!("{}: 📢 Subscribed to topic: '{}'", peer_name, topic);
                }
                NetworkEvent::TopicUnsubscribed { topic } => {
                    println!("{}: 📢 Unsubscribed from topic: '{}'", peer_name, topic);
                }
            }
        }
        
        // Publish a message every 3 seconds
        if last_publish.elapsed() > Duration::from_secs(3) {
            msg_count += 1;
            let message = format!("{}: Hello #{}", peer_name, msg_count);
            
            match node.publish(topic, message.as_bytes().to_vec()) {
                Ok(_) => println!("{}: 📤 Published: '{}'", peer_name, message),
                Err(e) => eprintln!("{}: Failed to publish: {}", peer_name, e),
            }
            
            last_publish = tokio::time::Instant::now();
        }
        
        // Sleep to avoid busy-waiting
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
