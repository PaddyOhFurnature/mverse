//! P2P networking layer for the metaverse
//!
//! This module implements the libp2p-based networking stack with:
//! - Kademlia DHT for peer discovery
//! - Gossipsub for publish/subscribe messaging
//! - mDNS for local network discovery
//! - Bandwidth-aware message routing
//!
//! # Architecture
//!
//! The NetworkNode is the central hub for all P2P communication. It manages:
//! - Swarm: libp2p's network abstraction (transports, protocols, connections)
//! - Topics: Gossipsub channels for different message types
//! - Events: Incoming messages and connection status changes
//!
//! # Usage
//!
//! ```no_run
//! use metaverse_core::identity::Identity;
//! use metaverse_core::network::NetworkNode;
//!
//! // Create identity and network node
//! let identity = Identity::load_or_create()?;
//! let mut network = NetworkNode::new(identity)?;
//!
//! // Start listening
//! network.listen_on("/ip4/0.0.0.0/tcp/0")?;
//!
//! // Poll for events
//! loop {
//!     if let Some(event) = network.poll() {
//!         match event {
//!             NetworkEvent::Message { peer_id, topic, data } => {
//!                 println!("Received from {}: {:?}", peer_id, data);
//!             }
//!             // ... handle other events
//!         }
//!     }
//! }
//! ```

use crate::identity::Identity;
use libp2p::{
    autonat,
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify,
    kad::{self, store::MemoryStore},
    mdns,
    noise,
    relay,
    dcutr,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, quic, websocket, tls, yamux, Multiaddr, PeerId, Swarm, Transport,
};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use futures::StreamExt;

/// Result type for network operations
pub type Result<T> = std::result::Result<T, NetworkError>;

/// Commands sent from main thread to background network thread
#[derive(Debug, Clone)]
pub enum NetworkCommand {
    /// Start listening on an address
    Listen {
        multiaddr: String,
    },
    
    /// Dial a peer
    Dial {
        address: String,
    },
    
    /// Subscribe to a topic
    Subscribe {
        topic: String,
    },
    
    /// Unsubscribe from a topic
    Unsubscribe {
        topic: String,
    },
    
    /// Subscribe to multiple topics at once (efficient bulk operation)
    SubscribeBulk {
        topics: Vec<String>,
    },
    
    /// Unsubscribe from multiple topics at once (efficient bulk operation)
    UnsubscribeBulk {
        topics: Vec<String>,
    },
    
    /// Publish a message to a topic
    Publish {
        topic: String,
        data: Vec<u8>,
    },
    
    /// Shutdown the network thread
    Shutdown,
}

/// Errors that can occur during network operations
#[derive(Debug)]
pub enum NetworkError {
    /// Failed to initialize libp2p transport
    TransportError(String),
    
    /// Failed to build Swarm
    SwarmBuildError(String),
    
    /// Failed to start listening on address
    ListenError(std::io::Error),
    
    /// Failed to parse multiaddr
    MultiaddrParseError(String),
    
    /// Topic not subscribed
    TopicNotSubscribed(String),
    
    /// Failed to publish message
    PublishError(String),
    
    /// Tokio runtime error
    RuntimeError(String),
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TransportError(e) => write!(f, "Transport error: {}", e),
            Self::SwarmBuildError(e) => write!(f, "Swarm build error: {}", e),
            Self::ListenError(e) => write!(f, "Listen error: {}", e),
            Self::MultiaddrParseError(e) => write!(f, "Multiaddr parse error: {}", e),
            Self::TopicNotSubscribed(t) => write!(f, "Not subscribed to topic: {}", t),
            Self::PublishError(e) => write!(f, "Publish error: {}", e),
            Self::RuntimeError(e) => write!(f, "Runtime error: {}", e),
        }
    }
}

impl std::error::Error for NetworkError {}

/// Events emitted by the NetworkNode
#[derive(Debug)]
pub enum NetworkEvent {
    /// Received a message on a subscribed topic
    Message {
        peer_id: PeerId,
        topic: String,
        data: Vec<u8>,
    },
    
    /// Connected to a new peer
    PeerConnected {
        peer_id: PeerId,
        address: Multiaddr,
    },
    
    /// Disconnected from a peer
    PeerDisconnected {
        peer_id: PeerId,
    },
    
    /// Discovered a new peer via mDNS
    PeerDiscovered {
        peer_id: PeerId,
    },
    
    /// Started listening on an address
    ListeningOn {
        address: Multiaddr,
    },
    
    /// Subscribed to a new topic
    TopicSubscribed {
        topic: String,
    },
    
    /// Unsubscribed from a topic
    TopicUnsubscribed {
        topic: String,
    },
    
    /// NAT status changed (detected via AutoNAT)
    NatStatusChanged {
        old_status: String,
        new_status: String,
        external_address: Option<Multiaddr>,
    },
    
