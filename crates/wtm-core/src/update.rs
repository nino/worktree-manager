//! What the in-app updater reads, and how it decides there is something to
//! install. The fetching and installing are the backend's job (they mean
//! HTTPS, code signatures and app bundles); this is the part worth testing.

use serde::Deserialize;

/// The manifest published beside a release (`appcast.json`, written by
/// `.github/workflows/release.yml`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// The released version, e.g. `1.0.42`.
    pub version: String,
    /// Where the stapled `.app` zip lives.
    pub url: String,
    /// Hex SHA-256 of that zip.
    pub sha256: String,
    /// The oldest macOS this build runs on.
    #[serde(default)]
    pub minimum_system_version: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
}

/// Where an update has got to, for the UI to report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    /// Not a release build (run from `cargo run`, or an unsigned copy): there
    /// is nothing to update and nothing to say about it.
    #[default]
    Unsupported,
    Idle,
    Checking,
    UpToDate,
    Downloading(String),
    /// Installed alongside the running copy; a restart switches to it.
    Ready(String),
    Failed(String),
}

/// Whether `candidate` is a later version than `current`.
///
/// Versions are the dotted numbers the release workflow stamps
/// (`1.0.<commits on main>`); anything unparsable sorts as 0, and a version
/// with more parts wins a tie, so `1.0.1.1` beats `1.0.1`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (a, b) = (parts(candidate), parts(current));
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

impl Manifest {
    /// Parse a manifest, rejecting one that could not lead anywhere: an update
    /// is only ever installed from an HTTPS URL, and only when its hash is
    /// there to check the download against.
    pub fn parse(json: &str) -> Result<Manifest, String> {
        let manifest: Manifest =
            serde_json::from_str(json).map_err(|e| format!("unreadable update manifest: {e}"))?;
        if !manifest.url.starts_with("https://") {
            return Err(format!("update URL is not https: {}", manifest.url));
        }
        if manifest.sha256.len() != 64 || !manifest.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("update manifest has no usable sha256".into());
        }
        if manifest.version.trim().is_empty() {
            return Err("update manifest has no version".into());
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
        "version": "1.0.42",
        "url": "https://github.com/nino/worktree-manager/releases/latest/download/app.zip",
        "sha256": "c6b90d6aeed473483170f40e700ffbd8fa3820aa97919f9eba16d1edafee2b76",
        "minimumSystemVersion": "13.0",
        "commit": "abc",
        "publishedAt": "2026-09-07T06:18:21Z"
    }"#;

    #[test]
    fn parses_a_release_manifest() {
        let m = Manifest::parse(GOOD).unwrap();
        assert_eq!(m.version, "1.0.42");
        assert_eq!(m.minimum_system_version.as_deref(), Some("13.0"));
    }

    #[test]
    fn refuses_a_manifest_it_could_not_act_on_safely() {
        let plain_http = GOOD.replace("https://", "http://");
        assert!(Manifest::parse(&plain_http).is_err());
        let short_hash = GOOD.replace(
            "c6b90d6aeed473483170f40e700ffbd8fa3820aa97919f9eba16d1edafee2b76",
            "abc",
        );
        assert!(Manifest::parse(&short_hash).is_err());
        assert!(Manifest::parse("not json").is_err());
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("1.0.42", "1.0.41"));
        assert!(is_newer("1.0.100", "1.0.99"));
        assert!(!is_newer("1.0.41", "1.0.42"));
        assert!(!is_newer("1.0.42", "1.0.42"));
        assert!(is_newer("1.1.0", "1.0.999"));
        // A dev build's placeholder is older than anything released.
        assert!(is_newer("1.0.1", "0.1.0"));
        assert!(is_newer("v1.0.2", "1.0.1"));
        assert!(!is_newer("nonsense", "1.0.1"));
    }
}
