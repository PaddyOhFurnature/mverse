//! Cryptographic identity system for P2P metaverse
//!
//! Each user has an Ed25519 keypair that serves as their cryptographic identity:
//! - **Private key:** Never leaves your machine, used to sign operations
//! - **Public key:** Shared with peers, used to verify your signatures
//! - **PeerId:** Derived from public key, your unique identifier in the network
//!
//! # Identity Persistence
//!
//! Keypairs are stored in `~/.metaverse/identity.key` and persist across sessions.
//! If the file doesn't exist, a new identity is generated on first run.
//!
//! **WARNING:** Losing your private key means losing your identity. All your
//! signed voxel operations will be attributed to this key. Backup your identity file!
//!
//! # Usage
//!
//! ```no_run
//! use metaverse_core::identity::Identity;
//!
//! // Load existing identity or create new one
//! let identity = Identity::load_or_create().expect("Failed to load identity");
//!
//! println!("Your PeerId: {}", identity.peer_id());
//!
//! // Sign some data
//! let data = b"Player dug voxel at (100, 50, 200)";
//! let signature = identity.sign(data);
//!
//! // Verify signature (anyone can do this with your public key)
//! assert!(Identity::verify(identity.peer_id(), data, &signature));
//! ```

use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use libp2p::identity::{Keypair as Libp2pKeypair, ed25519 as libp2p_ed25519};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Result type for identity operations
pub type Result<T> = std::result::Result<T, IdentityError>;

/// Errors that can occur during identity operations
#[derive(Debug)]
pub enum IdentityError {
    /// Failed to read or write identity file
    IoError(std::io::Error),
    
    /// Failed to serialize or deserialize keypair
    SerializationError(bincode::Error),
    
    /// Invalid signature
    InvalidSignature,
    
    /// Invalid keypair data
    InvalidKeypair,
    
    /// Failed to create .metaverse directory
    DirectoryCreationFailed(std::io::Error),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::SerializationError(e) => write!(f, "Serialization error: {}", e),
            Self::InvalidSignature => write!(f, "Invalid signature"),
            Self::InvalidKeypair => write!(f, "Invalid keypair"),
            Self::DirectoryCreationFailed(e) => write!(f, "Failed to create directory: {}", e),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<std::io::Error> for IdentityError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

impl From<bincode::Error> for IdentityError {
    fn from(e: bincode::Error) -> Self {
        Self::SerializationError(e)
    }
}

/// Cryptographic identity for a peer in the metaverse
///
/// Each identity consists of an Ed25519 signing key (private + public) and derived PeerId.
/// The signing key never leaves the local machine and is used to sign
/// all operations (voxel modifications, chat messages, etc.).
///
/// The PeerId is derived deterministically from the public key and serves
/// as the unique identifier for this peer in the P2P network.
#[derive(Clone)]
pub struct Identity {
    /// Ed25519 signing key (contains both secret and public key)
    signing_key: SigningKey,
    
    /// Ed25519 verifying key (public key, derived from signing key)
    verifying_key: VerifyingKey,
    
    /// libp2p PeerId (derived from public key)
    peer_id: PeerId,
}

/// Serializable representation of an identity (for disk storage)
#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    /// Secret key (32 bytes)
    secret_key: [u8; 32],
    
    /// Public key (32 bytes)
    public_key: [u8; 32],
}