    /// Connection upgraded from relay to direct (DCUtR success)
    ConnectionUpgraded {
        peer_id: PeerId,
        from_relay: bool,
    },
}

/// Combined network behaviour for libp2p
///
/// **Mesh P2P Architecture:**
/// - Kademlia: DHT for peer discovery
/// - Gossipsub: Pubsub for state sync (primary communication)
/// - mDNS: Local network auto-discovery
/// - Identify: Peer information exchange
/// - Relay Client: Can USE other peers as relays for NAT traversal
/// - Relay Server: Can BE a relay to help other peers connect (mesh topology)
/// - DCUtR: Direct Connection Upgrade (hole punching)
///
/// Every peer is simultaneously:
/// - A client (can use relays)
/// - A server (can be a relay)
/// - A DHT node (helps with discovery)
/// - A content node (shares data via gossipsub)
///
/// This creates a true mesh network where dedicated relays are just
/// "always-on peers" rather than special infrastructure.
#[derive(NetworkBehaviour)]
pub(crate) struct MetaverseBehaviour {
    pub(crate) kademlia: kad::Behaviour<MemoryStore>,
    pub(crate) gossipsub: gossipsub::Behaviour,
    pub(crate) mdns: mdns::tokio::Behaviour,
    pub(crate) identify: identify::Behaviour,
    pub(crate) relay_client: relay::client::Behaviour,
    pub(crate) relay_server: relay::Behaviour,
    pub(crate) autonat: autonat::Behaviour,
}

/// P2P networking node
///
/// Manages all network communication for the metaverse client.
/// Uses libp2p for transport, discovery, and messaging.
pub struct NetworkNode {
    /// libp2p Swarm managing connections and protocols
    pub(crate) swarm: Swarm<MetaverseBehaviour>,
    
    /// Local peer identity
    identity: Identity,
    
    /// Local peer ID
    local_peer_id: PeerId,
    
    /// Currently subscribed topics
    subscribed_topics: HashMap<String, IdentTopic>,
    
    /// Event queue (for non-async polling)
    event_queue: Vec<NetworkEvent>,
    
    /// Connected peers
    connected_peers: HashSet<PeerId>,

    /// Peers we know are relay nodes - listen on circuit when connected
    relay_nodes: HashSet<PeerId>,

    /// Known base address for each relay node (for circuit re-registration)
    relay_addrs: HashMap<PeerId, Multiaddr>,

    /// Known game peers and their last seen address — used to redial after reconnect
    /// since RoutingUpdated only fires for NEW DHT entries, not already-known peers
    known_game_peers: HashMap<PeerId, Multiaddr>,
}

/// Returns true if this address is useful to advertise in DHT.
/// Filters out loopback, link-local, and virtual bridge addresses
/// that are unreachable from other machines.
fn is_routable_addr(addr: &Multiaddr) -> bool {
    use libp2p::multiaddr::Protocol;
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => {
                if ip.is_loopback()           { return false; } // 127.x
                if ip.is_link_local()         { return false; } // 169.254.x
                // Filter common virtual bridge ranges (libvirt, docker, vmware)
                let octets = ip.octets();
                if octets[0] == 192 && octets[1] == 168 && octets[2] == 122 { return false; } // libvirt
                if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31   { return false; } // docker
                if octets[0] == 10  && octets[1] == 0   && octets[2] == 2   { return false; } // VirtualBox NAT
            }
            Protocol::Ip6(ip) => {
                if ip.is_loopback()    { return false; }
            }
            _ => {}
        }
    }
    true
}

impl NetworkNode {
    /// Create a new NetworkNode with the given identity
    ///
    /// **WARNING:** This uses pollster::block_on() which does NOT provide
    /// a tokio runtime context. mDNS will FAIL when called this way.
    ///
    /// **Use new_async() instead when running in a tokio context.**
    ///
    /// Initializes libp2p Swarm with:
    /// - TCP transport with Noise encryption and Yamux multiplexing
    /// - Kademlia DHT in client mode
    /// - Gossipsub for pub/sub messaging
    /// - mDNS for local network discovery
    /// - Identify protocol for peer info exchange
    pub fn new(identity: Identity) -> Result<Self> {
        let local_peer_id = identity.peer_id().clone();
        
        // Build the Swarm (now synchronous since we use pollster)
        // WARNING: This will fail for mDNS if not in a tokio context
        let swarm = pollster::block_on(Self::build_swarm(identity.clone()))?;
        
        Ok(Self {
            swarm,
            identity,
            local_peer_id,
            subscribed_topics: HashMap::new(),
            event_queue: Vec::new(),
            connected_peers: HashSet::new(),
            relay_nodes: HashSet::new(),
            relay_addrs: HashMap::new(),
            known_game_peers: HashMap::new(),
        })
    }
    
