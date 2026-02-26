//! Binary auto-update for server and relay nodes.
//!
//! Workflow:
//!   1. Node periodically fetches the operator-configured `manifest_url`.
//!   2. If the manifest version is newer than the running binary, the new binary
//!      is downloaded and its SHA-256 verified.
//!   3. The current executable is atomically replaced.
//!   4. The process exec()-restarts in-place (same PID, new binary image).
//!
//! Manifest format (JSON at the operator-controlled URL):
//! ```json
//! {
//!   "version": "0.1.5",
//!   "download_url": "https://example.com/builds/metaverse-server",
//!   "sha256": "a1b2c3...",
//!   "release_notes": "Bug fixes and new features"
//! }
//! ```
//!
//! The operator is responsible for hosting a manifest per binary per platform.
//! Configure `update_manifest_url` in server.json / relay.json to enable.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Manifest ──────────────────────────────────────────────────────────────────

/// Operator-hosted JSON manifest describing an available release.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// Semantic version string, e.g. `"0.1.5"` or `"v0.1.5"`.
    pub version: String,
    /// Direct download URL for this node's binary.
    pub download_url: String,
    /// Lowercase hex SHA-256 of the downloaded binary.
    pub sha256: String,
    /// Human-readable release notes (shown in the web UI).
    #[serde(default)]
    pub release_notes: String,
}

// ── Version helpers ───────────────────────────────────────────────────────────

fn parse_semver(v: &str) -> (u64, u64, u64) {
    let v = v.trim_start_matches('v');
    let mut parts = v.split('.').map(|s| s.parse::<u64>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// Returns `true` if `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse_semver(candidate) > parse_semver(current)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetch the manifest at `url` and return it if its version is newer than
/// `current_version`. Returns `None` if already up-to-date or on any error.
///
/// Errors are logged to stderr but not propagated — a failed check is silent.
pub async fn check_for_update(url: &str, current_version: &str) -> Option<UpdateManifest> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    let resp = match client.get(url).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r)  => { eprintln!("[AutoUpdate] manifest fetch returned {}", r.status()); return None; }
        Err(e) => { eprintln!("[AutoUpdate] manifest fetch failed: {}", e); return None; }
    };

    let manifest: UpdateManifest = match resp.json().await {
        Ok(m) => m,
        Err(e) => { eprintln!("[AutoUpdate] manifest parse failed: {}", e); return None; }
    };

    if is_newer(&manifest.version, current_version) {
        Some(manifest)
    } else {
        None
    }
}

/// Download, verify, and atomically install the binary described by `manifest`,
/// then exec()-restart in-place.
///
/// Returns `Err` if any step fails — the running binary is left untouched.
/// On success this function does **not** return (the process is replaced).
pub async fn apply_update(manifest: &UpdateManifest) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let exe_path = std::env::current_exe()?;
    let tmp_path = exe_path.with_extension("_update_tmp");

    eprintln!("[AutoUpdate] Downloading v{}…", manifest.version);

    // ── Download ──────────────────────────────────────────────────────────────
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let bytes = client.get(&manifest.download_url)
        .send().await?
        .bytes().await?;

    eprintln!("[AutoUpdate] Downloaded {} bytes, verifying…", bytes.len());

    // ── Verify SHA-256 ────────────────────────────────────────────────────────
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual   = hex::encode(hasher.finalize());
    let expected = manifest.sha256.to_lowercase();

    if actual != expected {
        return Err(format!(
            "SHA-256 mismatch: got {} expected {}", actual, expected
        ).into());
    }

    // ── Write + chmod ─────────────────────────────────────────────────────────
    std::fs::write(&tmp_path, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // ── Atomic replace ────────────────────────────────────────────────────────
    std::fs::rename(&tmp_path, &exe_path)?;

    eprintln!("[AutoUpdate] v{} installed — restarting…", manifest.version);

    // ── Exec-restart: replace process image in-place (same PID) ──────────────
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let args: Vec<String> = std::env::args().collect();
        let err = std::process::Command::new(&exe_path)
            .args(&args[1..])
            .exec(); // only returns on error
        return Err(format!("exec restart failed: {}", err).into());
    }

    // Non-Unix fallback: exit and let the supervisor (systemd/etc.) restart.
    #[cfg(not(unix))]
    std::process::exit(0);
}