impl Identity {
    /// Get the default path for storing identity
    ///
    /// Returns `~/.metaverse/identity.key`
    fn default_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| {
                IdentityError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find home directory",
                ))
            })?;
        
        Ok(home.join(".metaverse").join("identity.key"))
    }
    
    /// Ensure the `.metaverse` directory exists
    fn ensure_metaverse_dir() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| {
                IdentityError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find home directory",
                ))
            })?;
        
        let metaverse_dir = home.join(".metaverse");
        
        if !metaverse_dir.exists() {
            fs::create_dir(&metaverse_dir)
                .map_err(IdentityError::DirectoryCreationFailed)?;
            
            eprintln!("Created ~/.metaverse/ directory for identity and cache storage");
        }
        
        Ok(metaverse_dir)
    }
    
    /// Generate a new random identity
    ///
    /// Uses cryptographically secure randomness via `rand::rngs::OsRng`.
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        
        // Derive libp2p PeerId from public key
        let peer_id = Self::derive_peer_id(&signing_key);
        
        Self { signing_key, verifying_key, peer_id }
    }
    
    /// Derive a libp2p PeerId from an Ed25519 signing key
    fn derive_peer_id(signing_key: &SigningKey) -> PeerId {
        // Convert dalek signing key to libp2p keypair
        let libp2p_keypair = {
            let secret_bytes = signing_key.to_bytes();
            let libp2p_secret = libp2p_ed25519::SecretKey::try_from_bytes(secret_bytes)
                .expect("Valid ed25519 secret key");
            let libp2p_keypair = libp2p_ed25519::Keypair::from(libp2p_secret);
            Libp2pKeypair::from(libp2p_keypair)
        };
        
        PeerId::from(libp2p_keypair.public())
    }
    
    /// Load identity from disk, or generate a new one if it doesn't exist
    ///
    /// This is the primary entry point for obtaining an identity.
    ///
    /// # Example
    /// ```no_run
    /// let identity = Identity::load_or_create()?;
    /// ```
    pub fn load_or_create() -> Result<Self> {
        let path = Self::default_path()?;
        
        if path.exists() {
            eprintln!("Loading identity from {}", path.display());
            Self::load_from_path(&path)
        } else {
            eprintln!("No identity found, generating new one...");
            let identity = Self::generate();
            
            // Ensure directory exists before saving
            Self::ensure_metaverse_dir()?;
            
            identity.save_to_path(&path)?;
            eprintln!("Identity saved to {}", path.display());
            eprintln!("PeerId: {}", identity.peer_id);
            eprintln!("⚠️  BACKUP THIS FILE! Losing it means losing your identity.");
            
            Ok(identity)
        }
    }
    
    /// Load identity from a specific path
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        let bytes = fs::read(path)?;
        let stored: StoredIdentity = bincode::deserialize(&bytes)?;
        
        // Reconstruct signing key and verifying key
        let signing_key = SigningKey::from_bytes(&stored.secret_key);
        let verifying_key = VerifyingKey::from_bytes(&stored.public_key)
            .map_err(|_| IdentityError::InvalidKeypair)?;
        
        // Derive PeerId
        let peer_id = Self::derive_peer_id(&signing_key);
        
        Ok(Self { signing_key, verifying_key, peer_id })
    }
    
    /// Save identity to a specific path
    pub fn save_to_path(&self, path: &PathBuf) -> Result<()> {
        let stored = StoredIdentity {
            secret_key: self.signing_key.to_bytes(),
            public_key: self.verifying_key.to_bytes(),
        };
        
        let bytes = bincode::serialize(&stored)?;
        fs::write(path, bytes)?;
        
        Ok(())
    }
    
    /// Get the PeerId for this identity
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }
    
    /// Get the verifying key (public key)
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }
    
    /// Sign arbitrary data with this identity's private key
    ///
    /// The signature can be verified by anyone with your public key/PeerId.
    ///
    /// # Example
    /// ```no_run
    /// let data = b"Voxel at (100, 50, 200) changed to STONE";
    /// let signature = identity.sign(data);
    /// ```
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }
    
    /// Verify a signature against this identity's public key
    ///
    /// Returns `true` if the signature is valid for the given data.
    pub fn verify_own(&self, data: &[u8], signature: &Signature) -> bool {
        self.verifying_key.verify(data, signature).is_ok()
    }
    
    /// Verify a signature with an explicit verifying key (public key)
    ///
    /// Use this when you have the peer's public key directly.
    pub fn verify_with_pubkey(
        verifying_key: &VerifyingKey,
        data: &[u8],
        signature: &Signature,
    ) -> bool {
        verifying_key.verify(data, signature).is_ok()
    }
    
    /// Convert this identity to a libp2p Keypair
    ///
    /// Used when initializing the libp2p Swarm.
    pub fn to_libp2p_keypair(&self) -> Libp2pKeypair {
        let secret_bytes = self.signing_key.to_bytes();
        let libp2p_secret = libp2p_ed25519::SecretKey::try_from_bytes(secret_bytes)
            .expect("Valid ed25519 secret key");
        let libp2p_keypair = libp2p_ed25519::Keypair::from(libp2p_secret);
        Libp2pKeypair::from(libp2p_keypair)
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("peer_id", &self.peer_id)
            .field("verifying_key", &format!("{:02x?}", &self.verifying_key.to_bytes()[..8]))
            .field("signing_key", &"<REDACTED>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_identity() {
        let identity = Identity::generate();
        assert_ne!(identity.peer_id.to_string(), "");
    }
    
    #[test]
    fn test_sign_and_verify() {
        let identity = Identity::generate();
        let data = b"Test message for signing";
        
        let signature = identity.sign(data);
        
        // Verify with own public key
        assert!(identity.verify_own(data, &signature));
        
        // Verify with wrong data should fail
        let wrong_data = b"Different message";
        assert!(!identity.verify_own(wrong_data, &signature));
    }
    
    #[test]
    fn test_verify_with_pubkey() {
        let identity = Identity::generate();
        let data = b"Test data";
        let signature = identity.sign(data);
        
        // Verify using explicit verifying key
        assert!(Identity::verify_with_pubkey(
            identity.verifying_key(),
            data,
            &signature
        ));
        
        // Wrong data should fail
        assert!(!Identity::verify_with_pubkey(
            identity.verifying_key(),
            b"Wrong data",
            &signature
        ));
    }
    
    #[test]
    fn test_identity_persistence() {
        use tempfile::NamedTempFile;
        
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        
        // Generate and save
        let identity1 = Identity::generate();
        let peer_id1 = identity1.peer_id().clone();
        identity1.save_to_path(&path).unwrap();
        
        // Load and verify
        let identity2 = Identity::load_from_path(&path).unwrap();
        let peer_id2 = identity2.peer_id().clone();
        
        assert_eq!(peer_id1, peer_id2);
        
        // Signatures should be identical
        let data = b"Test";
        let sig1 = identity1.sign(data);
        let sig2 = identity2.sign(data);
        
        assert_eq!(sig1.to_bytes(), sig2.to_bytes());
    }
    
    #[test]
    fn test_peer_id_deterministic() {
        let identity = Identity::generate();
        let peer_id1 = identity.peer_id().clone();
        
        // Derive again from same signing key
        let peer_id2 = Identity::derive_peer_id(&identity.signing_key);
        
        assert_eq!(peer_id1, peer_id2);
    }
}