    /// Create a new NetworkNode with the given identity (async version)
    ///
    /// **This is the preferred method when using with MultiplayerSystem.**
    ///
    /// Must be called from within a tokio runtime context because
    /// mDNS requires access to the tokio reactor.
    ///
    /// Initializes libp2p Swarm with:
    /// - TCP transport with Noise encryption and Yamux multiplexing
    /// - Kademlia DHT in client mode
    /// - Gossipsub for pub/sub messaging
    /// - mDNS for local network discovery
    /// - Identify protocol for peer info exchange
    pub async fn new_async(identity: Identity) -> Result<Self> {
        let local_peer_id = identity.peer_id().clone();
        
        // Build the Swarm asynchronously (mDNS needs tokio context)
        let swarm = Self::build_swarm(identity.clone()).await?;
        
        Ok(Self {
            swarm,
            identity,
            local_peer_id,
            subscribed_topics: HashMap::new(),
            event_queue: Vec::new(),
            connected_peers: HashSet::new(),
            relay_nodes: HashSet::new(),
            relay_addrs: HashMap::new(),
            known_game_peers: HashMap::new(),
        })
    }
    
    /// Build the libp2p Swarm with all protocols configured
    async fn build_swarm(identity: Identity) -> Result<Swarm<MetaverseBehaviour>> {
        let local_peer_id = identity.peer_id().clone();
        let keypair = identity.to_libp2p_keypair();
        
        // Build Swarm with multi-transport support for universal connectivity
        // TCP + QUIC + WebSocket = works on open networks, CGNAT, VPNs, firewalls
        let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            // TCP transport (primary, works on open networks)
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| NetworkError::TransportError(format!("{:?}", e)))?
            // QUIC transport (better NAT traversal, faster handshake, UDP-based)
            .with_quic()
            // DNS resolution (required for websocket + hostname dials)
            .with_dns()
            .map_err(|e| NetworkError::TransportError(format!("{:?}", e)))?
            // WebSocket transport (port 443/80 fallback - works through VPNs, firewalls, CGNAT)
            // Both sides connect OUTBOUND to a WSS relay - no inbound port needed
            .with_websocket(
                (libp2p::tls::Config::new, noise::Config::new),
                yamux::Config::default,
            )
            .await
            .map_err(|e| NetworkError::TransportError(format!("{:?}", e)))?
            // Relay client (can use relays for NAT traversal + DCUtR hole punching)
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .map_err(|e| NetworkError::TransportError(format!("{:?}", e)))?
            .with_behaviour(|keypair, relay_behaviour| {
                // Configure Kademlia DHT
                let kad_store = MemoryStore::new(local_peer_id);
                let mut kad_config = kad::Config::default();
                kad_config.set_query_timeout(Duration::from_secs(5 * 60));
                let mut kademlia = kad::Behaviour::with_config(
                    local_peer_id,
                    kad_store,
                    kad_config,
                );
                // Server mode: advertise our addresses (including circuit addresses) to the DHT
                // This is how other peers find us when we're behind CGNAT/VPN/Starlink
                kademlia.set_mode(Some(kad::Mode::Server));
                
                // Configure Gossipsub
                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(1))
                    .validation_mode(ValidationMode::Strict)
                    .max_transmit_size(1024 * 1024) // 1 MB max message size (for state sync)
                    .flood_publish(true) // Send to all peers when mesh is small (< D peers)
                    .mesh_n_low(1)       // Allow mesh with just 1 peer
                    .mesh_n(2)           // Target mesh size of 2
                    .mesh_n_high(4)      // Max mesh size before pruning
                    .message_id_fn(|msg| {
                        use sha2::{Sha256, Digest};
                        let mut hasher = Sha256::new();
                        hasher.update(&msg.data);
                        hasher.update(msg.source.as_ref().map(|p| p.to_bytes()).unwrap_or_default().as_slice());
                        hasher.update(&msg.sequence_number.unwrap_or(0).to_le_bytes());
                        gossipsub::MessageId::from(hasher.finalize().to_vec())
                    })
                    .build()
                    .map_err(|e| NetworkError::SwarmBuildError(e.to_string()))?;
                
                let gossipsub = gossipsub::Behaviour::new(
                    MessageAuthenticity::Signed(keypair.clone()),
                    gossipsub_config,
                ).map_err(|e| NetworkError::SwarmBuildError(e.to_string()))?;
                
                // Configure mDNS for local network discovery
                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    local_peer_id,
                ).map_err(|e| NetworkError::SwarmBuildError(e.to_string()))?;
                
                // Configure Identify protocol
                let identify = identify::Behaviour::new(
                    identify::Config::new(
                        "/metaverse/1.0.0".to_string(),
                        keypair.public(),
                    )
                );
                
