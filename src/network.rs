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
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify,
    kad::{self, store::MemoryStore},
    mdns,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm,
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
}

/// Combined network behaviour for libp2p
///
/// Combines multiple protocols:
/// - Kademlia: DHT for peer discovery and routing
/// - Gossipsub: Publish/subscribe messaging
/// - mDNS: Local network peer discovery
/// - Identify: Peer information exchange
#[derive(NetworkBehaviour)]
pub(crate) struct MetaverseBehaviour {
    pub(crate) kademlia: kad::Behaviour<MemoryStore>,
    pub(crate) gossipsub: gossipsub::Behaviour,
    pub(crate) mdns: mdns::tokio::Behaviour,
    pub(crate) identify: identify::Behaviour,
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
        })
    }
    
    /// Build the libp2p Swarm with all protocols configured
    async fn build_swarm(identity: Identity) -> Result<Swarm<MetaverseBehaviour>> {
        let local_peer_id = identity.peer_id().clone();
        let keypair = identity.to_libp2p_keypair();
        
        // Configure Kademlia DHT
        let kad_store = MemoryStore::new(local_peer_id);
        let mut kad_config = kad::Config::default();
        kad_config.set_query_timeout(Duration::from_secs(5 * 60));
        let kademlia = kad::Behaviour::with_config(
            local_peer_id,
            kad_store,
            kad_config,
        );
        
        // Configure Gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(ValidationMode::Strict)
            .max_transmit_size(1024 * 1024) // 1 MB max message size (for state sync)
            .message_id_fn(|msg| {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(&msg.data);
                // Include source and sequence to prevent deduplication of similar messages
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
        
        // Combine behaviours
        let behaviour = MetaverseBehaviour {
            kademlia,
            gossipsub,
            mdns,
            identify,
        };
        
        // Build Swarm with libp2p v0.56 API
        // Use development_transport which handles TCP + Noise + Yamux automatically
        let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| NetworkError::TransportError(format!("{:?}", e)))?
            .with_behaviour(|_| behaviour)
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
                Some(NetworkEvent::PeerConnected {
                    peer_id,
                    address: endpoint.get_remote_address().clone(),
                })
            }
            
            // Connection closed
            SwarmEvent::ConnectionClosed {
                peer_id,
                ..
            } => {
                self.connected_peers.remove(&peer_id);
                Some(NetworkEvent::PeerDisconnected { peer_id })
            }
            
            // New listen address
            SwarmEvent::NewListenAddr { address, .. } => {
                Some(NetworkEvent::ListeningOn { address })
            }
            
            // Identify protocol received peer info
            SwarmEvent::Behaviour(MetaverseBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info, .. }
            )) => {
                // Add peer's listen addresses to Kademlia
                for addr in info.listen_addrs {
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                }
                None
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
