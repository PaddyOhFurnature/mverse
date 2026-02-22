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
    tls,
    kad, identify,
    swarm::{NetworkBehaviour, SwarmEvent},
    SwarmBuilder,
    PeerId,
    Multiaddr,
};
use libp2p::kad::store::MemoryStore;
use futures::StreamExt;
use std::error::Error;
use std::time::Duration;
use clap::Parser;
use reqwest;

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
    #[arg(long, default_value = "3600")]
    max_circuit_duration: u64,

    /// Maximum bytes per circuit (1GB default — chunk terrain sync can be several MB per session)
    #[arg(long, default_value = "1073741824")]
    max_circuit_bytes: u64,

    /// Other relay nodes to peer with — dial these at startup so relays form a DHT mesh.
    /// Can be specified multiple times: --peer /ip4/x.x.x.x/tcp/4001/p2p/12D3...
    #[arg(long)]
    peer: Vec<String>,
}

/// Relay server behavior
#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    ping: libp2p::ping::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour,
}

/// Try several public IP detection services in order, return first success.
async fn detect_public_ip() -> Option<String> {
    let services = [
        "https://api.ipify.org",
        "https://ipv4.icanhazip.com",
        "https://checkip.amazonaws.com",
    ];
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    for url in &services {
        if let Ok(resp) = client.get(*url).send().await {
            if let Ok(text) = resp.text().await {
                let ip = text.trim().to_string();
                // Basic validation: looks like an IPv4 address
                if ip.split('.').count() == 4 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return Some(ip);
                }
            }
        }
    }
    None
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

    // Load or generate persistent identity (saves to ~/.metaverse/relay.key)
    let key_path = {
        let base = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        base.join(".metaverse").join("relay.key")
    };
    let local_key = if key_path.exists() {
        let bytes = std::fs::read(&key_path)?;
        identity::Keypair::from_protobuf_encoding(&bytes)?
    } else {
        let kp = identity::Keypair::generate_ed25519();
        if let Some(parent) = key_path.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(&key_path, kp.to_protobuf_encoding()?)?;
        kp
    };
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

    // Build swarm with relay server - TCP + WebSocket for universal connectivity
    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_dns()?
        .with_websocket(
            (libp2p::tls::Config::new, libp2p::noise::Config::new),
            libp2p::yamux::Config::default,
        )
        .await?
        .with_behaviour(|key: &identity::Keypair| {
            let peer_id = key.public().to_peer_id();
            // Kademlia DHT - serves as bootstrap node for peer discovery
            let mut kad_config = kad::Config::default();
            kad_config.set_query_timeout(Duration::from_secs(60));
            let mut kademlia = kad::Behaviour::with_config(peer_id, MemoryStore::new(peer_id), kad_config);
            kademlia.set_mode(Some(kad::Mode::Server)); // relay is always a DHT server
            // Identify - lets clients know our addresses and protocols
            let identify = identify::Behaviour::new(
                identify::Config::new("/metaverse/1.0.0".to_string(), key.public())
                    .with_push_listen_addr_updates(true)
            );
            Ok(RelayBehaviour {
                relay: relay::Behaviour::new(peer_id, relay_config),
                ping: libp2p::ping::Behaviour::new(libp2p::ping::Config::new()),
                kademlia,
                identify,
            })
        })?
        .with_swarm_config(|c: libp2p::swarm::Config| {
            // 60s idle timeout — prevents stale connections from accumulating when
            // clients are reconnecting frequently. Reservations renew before expiry.
            c.with_idle_connection_timeout(Duration::from_secs(60))
        })
        .build();

    // Listen TCP
    let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", args.port);
    swarm.listen_on(listen_addr.parse()?)?;

    // Listen WebSocket on port 9001 (or tcp_port+5000 for WS)
    let ws_port = args.port + 5000;
    let ws_addr = format!("/ip4/0.0.0.0/tcp/{}/ws", ws_port);
    swarm.listen_on(ws_addr.parse()?)?;
    println!("🌐 WebSocket port: {}", ws_port);

    // Resolve external address — use provided value or auto-detect via HTTP
    let external_ip = match args.external_addr {
        Some(ref addr) => {
            println!("📍 External address: {} (provided)", addr);
            Some(addr.clone())
        }
        None => {
            print!("🌐 Auto-detecting public IP... ");
            let ip = detect_public_ip().await;
            match &ip {
                Some(detected) => println!("detected: {}", detected),
                None => println!("failed — run with --external-addr /ip4/YOUR_IP/tcp/{}", args.port),
            }
            ip.map(|ip| format!("/ip4/{}/tcp/{}", ip, args.port))
        }
    };
    if let Some(ref external) = external_ip {
        if let Ok(addr) = external.parse() {
            swarm.add_external_address(addr);
        }
        // Also advertise WebSocket external address
        let ws_external = external.replace(&format!("/tcp/{}", args.port), &format!("/tcp/{}/ws", ws_port));
        if let Ok(addr) = ws_external.parse() {
            swarm.add_external_address(addr);
        }
    }

    println!("\n✅ Relay server started");
    println!("👂 Listening for connections...\n");

    // Dial peer relays so they share DHT state from the start.
    // Without this, two isolated relays can't help clients find each other.
    if !args.peer.is_empty() {
        println!("🔗 Dialing {} peer relay(s)...", args.peer.len());
        for peer_addr in &args.peer {
            match peer_addr.parse::<Multiaddr>() {
                Ok(addr) => {
                    // Add to DHT routing table
                    if let Some(libp2p::multiaddr::Protocol::P2p(peer_id)) = addr.iter().last() {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                    }
                    match swarm.dial(addr.clone()) {
                        Ok(()) => println!("   Dialing peer relay: {}", peer_addr),
                        Err(e) => eprintln!("   Failed to dial {}: {}", peer_addr, e),
                    }
                }
                Err(e) => eprintln!("   Invalid peer address {}: {}", peer_addr, e),
            }
        }
        swarm.behaviour_mut().kademlia.bootstrap().ok();
    }

    // Connection statistics
    let mut active_connections: i64 = 0;
    let mut total_connections: u64 = 0;
    let mut active_circuits: i64 = 0;

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
            SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info, .. }
            )) => {
                // Add peer's addresses to our DHT so other clients can find them
                for addr in &info.listen_addrs {
                    swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                }
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Kademlia(_)) => {
                // DHT events - routing updates etc, no action needed
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Ping(_)) => {
                // Ping events are verbose, ignore them
            }
            _ => {}
        }
    }
}