                // DCUtR for hole punching
                // DCUtR disabled - breaks CGNAT connections (carrier NAT is not hole-punchable)
                // Relay circuit is sufficient and stable
                
                // Configure Relay Server - enables this peer to relay for others
                // Conservative limits for client nodes (not dedicated relays)
                let relay_server_config = relay::Config {
                    max_reservations: 128,                              // Allow 128 peers to reserve slots
                    max_reservations_per_peer: 4,                       // Each peer can reserve 4 slots
                    max_circuits: 16,                                   // Allow 16 simultaneous relay circuits
                    max_circuits_per_peer: 4,                           // Each peer can use 4 circuits through us
                    max_circuit_duration: Duration::from_secs(3600),     // 1 hour per circuit
                    max_circuit_bytes: 1024 * 1024,                     // 1 MB per circuit (state sync only)
                    ..Default::default()
                };
                let relay_server = relay::Behaviour::new(local_peer_id, relay_server_config);
                
                // Configure AutoNAT - detect our own NAT status
                let autonat = autonat::Behaviour::new(
                    local_peer_id,
                    autonat::Config {
                        // Only servers with confirmed public addresses can be servers
                        only_global_ips: true,
                        // Ask 3 peers to probe us
                        boot_delay: Duration::from_secs(15),
                        // Refresh every 5 minutes
                        refresh_interval: Duration::from_secs(300),
                        // Retry failed probes
                        retry_interval: Duration::from_secs(60),
                        // Throttle to avoid spamming
                        throttle_server_period: Duration::ZERO,
                        ..Default::default()
                    },
                );
                
