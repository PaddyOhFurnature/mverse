//! Minimal P2P Relay Server for Metaverse
//!
//! **Purpose:** Bootstrap node for NAT traversal coordination
//!
//! **NOT a game server** - This is purely for:
//! - Helping peers discover each other across the internet
//! - Coordinating hole-punching (DCUtR)
//! - Fallback relay when direct P2P fails
//!
//! Once direct P2P is established, this relay is no longer used.
//!
//! # Usage
//!
//! ```bash
//! # Basic usage (random port)
//! cargo run --release --bin metaverse-relay
//!
//! # Specify port
//! cargo run --release --bin metaverse-relay -- --port 4001
//!
//! # Production mode
//! cargo run --release --bin metaverse-relay -- --port 4001 --external-addr /ip4/YOUR_PUBLIC_IP/tcp/4001
//! ```
//!
//! # Deployment
//!
//! This can run on:
//! - Free VPS (Oracle Cloud free tier, AWS free tier)
//! - Home server / NAS
//! - Docker container
//!
//! Requirements: 512MB RAM, 1 CPU core, public IP

use libp2p::{
    identity,
    relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    SwarmBuilder,
    PeerId,
};
use futures::StreamExt;
use std::error::Error;
use std::time::Duration;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "metaverse-relay")]
#[command(about = "P2P relay server for metaverse NAT traversal", long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "4001")]
    port: u16,

    /// External address to advertise (for NAT/firewall)
    #[arg(long)]
    external_addr: Option<String>,

    /// Maximum number of relay circuits
    #[arg(long, default_value = "100")]
    max_circuits: usize,

    /// Maximum circuit duration in seconds
    #[arg(long, default_value = "120")]
    max_circuit_duration: u64,

    /// Maximum bytes per circuit (1MB default)
    #[arg(long, default_value = "1048576")]
    max_circuit_bytes: u64,
}

/// Relay server behavior
#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    ping: libp2p::ping::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let args = Args::parse();

    println!("🌐 Metaverse P2P Relay Server");
    println!("================================");
    println!("Port: {}", args.port);
    println!("Max circuits: {}", args.max_circuits);
    println!("Circuit duration: {}s", args.max_circuit_duration);
    println!("Circuit data limit: {}MB", args.max_circuit_bytes / 1024 / 1024);
    println!();

    // Generate or load identity
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("🔑 Peer ID: {}", local_peer_id);

    // Configure relay behavior
    let relay_config = relay::Config {
        max_reservations: args.max_circuits,
        max_circuits: args.max_circuits,
        max_circuit_duration: Duration::from_secs(args.max_circuit_duration),
        max_circuit_bytes: args.max_circuit_bytes,
        ..Default::default()
    };

    // Build swarm with relay server
    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_behaviour(|key: &identity::Keypair| {
            Ok(RelayBehaviour {
                relay: relay::Behaviour::new(key.public().to_peer_id(), relay_config),
                ping: libp2p::ping::Behaviour::new(libp2p::ping::Config::new()),
            })
        })?
        .with_swarm_config(|c: libp2p::swarm::Config| {
            c.with_idle_connection_timeout(Duration::from_secs(300))
        })
        .build();

    // Listen on all interfaces
    let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", args.port);
    swarm.listen_on(listen_addr.parse()?)?;

    // If external address provided, add it
    if let Some(external) = args.external_addr {
        swarm.add_external_address(external.parse()?);
        println!("📍 External address: {}", external);
    }

    println!("\n✅ Relay server started");
    println!("👂 Listening for connections...\n");

    // Connection statistics
    let mut active_connections = 0;
    let mut total_connections = 0;
    let mut active_circuits = 0;

    // Event loop
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("👂 Listening on: {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                active_connections += 1;
                total_connections += 1;
                println!("🔗 Connection established: {} (active: {}, total: {})", 
                    peer_id, active_connections, total_connections);
            }
            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                active_connections -= 1;
                println!("❌ Connection closed: {} - {:?} (active: {})", 
                    peer_id, cause, active_connections);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(event)) => {
                match event {
                    relay::Event::ReservationReqAccepted { src_peer_id, .. } => {
                        println!("✅ Reservation accepted: {}", src_peer_id);
                    }
                    relay::Event::ReservationTimedOut { src_peer_id } => {
                        println!("⏱️  Reservation timed out: {}", src_peer_id);
                    }
                    relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id } => {
                        active_circuits += 1;
                        println!("🔄 Circuit established: {} → {} (active circuits: {})", 
                            src_peer_id, dst_peer_id, active_circuits);
                    }
                    relay::Event::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                        active_circuits -= 1;
                        println!("🔚 Circuit closed: {} → {} (active circuits: {})", 
                            src_peer_id, dst_peer_id, active_circuits);
                    }
                    _ => {}
                }
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Ping(_)) => {
                // Ping events are verbose, ignore them
            }
            _ => {}
        }
    }
}
