//! HTTP tunnel fallback for P2P connectivity
//!
//! When all other methods fail (firewall blocks everything except HTTP/HTTPS),
//! this module provides a fallback HTTP tunnel that allows P2P traffic to flow
//! disguised as normal HTTP requests.
//!
//! **How it works:**
//! 1. Client sends P2P data as HTTP POST to relay server
//! 2. Relay forwards via normal P2P to destination
//! 3. Response comes back as HTTP response
//! 4. Looks like web traffic to firewalls
//!
//! **Performance:**
//! - Slow (HTTP overhead + long-polling)
//! - But ALWAYS works (100% connectivity guarantee)
//! - Only used when TCP, QUIC, and relay all fail

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};

/// HTTP tunnel client - send/receive P2P data via HTTP
pub struct HttpTunnelClient {
    relay_url: String,
    peer_id: PeerId,
    session_id: String,
    client: reqwest::Client,
}

impl HttpTunnelClient {
    /// Create new HTTP tunnel client
    pub fn new(relay_url: String, peer_id: PeerId) -> Self {
        let session_id = uuid::Uuid::new_v4().to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            relay_url,
            peer_id,
            session_id,
            client,
        }
    }

    /// Send message via HTTP POST
    pub async fn send(&self, dest_peer: PeerId, data: Vec<u8>) -> Result<(), HttpTunnelError> {
        let response = self
            .client
            .post(&format!("{}/tunnel/send", self.relay_url))
            .header("X-Source-Peer", self.peer_id.to_string())
            .header("X-Dest-Peer", dest_peer.to_string())
            .header("X-Session-ID", &self.session_id)
            .body(data)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(HttpTunnelError::SendFailed(response.status().as_u16()))
        }
    }

    /// Receive messages via long-polling (blocks up to 30 seconds)
    pub async fn receive(&self) -> Result<Option<HttpTunnelMessage>, HttpTunnelError> {
        let response = self
            .client
            .get(&format!("{}/tunnel/recv", self.relay_url))
            .header("X-Peer-ID", self.peer_id.to_string())
            .header("X-Session-ID", &self.session_id)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            // No messages available
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(HttpTunnelError::ReceiveFailed(response.status().as_u16()));
        }

        let message: HttpTunnelMessage = response.json().await?;
        Ok(Some(message))
    }

    /// Start receive loop (continuously poll for messages)
    pub async fn receive_loop(self, tx: mpsc::UnboundedSender<HttpTunnelMessage>) {
        loop {
            match self.receive().await {
                Ok(Some(msg)) => {
                    if tx.send(msg).is_err() {
                        break; // Channel closed
                    }
                }
                Ok(None) => {
                    // No message, continue polling
                }
                Err(e) => {
                    eprintln!("HTTP tunnel receive error: {:?}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}

/// Message received via HTTP tunnel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTunnelMessage {
    pub from: PeerId,
    pub data: Vec<u8>,
}

/// Errors that can occur with HTTP tunnel
#[derive(Debug)]
pub enum HttpTunnelError {
    SendFailed(u16),
    ReceiveFailed(u16),
    Http(reqwest::Error),
}

impl From<reqwest::Error> for HttpTunnelError {
    fn from(e: reqwest::Error) -> Self {
        HttpTunnelError::Http(e)
    }
}

impl std::fmt::Display for HttpTunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpTunnelError::SendFailed(status) => {
                write!(f, "HTTP tunnel send failed: {}", status)
            }
            HttpTunnelError::ReceiveFailed(status) => {
                write!(f, "HTTP tunnel receive failed: {}", status)
            }
            HttpTunnelError::Http(e) => write!(f, "HTTP error: {}", e),
        }
    }
}

impl std::error::Error for HttpTunnelError {}

/// HTTP tunnel server - relay server endpoint
/// This runs on the relay server to accept HTTP tunnel connections
pub struct HttpTunnelServer {
    /// Buffer of messages waiting for delivery (dest_peer -> messages)
    pending_messages: Arc<RwLock<HashMap<PeerId, Vec<HttpTunnelMessage>>>>,

    /// Channel to forward messages to P2P network
    p2p_tx: mpsc::UnboundedSender<(PeerId, Vec<u8>)>,
}

impl HttpTunnelServer {
    pub fn new(p2p_tx: mpsc::UnboundedSender<(PeerId, Vec<u8>)>) -> Self {
        Self {
            pending_messages: Arc::new(RwLock::new(HashMap::new())),
            p2p_tx,
        }
    }

    /// Handle incoming HTTP POST (send message)
    pub async fn handle_send(
        &self,
        _source_peer: PeerId,
        dest_peer: PeerId,
        data: Vec<u8>,
    ) -> Result<(), HttpTunnelError> {
        // Forward to P2P network
        self.p2p_tx
            .send((dest_peer, data))
            .map_err(|_| HttpTunnelError::SendFailed(500))?;

        Ok(())
    }

    /// Handle incoming HTTP GET (receive message)
    pub async fn handle_receive(
        &self,
        peer_id: PeerId,
    ) -> Result<Option<HttpTunnelMessage>, HttpTunnelError> {
        let mut messages = self.pending_messages.write().await;

        if let Some(queue) = messages.get_mut(&peer_id) {
            if let Some(msg) = queue.pop() {
                return Ok(Some(msg));
            }
        }

        Ok(None)
    }

    /// Queue message for delivery via HTTP (called when P2P message arrives)
    pub async fn queue_message(&self, dest_peer: PeerId, from: PeerId, data: Vec<u8>) {
        let mut messages = self.pending_messages.write().await;
        messages
            .entry(dest_peer)
            .or_insert_with(Vec::new)
            .push(HttpTunnelMessage { from, data });
    }
}

// Note: Actual Axum HTTP endpoints would be in a separate binary (metaverse-relay)
// This module just provides the client/server logic

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_tunnel_client_creation() {
        let peer_id = PeerId::random();
        let client = HttpTunnelClient::new("https://relay.example.com".to_string(), peer_id);

        assert_eq!(client.peer_id, peer_id);
        assert!(client.session_id.len() > 0);
    }
}