                Ok(MetaverseBehaviour {
                    kademlia,
                    gossipsub,
                    mdns,
                    identify,
                    relay_client: relay_behaviour,
                    relay_server,
                    autonat,
                })
            })
            .map_err(|e| NetworkError::SwarmBuildError(format!("{:?}", e)))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();
        
        Ok(swarm)
    }
    
    /// Start listening on the given multiaddr
    ///
    /// Example: "/ip4/0.0.0.0/tcp/0" to listen on random port
    pub fn listen_on(&mut self, addr: &str) -> Result<()> {
        let multiaddr: Multiaddr = addr.parse()
            .map_err(|e| NetworkError::MultiaddrParseError(format!("{}", e)))?;
        
        self.swarm.listen_on(multiaddr)
            .map_err(|e| NetworkError::ListenError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        
        Ok(())
    }
    
    /// Dial a peer at the given multiaddr
    pub fn dial(&mut self, addr: &str) -> Result<()> {
        let multiaddr: Multiaddr = addr.parse()
            .map_err(|e| NetworkError::MultiaddrParseError(format!("{}", e)))?;
        
        self.swarm.dial(multiaddr)
            .map_err(|e| NetworkError::SwarmBuildError(e.to_string()))?;
        
        Ok(())
    }

    /// Fetch bootstrap nodes from the remote URL (or cache/fallback) and dial them all.
    /// Also dials Protocol Labs public relay nodes and registers relay circuit listeners.
    /// Call this once after `listen_on` to join the network.
    pub async fn connect_to_bootstrap(&mut self) {
        // Protocol Labs public relay/bootstrap nodes - permanent infrastructure, no hosting needed
        // Both sides connect OUTBOUND to these - works through CGNAT, VPN, Starlink, 4G
        let public_relay_nodes: &[(&str, &str)] = &[
            ("QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
             "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN"),
            ("QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
             "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa"),
            ("QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
             "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb"),
            ("QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
             "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt"),
        ];

        // Dial Protocol Labs nodes and mark them as known relay nodes
        for (peer_id_str, addr) in public_relay_nodes {
            if let Ok(peer_id) = peer_id_str.parse::<PeerId>() {
                self.relay_nodes.insert(peer_id);
            }
            match self.dial(addr) {
                Ok(()) => println!("[bootstrap] Dialing public relay: {}", addr),
                Err(e) => eprintln!("[bootstrap] Failed to dial {}: {}", addr, e),
            }
        }

        // Also dial nodes from our bootstrap.json (Gist / cache / hardcoded fallback)
        let nodes = crate::bootstrap::resolve_bootstrap_nodes().await;
        println!("[bootstrap] Dialing {} node(s) from bootstrap file", nodes.len());
        for addr in &nodes {
            // Mark as relay node so we listen on circuit when connected
            if let Ok(ma) = addr.parse::<Multiaddr>() {
                if let Some(libp2p::multiaddr::Protocol::P2p(peer_id)) = ma.iter().last() {
                    self.relay_nodes.insert(peer_id);
                }
            }
            match self.dial(addr) {
                Ok(()) => println!("[bootstrap] Dialing: {}", addr),
                Err(e) => eprintln!("[bootstrap] Failed to dial {}: {}", addr, e),
            }
        }

        // Kick off DHT bootstrap query
        self.swarm.behaviour_mut().kademlia.bootstrap().ok();

        // Re-register circuit relay for every currently connected relay node.
        // If relay TCP is still up but circuit reservation expired, calling listen_via_relay()
        // again requests a fresh reservation. ConnectionEstablished won't fire for already-connected
        // relays, so we must do this explicitly on every reconnect attempt.
        let relay_pairs: Vec<(PeerId, Multiaddr)> = self.relay_addrs.iter()
            .filter(|(p, _)| self.connected_peers.contains(*p))
            .map(|(p, a)| (*p, a.clone()))
            .collect();
        for (relay_peer_id, relay_base) in relay_pairs {
            println!("[relay] Re-registering circuit with connected relay {}", relay_peer_id);
            self.listen_via_relay(relay_peer_id, relay_base);
        }

        // Redial known game peers that we've dropped.
        // RoutingUpdated only fires for NEW DHT entries — peers already in our routing
        // table won't trigger it again, so we dial them explicitly.
        self.redial_known_game_peers();
    }

    /// Redial any known game peers we're no longer connected to.
    pub fn redial_known_game_peers(&mut self) {
        let to_dial: Vec<(PeerId, Multiaddr)> = self.known_game_peers
            .iter()
            .filter(|(peer, _)| !self.connected_peers.contains(*peer))
            .map(|(p, a)| (*p, a.clone()))
            .collect();

        for (peer_id, addr) in to_dial {
            // Prefer dialing via relay circuit if we have one
            let dial_addr = if addr.to_string().contains("p2p-circuit") {
                // Address is already a circuit addr
                addr.with(libp2p::multiaddr::Protocol::P2p(peer_id))
            } else {
                // Try via each known relay
                let relay_circuit: Option<Multiaddr> = self.relay_addrs.iter()
                    .find(|(rp, _)| self.connected_peers.contains(*rp))
                    .map(|(relay_peer_id, relay_base)| {
                        relay_base.clone()
                            .with(libp2p::multiaddr::Protocol::P2p(*relay_peer_id))
                            .with(libp2p::multiaddr::Protocol::P2pCircuit)
                            .with(libp2p::multiaddr::Protocol::P2p(peer_id))
                    });
                match relay_circuit {
                    Some(c) => c,
                    None => addr.with(libp2p::multiaddr::Protocol::P2p(peer_id)),
                }
            };
            println!("🔄 [Network] Redialing known peer {} via {}", peer_id, dial_addr);
            self.swarm.dial(dial_addr).ok();
        }
    }

    /// Listen on a relay circuit - makes this peer reachable through the relay.
    /// Works through CGNAT, VPN, Starlink - no inbound port needed.
    fn listen_via_relay(&mut self, relay_peer_id: PeerId, relay_addr: Multiaddr) {
        // Circuit relay listen address: <relay-multiaddr>/p2p-circuit
        let circuit_addr = relay_addr
            .with(libp2p::multiaddr::Protocol::P2p(relay_peer_id))
            .with(libp2p::multiaddr::Protocol::P2pCircuit);
        println!("[relay] Listening via circuit: {}", circuit_addr);
        if let Err(e) = self.swarm.listen_on(circuit_addr) {
            eprintln!("[relay] Failed to listen on circuit: {}", e);
        }
    }

    
    /// Subscribe to a topic
    ///
    /// After subscribing, messages published to this topic will trigger
    /// NetworkEvent::Message events when polled.
    pub fn subscribe(&mut self, topic_name: &str) -> Result<()> {
        if self.subscribed_topics.contains_key(topic_name) {
            return Ok(()); // Already subscribed
        }
        
        let topic = IdentTopic::new(topic_name);
        
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)
            .map_err(|e| NetworkError::PublishError(e.to_string()))?;
        
        self.subscribed_topics.insert(topic_name.to_string(), topic);
        self.event_queue.push(NetworkEvent::TopicSubscribed {
            topic: topic_name.to_string(),
        });
        
        Ok(())
    }
    
    /// Unsubscribe from a topic
    pub fn unsubscribe(&mut self, topic_name: &str) -> Result<()> {
        if let Some(topic) = self.subscribed_topics.remove(topic_name) {
            // unsubscribe() returns bool (true if was subscribed), not Result
            let _ = self.swarm.behaviour_mut().gossipsub.unsubscribe(&topic);
            
            self.event_queue.push(NetworkEvent::TopicUnsubscribed {
                topic: topic_name.to_string(),
            });
        }
        
        Ok(())
    }
    
    /// Publish data to a topic
    ///
    /// Returns error if not subscribed to the topic.
    pub fn publish(&mut self, topic_name: &str, data: Vec<u8>) -> Result<()> {
        let topic = self.subscribed_topics.get(topic_name)
            .ok_or_else(|| NetworkError::TopicNotSubscribed(topic_name.to_string()))?
            .clone();
        
        println!("🟢 [NETWORK] Publishing to {}: {} bytes", topic_name, data.len());
        
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)
            .map_err(|e| NetworkError::PublishError(e.to_string()))?;
        
        Ok(())
    }
    
    /// Poll for network events (non-blocking)
    ///
    /// Returns Some(event) if an event is available, None otherwise.
    /// Should be called frequently in the main loop.
    pub fn poll(&mut self) -> Option<NetworkEvent> {
        // First, drain any queued events
        if !self.event_queue.is_empty() {
            return Some(self.event_queue.remove(0));
        }
        
        // Poll the swarm for new events (non-blocking)
        // Use pollster to run the async poll in a blocking context
        pollster::block_on(async {
            use futures::FutureExt;
            match self.swarm.next().now_or_never() {
                Some(Some(event)) => self.handle_swarm_event(event),
                _ => None,
            }
        })
    }

    /// Wait for the next network event (async, properly drives the swarm).
    /// Use this in async contexts instead of poll().
    pub async fn next_event(&mut self) -> NetworkEvent {
        loop {
            if !self.event_queue.is_empty() {
                return self.event_queue.remove(0);
            }
            use futures::StreamExt;
            let event = self.swarm.next().await.unwrap();
            if let Some(net_event) = self.handle_swarm_event(event) {
                return net_event;
            }
        }
    }
    
    /// Handle a swarm event and convert to NetworkEvent
    pub(crate) fn handle_swarm_event(&mut self, event: SwarmEvent<MetaverseBehaviourEvent>) -> Option<NetworkEvent> {
        match event {
            // Gossipsub message received
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Gossipsub(
                gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message,
                    ..
                }
            )) => {
                let topic = message.topic.to_string();
                let data = message.data;
                
                println!("🔵 [NETWORK] Gossipsub message received! topic={}, from={}, size={} bytes", 
                    topic, peer_id, data.len());
                
                Some(NetworkEvent::Message {
                    peer_id,
                    topic,
                    data,
                })
            }
            
            // mDNS discovered a peer
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Mdns(
                mdns::Event::Discovered(peers)
            )) => {
                for (peer_id, multiaddr) in peers {
                    // Add to Kademlia DHT
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr.clone());

                    // Dial immediately — mDNS gives us a direct LAN address, use it
                    if !self.connected_peers.contains(&peer_id) {
                        let _ = self.swarm.dial(multiaddr);
                    }

                    // Queue discovery event
                    self.event_queue.push(NetworkEvent::PeerDiscovered {
                        peer_id: peer_id.clone(),
                    });
                }
                
                // Return first discovery event
                self.event_queue.pop()
            }
            
            // mDNS expired a peer
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Mdns(
                mdns::Event::Expired(peers)
            )) => {
                for (peer_id, _) in peers {
                    if self.connected_peers.remove(&peer_id) {
                        return Some(NetworkEvent::PeerDisconnected { peer_id });
                    }
                }
                None
            }
            
            // Connection established
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint,
                ..
            } => {
                self.connected_peers.insert(peer_id);
                let remote_addr = endpoint.get_remote_address().clone();

                // If this is a known relay node, listen on circuit relay immediately
                // This makes us reachable through the relay - works through CGNAT/VPN/Starlink
                if self.relay_nodes.contains(&peer_id) {
                    // Strip the peer ID suffix from the address for the circuit listen
                    let relay_base: Multiaddr = remote_addr.iter()
                        .filter(|p| !matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
                        .collect();
                    // Store for later circuit re-registration on reconnect
                    self.relay_addrs.insert(peer_id, relay_base.clone());
                    self.listen_via_relay(peer_id, relay_base);
                } else {
                    // Game peer — remember their address so we can redial after reconnect.
                    // RoutingUpdated only fires for NEW DHT entries; if we drop and
                    // reconnect, the peer is already in our routing table and won't re-fire,
                    // so we dial them explicitly using this saved address.
                    self.known_game_peers.insert(peer_id, remote_addr.clone());
                }

                Some(NetworkEvent::PeerConnected {
                    peer_id,
                    address: remote_addr,
                })
            }
            
            // Connection closed — only remove peer when ALL connections to them are gone.
            // libp2p opens multiple connections per peer (QUIC + TCP + circuit).
            // Removing on any single close causes spurious "Peers: 0" while still connected.
            SwarmEvent::ConnectionClosed {
                peer_id,
                num_established,
                ..
            } => {
                if num_established == 0 {
                    self.connected_peers.remove(&peer_id);
                    Some(NetworkEvent::PeerDisconnected { peer_id })
                } else {
                    None // Still have other connections to this peer
                }
            }
            
            // New listen address
            SwarmEvent::NewListenAddr { address, .. } => {
                // If we got a circuit address, re-advertise to DHT so other peers find us
                if address.to_string().contains("p2p-circuit") {
                    self.swarm.behaviour_mut().kademlia.bootstrap().ok();
                }
                Some(NetworkEvent::ListeningOn { address })
            }
            
            // Identify protocol received peer info
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info, .. }
            )) => {
                // Add peer's listen addresses to Kademlia — skip virtual/loopback addrs
                for addr in &info.listen_addrs {
                    if is_routable_addr(addr) {
                        self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                    }
                }
                // If peer advertises circuit relay v2 support, mark as relay and listen via it
                let relay_proto = libp2p::core::upgrade::Version::V1;
                let is_relay = info.protocols.iter().any(|p| {
                    p.as_ref().contains("/libp2p/circuit/relay/0.2.0/hop")
                });
                if is_relay && !self.relay_nodes.contains(&peer_id) {
                    println!("[relay] Peer {} supports relay, listening via circuit", peer_id);
                    self.relay_nodes.insert(peer_id);
                    if let Some(addr) = info.listen_addrs.first() {
                        let relay_base: Multiaddr = addr.iter()
                            .filter(|p| !matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
                            .collect();
                        self.listen_via_relay(peer_id, relay_base);
                    }
                }
                None
            }
            
            // Relay client events - NAT traversal coordination
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::RelayClient(event)) => {
                match event {
                    relay::client::Event::ReservationReqAccepted { relay_peer_id, renewal, .. } => {
                        println!("✅ [RELAY] Reservation {} by relay: {}", 
                            if renewal { "renewed" } else { "accepted" }, relay_peer_id);
                        None
                    }
                    relay::client::Event::OutboundCircuitEstablished { relay_peer_id, .. } => {
                        println!("🔄 [RELAY] Circuit established via {}", relay_peer_id);
                        None
                    }
                    relay::client::Event::InboundCircuitEstablished { src_peer_id, .. } => {
                        println!("📞 [RELAY] Inbound circuit from {}", src_peer_id);
                        None
                    }
                }
            }
            
            // Relay server events - we're acting as a relay for others
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::RelayServer(event)) => {
                match event {
                    relay::Event::ReservationReqAccepted { src_peer_id, renewed, .. } => {
                        println!("✅ [RELAY SERVER] Reservation {} for peer: {}", 
                            if renewed { "renewed" } else { "accepted" }, src_peer_id);
                        None
                    }
                    relay::Event::ReservationTimedOut { src_peer_id } => {
                        println!("⏱️  [RELAY SERVER] Reservation timed out for: {}", src_peer_id);
                        None
                    }
                    relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id } => {
                        println!("🔄 [RELAY SERVER] Circuit: {} → {}", src_peer_id, dst_peer_id);
                        None
                    }
                    relay::Event::CircuitReqDenied { src_peer_id, dst_peer_id, .. } => {
                        println!("❌ [RELAY SERVER] Circuit denied: {} → {}", src_peer_id, dst_peer_id);
                        None
                    }
                    relay::Event::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                        println!("🔚 [RELAY SERVER] Circuit closed: {} → {}", src_peer_id, dst_peer_id);
                        None
                    }
                    relay::Event::ReservationReqDenied { src_peer_id, .. } => {
                        println!("❌ [RELAY SERVER] Reservation denied for: {}", src_peer_id);
                        None
                    }
                    // Other relay events (accept failed, deny failed, closed, etc.)
                    _ => {
                        println!("ℹ️  [RELAY SERVER] Event: {:?}", event);
                        None
                    }
                }
            }
            
            // AutoNAT events - Detect our NAT status
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Autonat(event)) => {
                match event {
                    autonat::Event::StatusChanged { old, new } => {
                        let (old_str, new_str, external_addr) = match (&old, &new) {
                            (autonat::NatStatus::Unknown, autonat::NatStatus::Public(addr)) => {
                                ("Unknown", "Public", Some(addr.clone()))
                            }
                            (autonat::NatStatus::Unknown, autonat::NatStatus::Private) => {
                                ("Unknown", "Private (NAT)", None)
                            }
                            (autonat::NatStatus::Public(_), autonat::NatStatus::Private) => {
                                ("Public", "Private (NAT)", None)
                            }
                            (autonat::NatStatus::Private, autonat::NatStatus::Public(addr)) => {
                                ("Private (NAT)", "Public", Some(addr.clone()))
                            }
                            _ => return None,
                        };
                        
                        println!("🔍 [AUTONAT] NAT status: {} → {}", old_str, new_str);
                        if let Some(ref addr) = external_addr {
                            println!("   External address: {}", addr);
                        }
                        
                        Some(NetworkEvent::NatStatusChanged {
                            old_status: old_str.to_string(),
                            new_status: new_str.to_string(),
                            external_address: external_addr,
                        })
                    }
                    autonat::Event::InboundProbe(_) | autonat::Event::OutboundProbe(_) => {
                        // Probing activity - don't spam console
                        None
                    }
                }
            }
            
            // Kademlia DHT events - peer discovery via DHT
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Kademlia(event)) => {
                match event {
                    kad::Event::RoutingUpdated { peer, addresses, .. } => {
                        // New peer found in DHT - dial via circuit address preferentially
                        if !self.connected_peers.contains(&peer) {
                            let all_addrs: Vec<_> = addresses.iter().collect();
                            // Prefer circuit address (works through CGNAT/VPN/Starlink)
                            let circuit_addr = all_addrs.iter()
                                .find(|a| a.to_string().contains("p2p-circuit"));
                            if let Some(addr) = circuit_addr {
                                let dial_addr = (*addr).clone()
                                    .with(libp2p::multiaddr::Protocol::P2p(peer));
                                println!("🔍 [DHT] Found peer {}, dialing via circuit: {}", peer, dial_addr);
                                self.swarm.dial(dial_addr).ok();
                            } else {
                                // No circuit address yet — try dialing via all known relays
                                // The peer may not have announced its circuit addr to DHT yet
                                let mut dialed_via_relay = false;
                                for (relay_peer, relay_addr) in &self.relay_addrs {
                                    let circuit = relay_addr.clone()
                                        .with(libp2p::multiaddr::Protocol::P2pCircuit)
                                        .with(libp2p::multiaddr::Protocol::P2p(peer));
                                    println!("🔍 [DHT] Found peer {} (no circuit addr yet), trying via relay {}", peer, relay_peer);
                                    self.swarm.dial(circuit).ok();
                                    dialed_via_relay = true;
                                }
                                if !dialed_via_relay {
                                    // No relay known, try direct (last resort)
                                    if let Some(addr) = all_addrs.first() {
                                        let dial_addr = (*addr).clone()
                                            .with(libp2p::multiaddr::Protocol::P2p(peer));
                                        println!("🔍 [DHT] Found peer {}, dialing direct: {}", peer, dial_addr);
                                        self.swarm.dial(dial_addr).ok();
                                    }
                                }
                            }
                        }
                        None
                    }
                    kad::Event::OutboundQueryProgressed {
                        result: kad::QueryResult::Bootstrap(Ok(kad::BootstrapOk { num_remaining, .. })),
                        ..
                    } => {
                        if num_remaining == 0 {
                            println!("✅ [DHT] Bootstrap complete");
                        }
                        None
                    }
                    _ => None,
                }
            }

            // Ignore other events
            _ => None,
        }
    }
    
    /// Get local peer ID
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }
    
    /// Get number of connected peers
    pub fn connected_peer_count(&self) -> usize {
        self.connected_peers.len()
    }

    /// Count only non-relay game peers (relay nodes don't count as "connected players")
    pub fn game_peer_count(&self) -> usize {
        self.connected_peers.iter().filter(|p| !self.relay_nodes.contains(*p)).count()
    }
    
    /// Get list of connected peers
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.connected_peers.iter().cloned().collect()
    }
    
    /// Check if subscribed to a topic
    pub fn is_subscribed(&self, topic_name: &str) -> bool {
        self.subscribed_topics.contains_key(topic_name)
    }
    
    /// Get list of subscribed topics
    pub fn subscribed_topics(&self) -> Vec<String> {
        self.subscribed_topics.keys().cloned().collect()
    }
}

