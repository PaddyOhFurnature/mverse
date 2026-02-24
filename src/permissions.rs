//! Permission system — what each key type is allowed to do.
//!
//! # Design
//!
//! Permissions are checked **independently** by every peer. No central authority
//! grants permissions at runtime. The rule set is deterministic: given the same
//! [`KeyRecord`] and the same world state, every peer reaches the same decision.
//!
//! The check is split into two independent layers:
//!
//! 1. **Key-type layer** ([`check_key_type`]): Does this key type support this
//!    class of action at all? A Guest key can never claim a parcel, regardless
//!    of world state.
//!
//! 2. **Ownership layer** (future, wired from `user_content.rs`): Does the
//!    author own or have access to the target location?
//!
//! ## Toggleable enforcement
//!
//! [`PermissionConfig`] has individual boolean flags for each layer. In
//! production all flags are `true`. During development and testing, flags can
//! be disabled independently so you can iterate on gameplay without cryptographic
//! friction.
//!
//! ```rust
//! // Development: disable ownership checks, keep signature checks
//! let cfg = PermissionConfig {
//!     verify_signatures:  true,
//!     verify_key_types:   true,
//!     verify_ownership:   false,  // parcels not yet implemented
//!     verify_expiry:      true,
//!     verify_revocation:  true,
//! };
//! ```
//!
//! # Full permission table
//!
//! See `docs/IDENTITY_SYSTEM.md` §4.1 for the authoritative table.
//! This module enforces that table in code.

use crate::identity::{KeyRecord, KeyType};
use crate::key_registry::KeyRegistry;
use crate::messages::Action;

// ─── Configuration ────────────────────────────────────────────────────────────

/// Toggle individual permission-check layers on or off.
///
/// All flags default to `true` (production behaviour).
/// Set individual flags to `false` during development or testing.
#[derive(Debug, Clone)]
pub struct PermissionConfig {
    /// Verify Ed25519 `self_sig` on the author's `KeyRecord` before accepting any op.
    pub verify_signatures: bool,

    /// Enforce key-type permission table (e.g., Guest cannot claim parcels).
    pub verify_key_types: bool,

    /// Enforce spatial ownership (author must own / have access to the target coord).
    /// Requires world state — wired in once the parcel system is complete.
    pub verify_ownership: bool,

    /// Reject operations from keys whose `expires_at` is in the past.
    pub verify_expiry: bool,

    /// Reject operations from keys marked `revoked = true` in the registry.
    pub verify_revocation: bool,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            verify_signatures: true,
            verify_key_types:  true,
            verify_ownership:  false, // not yet wired — set true once parcels land
            verify_expiry:     true,
            verify_revocation: true,
        }
    }
}

impl PermissionConfig {
    /// Permissive config for unit tests — all checks disabled.
    pub fn permissive() -> Self {
        Self {
            verify_signatures: false,
            verify_key_types:  false,
            verify_ownership:  false,
            verify_expiry:     false,
            verify_revocation: false,
        }
    }

    /// Production config — all checks enabled.
    pub fn production() -> Self {
        Self {
            verify_ownership: true,
            ..Self::default()
        }
    }
}

// ─── Action classes ───────────────────────────────────────────────────────────

/// High-level classes of actions, used by the key-type permission table.
///
/// Each variant maps to a row in the permission table in `IDENTITY_SYSTEM.md`.
/// Concrete [`Action`] variants (from `user_content.rs`) map to these classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionClass {
    /// Move/explore the world (no build). All keys allow this.
    Explore,

    /// Place or remove voxels in a free-build public zone.
    BuildInFreeZone,

    /// Place or remove voxels in an owned parcel (ownership also checked).
    BuildInOwnedParcel,

    /// Claim an unclaimed parcel as owned land.
    ClaimParcel,

    /// Abandon / release a parcel.
    AbandonParcel,

    /// Transfer ownership of a parcel or item to another peer.
    TransferOwnership,

    /// Grant build access to another peer within your parcel.
    GrantAccess,

    /// Revoke previously granted access.
    RevokeAccess,

    /// Create or publish a blueprint / model asset.
    PublishContent,

    /// Import an external asset into the world.
    ImportAsset,

    /// Create a commerce listing (item for sale).
    CreateListing,

    /// Sign a trade or service contract.
    SignContract,

    /// Deploy a script that runs in-world.
    DeployScript,

    /// Vote on a region governance proposal.
    Vote,

    /// Create a governance proposal (requires parcel ownership in region).
    CreateProposal,

    /// Kick or ban a user from a region.
    ModerateUser,

    /// Publish or update a `KeyRecord` on the network.
    PublishKeyRecord,

    /// Revoke another peer's key (self-revoke or authority revoke).
    RevokeKey,

    /// Register or update relay node configuration.
    ManageRelay,

    /// Issue a Relay or Admin key (Server key only).
    IssueKey,
}

