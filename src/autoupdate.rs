//! Binary auto-update using GitHub Releases.
//!
//! Workflow:
//!   1. Query `https://api.github.com/repos/{github_repo}/releases/latest`.
//!   2. If the release tag is newer than the running binary, find the matching
//!      asset (by executable filename) and download it.
//!   3. Atomically replace the current executable.
//!   4. exec()-restart in-place (same PID on Linux; exit for supervisor restart otherwise).
//!
//! Configuration (server.json / relay.json):
//! ```json
//! {
//!   "github_repo": "PaddyOhFurnature/mverse",
//!   "update_check_interval_secs": 21600
//! }
//! ```
//! Set `github_repo` to an empty string to disable auto-update.
//!
//! Asset names in GitHub releases must match the binary filename exactly
//! (e.g. `metaverse-server`, `metaverse-relay`). That is already the case
//! for `PaddyOhFurnature/mverse` releases.

use serde::Deserialize;

// ── GitHub Releases API types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    body:     Option<String>,
    assets:   Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name:                 String,
    browser_download_url: String,
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

/// Query the GitHub Releases API for `github_repo` (e.g. `"PaddyOhFurnature/mverse"`).
///
/// Returns `Some((tag, download_url, release_notes))` if there is a release newer
/// than `current_version` **and** it contains an asset matching this binary's
/// filename.  Returns `None` if up-to-date, the repo is empty, or on any error.
pub async fn check_for_update(
    github_repo: &str,
    current_version: &str,
) -> Option<(String, String, String)> {
    if github_repo.is_empty() { return None; }

    let binary_name = std::env::current_exe().ok()?
        .file_name()?
        .to_string_lossy()
        .to_string();

    let api_url = format!("https://api.github.com/repos/{}/releases/latest", github_repo);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("metaverse-autoupdate/1.0")
        .build()
        .ok()?;

    let release: GhRelease = match client.get(&api_url).send().await {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(v) => v,
            Err(e) => { eprintln!("[AutoUpdate] parse error: {}", e); return None; }
        },
        Ok(r)  => { eprintln!("[AutoUpdate] GitHub API returned {}", r.status()); return None; }
        Err(e) => { eprintln!("[AutoUpdate] GitHub API unreachable: {}", e); return None; }
    };

    if !is_newer(&release.tag_name, current_version) {
        return None; // already up-to-date
    }

    // Find the asset whose name matches this binary
    let asset = release.assets.iter().find(|a| a.name == binary_name)?;

    let notes = release.body.clone().unwrap_or_default();
    Some((release.tag_name, asset.browser_download_url.clone(), notes))
}

/// Download and atomically install the binary at `download_url`, then
/// exec()-restart the process in-place.
///
/// Returns `Err` on failure — the running binary is left untouched.
/// On success this function does **not** return (process is replaced).
pub async fn apply_update(
    version: &str,
    download_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let exe_path = std::env::current_exe()?;
    let tmp_path = exe_path.with_extension("_update_tmp");

    eprintln!("[AutoUpdate] Downloading {}…", version);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("metaverse-autoupdate/1.0")
        .build()?;

    // GitHub releases redirect to the CDN; follow redirects automatically.
    let bytes = client.get(download_url)
        .send().await?
        .bytes().await?;

    eprintln!("[AutoUpdate] Downloaded {} bytes", bytes.len());

    // ── Write + chmod ─────────────────────────────────────────────────────────
    std::fs::write(&tmp_path, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // ── Atomic replace ────────────────────────────────────────────────────────
    std::fs::rename(&tmp_path, &exe_path)?;

    eprintln!("[AutoUpdate] {} installed — restarting…", version);

    // ── Exec-restart: replace process image in-place (same PID) ──────────────
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Reset terminal state before exec so the new process inherits a clean terminal.
        // LeaveAlternateScreen + reset colours + show cursor.
        eprint!("\x1b[?1049l\x1b[0m\x1b[?25h");
        let args: Vec<String> = std::env::args().collect();
        let err = std::process::Command::new(&exe_path)
            .args(&args[1..])
            .exec(); // only returns on error
        // exec() failed but binary is already replaced on disk.
        // Exit so the supervisor (systemd / shell) restarts with the new binary.
        eprintln!("[AutoUpdate] exec failed: {} — exiting for supervisor restart", err);
        std::process::exit(0);
    }

    // Non-Unix fallback: exit and let the supervisor (systemd / etc.) restart.
    #[cfg(not(unix))]
    std::process::exit(0);
}