impl std::fmt::Debug for NetworkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkNode")
            .field("local_peer_id", &self.local_peer_id)
            .field("connected_peers", &self.connected_peers.len())
            .field("subscribed_topics", &self.subscribed_topics.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    
    #[test]
    fn test_network_node_creation() {
        let identity = Identity::generate();
        let network = NetworkNode::new(identity);
        assert!(network.is_ok());
    }
    
    #[test]
    fn test_subscribe_unsubscribe() {
        let identity = Identity::generate();
        let mut network = NetworkNode::new(identity).unwrap();
        
        // Subscribe to topic
        network.subscribe("test-topic").unwrap();
        assert!(network.is_subscribed("test-topic"));
        
        // Unsubscribe
        network.unsubscribe("test-topic").unwrap();
        assert!(!network.is_subscribed("test-topic"));
    }
    
    #[test]
    fn test_publish_requires_subscription() {
        let identity = Identity::generate();
        let mut network = NetworkNode::new(identity).unwrap();
        
        // Publishing without subscription should fail
        let result = network.publish("test-topic", vec![1, 2, 3]);
        assert!(result.is_err());
        
        // After subscribing, should work
        network.subscribe("test-topic").unwrap();
        let result = network.publish("test-topic", vec![1, 2, 3]);
        assert!(result.is_ok());
    }
}