// ─── Permission result ────────────────────────────────────────────────────────

/// The result of a permission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    /// Operation is allowed.
    Allowed,

    /// Key type does not support this action class.
    KeyTypeNotAllowed,

    /// Key has been revoked.
    Revoked,

    /// Key has expired.
    Expired,

    /// Signature verification failed (key record tampered or unknown).
    InvalidSignature,

    /// Author does not own the target parcel and has not been granted access.
    NotOwner,

    /// Location is in a restricted zone the author cannot edit.
    Restricted,

    /// This check is disabled by [`PermissionConfig`] — treated as `Allowed`.
    Disabled,
}

impl PermissionResult {
    /// True if the operation should proceed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, PermissionResult::Allowed | PermissionResult::Disabled)
    }

    /// Shorthand for `!is_allowed()`.
    pub fn is_denied(&self) -> bool {
        !self.is_allowed()
    }
}

impl std::fmt::Display for PermissionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allowed            => write!(f, "allowed"),
            Self::KeyTypeNotAllowed  => write!(f, "key type not allowed for this action"),
            Self::Revoked            => write!(f, "key has been revoked"),
            Self::Expired            => write!(f, "key has expired"),
            Self::InvalidSignature   => write!(f, "key record signature invalid"),
            Self::NotOwner           => write!(f, "author does not own target parcel"),
            Self::Restricted         => write!(f, "target zone is restricted"),
            Self::Disabled           => write!(f, "permission check disabled (dev mode)"),
        }
    }
}

// ─── Key-type permission table ────────────────────────────────────────────────

/// Check whether a [`KeyType`] is allowed to perform an [`ActionClass`].
///
/// This is the first-pass check — it only looks at the key type, not the
/// world state. Ownership checks are applied separately.
///
/// Returns `true` if this key type is allowed this action class.
pub fn key_type_allows(key_type: KeyType, action: ActionClass) -> bool {
    use ActionClass::*;
    use KeyType::*;
    match action {
        // ── All keys can explore ──────────────────────────────────────────────
        Explore => true,

        // ── Free-build zones ──────────────────────────────────────────────────
        BuildInFreeZone => !matches!(key_type, Relay),

        // ── Owned-parcel build: User+ only ────────────────────────────────
        BuildInOwnedParcel => matches!(key_type, User | Business | Admin | Server | Genesis),

        // ── Parcel claiming: User+ only ──────────────────────────────────
        ClaimParcel => matches!(key_type, User | Business | Server | Genesis),

        // ── Parcel abandon: only the owner (User+) ────────────────────────
        AbandonParcel => matches!(key_type, User | Business | Server | Genesis),

        // ── Ownership transfer ────────────────────────────────────────────────
        TransferOwnership => matches!(key_type, User | Business | Server | Genesis),

        // ── Access management ─────────────────────────────────────────────────
        GrantAccess  => matches!(key_type, User | Business | Admin | Server | Genesis),
        RevokeAccess => matches!(key_type, User | Business | Admin | Server | Genesis),

        // ── Content creation ─────────────────────────────────────────────────
        PublishContent => !matches!(key_type, Guest | Relay),
        ImportAsset    => !matches!(key_type, Guest | Relay),

        // ── Commerce ─────────────────────────────────────────────────────────
        CreateListing => matches!(key_type, User | Business | Server | Genesis),
        SignContract  => matches!(key_type, User | Business | Server | Genesis),

        // ── Scripts ──────────────────────────────────────────────────────────
        DeployScript => matches!(key_type, User | Business | Admin | Server | Genesis),

        // ── Governance ───────────────────────────────────────────────────────
        Vote           => matches!(key_type, User | Business | Admin | Server),
        CreateProposal => matches!(key_type, User | Business | Admin | Server | Genesis),

        // ── Moderation ───────────────────────────────────────────────────────
        ModerateUser => matches!(key_type, Admin | Server | Genesis),

        // ── Identity management ───────────────────────────────────────────────
        PublishKeyRecord => true, // any key can publish its own record
        RevokeKey => true,        // self-revoke: any key; authority revoke: checked separately

        // ── Infrastructure ────────────────────────────────────────────────────
        ManageRelay => matches!(key_type, Relay | Server | Genesis),
        IssueKey    => matches!(key_type, Server | Genesis),
    }
}

// ─── Full permission check (key-level) ───────────────────────────────────────

/// Check all key-level permissions for an operation.
///
/// This checks: signature validity, revocation, expiry, key-type permission.
/// It does **not** check ownership (spatial world state) — that is handled
/// separately in `user_content.rs` once the parcel system is implemented.
///
/// # Arguments
/// * `registry` — the key registry to look up the author's `KeyRecord`
/// * `author` — the `PeerId` claiming to have authored the operation
/// * `author_pubkey` — the public key used to sign the operation (from the message itself)
/// * `op_bytes` — the canonical bytes that were signed
/// * `signature` — the Ed25519 signature over `op_bytes`
/// * `action` — the action class being requested
/// * `config` — which checks are currently enabled
pub fn check_key_level_permission(
    registry: &mut KeyRegistry,
    author: &libp2p::PeerId,
    author_pubkey: &[u8; 32],
    op_bytes: &[u8],
    signature: &[u8; 64],
    action: ActionClass,
    config: &PermissionConfig,
) -> PermissionResult {
    // ── 1. Signature check ────────────────────────────────────────────────────
    if config.verify_signatures {
        use ed25519_dalek::{Signature, VerifyingKey, Verifier};
        let Ok(vk) = VerifyingKey::from_bytes(author_pubkey) else {
            return PermissionResult::InvalidSignature;
        };
        let sig = Signature::from_bytes(signature);
        if vk.verify(op_bytes, &sig).is_err() {
            return PermissionResult::InvalidSignature;
        }
    }

    // ── 2. Load KeyRecord ─────────────────────────────────────────────────────
    let record = registry.get_or_default(*author);

    // Verify the record actually belongs to this author and is untampered
    if config.verify_signatures && !record.verify_self_sig() {
        return PermissionResult::InvalidSignature;
    }

    // ── 3. Revocation check ───────────────────────────────────────────────────
    if config.verify_revocation && record.revoked {
        return PermissionResult::Revoked;
    }

    // ── 4. Expiry check ───────────────────────────────────────────────────────
    if config.verify_expiry && record.is_expired() {
        return PermissionResult::Expired;
    }

    // ── 5. Key-type permission table ──────────────────────────────────────────
    if config.verify_key_types {
        let effective = record.effective_key_type();
        if !key_type_allows(effective, action) {
            return PermissionResult::KeyTypeNotAllowed;
        }
    }

    PermissionResult::Allowed
}

/// Lightweight key-type check without needing registry or signature bytes.
///
/// Use this when you already have a verified `KeyRecord` in hand and just
/// need to know if the key type supports the action.
pub fn check_record_permission(
    record: &KeyRecord,
    action: ActionClass,
    config: &PermissionConfig,
) -> PermissionResult {
    if config.verify_revocation && record.revoked {
        return PermissionResult::Revoked;
    }
    if config.verify_expiry && record.is_expired() {
        return PermissionResult::Expired;
    }
    if config.verify_key_types && !key_type_allows(record.effective_key_type(), action) {
        return PermissionResult::KeyTypeNotAllowed;
    }
    PermissionResult::Allowed
}

// ─── Action → ActionClass mapping ────────────────────────────────────────────

/// Map a concrete [`Action`] to its permission [`ActionClass`].
///
/// This is the bridge between the gameplay `Action` enum (which has 21 concrete
/// variants covering every mutable game operation) and the 20-class permission
/// table that determines what each key type is allowed to do.
///
/// The mapping is conservative: when a voxel edit could be in either a free
/// zone or an owned parcel, we use `BuildInFreeZone` here and rely on the
/// ownership check (in `user_content.rs`) to elevate it to `BuildInOwnedParcel`
/// when the target coordinate is inside a parcel.
pub fn action_to_class(action: &Action) -> ActionClass {
    match action {
        // Terrain edits default to free-zone build; ownership layer upgrades when needed
        Action::SetVoxel { .. }       => ActionClass::BuildInFreeZone,
        Action::RemoveVoxel { .. }    => ActionClass::BuildInFreeZone,
        Action::FillRegion { .. }     => ActionClass::BuildInFreeZone,

        // Object manipulation also defaults to free-zone
        Action::PlaceObject { .. }    => ActionClass::BuildInFreeZone,
        Action::RemoveObject { .. }   => ActionClass::BuildInFreeZone,
        Action::MoveObject { .. }     => ActionClass::BuildInFreeZone,
        Action::ConfigureObject { .. }=> ActionClass::BuildInFreeZone,

        // Parcel management
        Action::ClaimParcel { .. }    => ActionClass::ClaimParcel,
        Action::AbandonParcel { .. }  => ActionClass::AbandonParcel,
        Action::TransferOwnership { .. } => ActionClass::TransferOwnership,
        Action::GrantAccess { .. }    => ActionClass::GrantAccess,
        Action::RevokeAccess { .. }   => ActionClass::RevokeAccess,

        // Commerce
        Action::CreateListing { .. }  => ActionClass::CreateListing,
        Action::AcceptListing { .. }  => ActionClass::SignContract,
        Action::CancelListing { .. }  => ActionClass::TransferOwnership,
        Action::SignContract { .. }   => ActionClass::SignContract,

        // Content creation
        Action::PublishBlueprint { .. } => ActionClass::PublishContent,
        Action::ImportAsset { .. }      => ActionClass::ImportAsset,

        // Identity management
        Action::PublishKeyRecord { .. } => ActionClass::PublishKeyRecord,
        Action::RevokeKey { .. }        => ActionClass::RevokeKey,

        // Infrastructure
        Action::RegisterRelay { .. }    => ActionClass::ManageRelay,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    fn record(id: &Identity, key_type: KeyType) -> KeyRecord {
        id.create_key_record(key_type, None, None, None, None, None)
    }

    // ── Key-type permission table ─────────────────────────────────────────────

    #[test]
    fn test_all_keys_can_explore() {
        for kt in [KeyType::Genesis, KeyType::Server, KeyType::Relay,
                   KeyType::Admin, KeyType::Business, KeyType::User,
                   KeyType::Trial, KeyType::Guest] {
            assert!(key_type_allows(kt, ActionClass::Explore),
                "{:?} must be able to explore", kt);
        }
    }

    #[test]
    fn test_guest_cannot_claim_parcel() {
        assert!(!key_type_allows(KeyType::Guest, ActionClass::ClaimParcel));
    }

    #[test]
    fn test_anonymous_cannot_claim_parcel() {
        assert!(!key_type_allows(KeyType::Trial, ActionClass::ClaimParcel));
    }

    #[test]
    fn test_personal_can_claim_parcel() {
        assert!(key_type_allows(KeyType::User, ActionClass::ClaimParcel));
    }

    #[test]
    fn test_business_can_claim_parcel() {
        assert!(key_type_allows(KeyType::Business, ActionClass::ClaimParcel));
    }

    #[test]
    fn test_relay_cannot_build() {
        assert!(!key_type_allows(KeyType::Relay, ActionClass::BuildInFreeZone));
        assert!(!key_type_allows(KeyType::Relay, ActionClass::BuildInOwnedParcel));
    }

    #[test]
    fn test_guest_cannot_sign_contract() {
        assert!(!key_type_allows(KeyType::Guest, ActionClass::SignContract));
    }

    #[test]
    fn test_personal_can_sign_contract() {
        assert!(key_type_allows(KeyType::User, ActionClass::SignContract));
    }

    #[test]
    fn test_only_server_can_issue_keys() {
        assert!(key_type_allows(KeyType::Server, ActionClass::IssueKey));
        assert!(key_type_allows(KeyType::Genesis, ActionClass::IssueKey));
        for kt in [KeyType::Relay, KeyType::Admin, KeyType::Business,
                   KeyType::User, KeyType::Trial, KeyType::Guest] {
            assert!(!key_type_allows(kt, ActionClass::IssueKey),
                "{:?} must not be able to issue keys", kt);
        }
    }

    #[test]
    fn test_only_admin_plus_can_moderate() {
        assert!(key_type_allows(KeyType::Admin, ActionClass::ModerateUser));
        assert!(key_type_allows(KeyType::Server, ActionClass::ModerateUser));
        assert!(!key_type_allows(KeyType::User, ActionClass::ModerateUser));
        assert!(!key_type_allows(KeyType::Guest, ActionClass::ModerateUser));
    }

    #[test]
    fn test_any_key_can_publish_own_record() {
        for kt in [KeyType::Guest, KeyType::Trial, KeyType::User,
                   KeyType::Business, KeyType::Admin, KeyType::Server, KeyType::Relay] {
            assert!(key_type_allows(kt, ActionClass::PublishKeyRecord),
                "{:?} must be able to publish own key record", kt);
        }
    }

    // ── check_record_permission ───────────────────────────────────────────────

    #[test]
    fn test_check_record_permission_valid_personal() {
        let id = Identity::generate();
        let rec = record(&id, KeyType::User);
        let cfg = PermissionConfig::default();
        let result = check_record_permission(&rec, ActionClass::ClaimParcel, &cfg);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_check_record_permission_guest_claim_denied() {
        let id = Identity::generate();
        let rec = record(&id, KeyType::Guest);
        let cfg = PermissionConfig::default();
        let result = check_record_permission(&rec, ActionClass::ClaimParcel, &cfg);
        assert_eq!(result, PermissionResult::KeyTypeNotAllowed);
    }

    #[test]
    fn test_check_record_permission_revoked_denied() {
        let id = Identity::generate();
        let original = record(&id, KeyType::User);
        let revoked = id.self_revoke(&original, None).unwrap();
        let cfg = PermissionConfig::default();
        let result = check_record_permission(&revoked, ActionClass::ClaimParcel, &cfg);
        assert_eq!(result, PermissionResult::Revoked);
    }

    #[test]
    fn test_check_record_permission_disabled_always_allows() {
        let id = Identity::generate();
        let rec = record(&id, KeyType::Guest);
        let cfg = PermissionConfig::permissive();
        // Even Guest claiming parcel is allowed when checks are off
        let result = check_record_permission(&rec, ActionClass::ClaimParcel, &cfg);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_check_record_permission_expired() {
        let id = Identity::generate();
        let past = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
            .saturating_sub(3600); // 1 hour ago
        let mut rec = id.create_key_record(KeyType::User, None, None, None, Some(past), None);
        // Re-sign with expiry in the past
        let _ = id.update_key_record(&rec, None, None, None); // just to get a valid sig
        // Manually set expires_at in past and re-sign
        rec.expires_at = Some(past);
        let msg = rec.canonical_bytes_for_self_sig();
        use ed25519_dalek::Signer;
        rec.self_sig = id.signing_key().sign(&msg).to_bytes();

        let cfg = PermissionConfig::default();
        let result = check_record_permission(&rec, ActionClass::Explore, &cfg);
        assert_eq!(result, PermissionResult::Expired);
    }

    // ── PermissionResult ──────────────────────────────────────────────────────

    #[test]
    fn test_allowed_is_allowed() {
        assert!(PermissionResult::Allowed.is_allowed());
        assert!(PermissionResult::Disabled.is_allowed());
        assert!(!PermissionResult::KeyTypeNotAllowed.is_allowed());
        assert!(!PermissionResult::Revoked.is_allowed());
        assert!(!PermissionResult::Expired.is_allowed());
    }
}
