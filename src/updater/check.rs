//! Pure version-check logic. No IO except `fetch_latest_release` (which
//! is itself a thin wrapper around `reqwest`); everything else is
//! deterministic and unit-testable.

use crate::github::{build_client, fetch_latest_release, Asset, ReleaseJson};
use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};

/// Per-app version metadata sent to the frontend. The shape mirrors what
/// the `<UpdateDialog />` needs to render: human-readable strings plus
/// the bytes we need to actually start a download (`asset_url`, plus the
/// optional `asset_digest_sha256` for integrity verification).
///
/// Implements `Deserialize` because `apply_update` takes the cached
/// `UpdateInfo` straight back from the frontend rather than re-fetching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Version we're running right now (`env!("CARGO_PKG_VERSION")`).
    pub current: String,
    /// Release tag verbatim — e.g. `v3.0.12`.
    pub latest_tag: String,
    /// `latest_tag` with any leading `v` stripped, validated as semver.
    pub latest_version: String,
    /// Markdown release notes from `release.body`. Empty string if the
    /// release omits a body.
    pub body: String,
    /// `release.html_url` so the UI can deep-link to the release page.
    pub html_url: String,
    /// Filename of the matched asset for this platform.
    pub asset_name: String,
    /// `browser_download_url` — used by the apply step to actually fetch.
    pub asset_url: String,
    /// Asset size in bytes. May be 0 if GitHub omits it (rare in practice).
    pub asset_size: u64,
    /// Hex-encoded SHA-256 from the asset's `digest` field, if present.
    /// `None` for old releases that pre-date the GitHub `digest` field.
    pub asset_digest_sha256: Option<String>,
}

/// Map `(env::consts::OS, env::consts::ARCH)` to the `<os>-<arch>` slug
/// embedded in our release asset names (see `scripts/package-zip.sh`).
/// Returns `None` for triples we don't publish — frontend will surface
/// "your platform isn't auto-updatable; check releases manually".
pub fn triple_for_current_platform() -> Option<&'static str> {
    triple_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn triple_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("linux-x64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        ("windows", "x86_64") => Some("windows-x64"),
        _ => None,
    }
}

/// Canonical asset filename our `scripts/package-zip.sh` produces for a
/// given version + triple. Used by tests and as the fallback exact-match
/// before `pick_release_asset` falls back to suffix matching.
pub fn expected_asset_name(version: &str, triple: &str) -> String {
    format!("akagi-{version}-{triple}.zip")
}

/// Find the release asset that targets `triple`. Match is suffix-based
/// (`-<triple>.zip`) so the version-in-filename never has to be threaded
/// through — if a future workflow renames the zip prefix from `akagi-`
/// the picker still works.
pub fn pick_release_asset<'a>(assets: &'a [Asset], triple: &str) -> Option<&'a Asset> {
    let suffix = format!("-{triple}.zip");
    assets.iter().find(|a| a.name.ends_with(&suffix))
}

/// `true` iff `latest` is a strictly-newer semver than `current`. Both
/// inputs accept a leading `v`. Malformed inputs return `false` — we'd
/// rather skip an update than crash, and the user can still see the
/// release page from the Settings card.
pub fn is_newer(current: &str, latest: &str) -> bool {
    let Ok(c) = Version::parse(current.trim_start_matches('v')) else {
        return false;
    };
    let Ok(l) = Version::parse(latest.trim_start_matches('v')) else {
        return false;
    };
    l > c
}

/// Extract the hex SHA-256 portion of a GitHub asset `digest` string,
/// which is shaped like `"sha256:abcdef..."`. Returns `None` for an
/// empty / wrong-algorithm / wrong-shape input.
pub fn parse_sha256_digest(raw: &str) -> Option<String> {
    let (algo, hex) = raw.split_once(':')?;
    if !algo.eq_ignore_ascii_case("sha256") {
        return None;
    }
    let hex = hex.trim();
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

/// Top-level orchestration: hit the latest-release endpoint, find an
/// asset that matches our platform, and compare versions. Returns
/// `Ok(None)` (not an error) for the common "we're already up to date"
/// path and for unsupported platforms.
pub async fn check_for_update(repo: &str) -> Result<Option<UpdateInfo>> {
    let Some(triple) = triple_for_current_platform() else {
        return Ok(None);
    };

    let client = build_client()?;
    let release = fetch_latest_release(&client, repo).await?;
    build_update_info(env!("CARGO_PKG_VERSION"), triple, &release)
}

/// Pure half of `check_for_update`. Split out so tests can drive it
/// without spinning up a network mock.
pub fn build_update_info(
    current: &str,
    triple: &str,
    release: &ReleaseJson,
) -> Result<Option<UpdateInfo>> {
    let tag = release
        .tag_name
        .clone()
        .context("release has no tag_name; can't compare versions")?;
    if !is_newer(current, &tag) {
        return Ok(None);
    }
    let Some(asset) = pick_release_asset(&release.assets, triple) else {
        return Ok(None);
    };

    let latest_version = tag.trim_start_matches('v').to_owned();
    let digest = asset
        .digest
        .as_deref()
        .and_then(parse_sha256_digest);

    Ok(Some(UpdateInfo {
        current: current.to_owned(),
        latest_tag: tag,
        latest_version,
        body: release.body.clone().unwrap_or_default(),
        html_url: release.html_url.clone().unwrap_or_default(),
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: asset.size.unwrap_or(0),
        asset_digest_sha256: digest,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, digest: Option<&str>, size: Option<u64>) -> Asset {
        Asset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.com/{name}"),
            size,
            digest: digest.map(|s| s.to_owned()),
        }
    }

    #[test]
    fn triple_for_known_triples() {
        assert_eq!(triple_for("linux", "x86_64"), Some("linux-x64"));
        assert_eq!(triple_for("macos", "aarch64"), Some("macos-arm64"));
        assert_eq!(triple_for("windows", "x86_64"), Some("windows-x64"));
    }

    #[test]
    fn triple_for_unsupported_returns_none() {
        for (os, arch) in [
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("windows", "aarch64"),
            ("freebsd", "x86_64"),
        ] {
            assert!(triple_for(os, arch).is_none(), "{os}/{arch}");
        }
    }

    #[test]
    fn expected_asset_name_formats_correctly() {
        assert_eq!(
            expected_asset_name("3.0.12", "linux-x64"),
            "akagi-3.0.12-linux-x64.zip"
        );
    }

    #[test]
    fn pick_release_asset_matches_triple_suffix() {
        let assets = vec![
            asset("akagi-3.0.12-linux-x64.zip", None, Some(10)),
            asset("akagi-3.0.12-macos-arm64.zip", None, Some(20)),
            asset("akagi-3.0.12-windows-x64.zip", None, Some(30)),
            asset("checksums.txt", None, None),
        ];
        assert_eq!(
            pick_release_asset(&assets, "linux-x64").map(|a| a.name.as_str()),
            Some("akagi-3.0.12-linux-x64.zip")
        );
        assert_eq!(
            pick_release_asset(&assets, "windows-x64").map(|a| a.name.as_str()),
            Some("akagi-3.0.12-windows-x64.zip")
        );
        assert!(pick_release_asset(&assets, "linux-arm64").is_none());
    }

    #[test]
    fn is_newer_basic_bumps() {
        assert!(is_newer("3.0.11", "3.0.12"));
        assert!(is_newer("3.0.11", "3.1.0"));
        assert!(is_newer("3.0.11", "4.0.0"));
        assert!(!is_newer("3.0.12", "3.0.11"));
        assert!(!is_newer("3.0.12", "3.0.12"));
    }

    #[test]
    fn is_newer_strips_v_prefix() {
        assert!(is_newer("3.0.11", "v3.0.12"));
        assert!(is_newer("v3.0.11", "3.0.12"));
        assert!(is_newer("v3.0.11", "v3.0.12"));
    }

    /// Per CLAUDE.md memory `feedback_version_format.md`, V3's
    /// pre-release tags stay numeric (`3.0.0-1`, `3.0.0-2`, …). Semver's
    /// numeric identifier ordering must order these correctly — string
    /// comparison would put `3.0.0-10` *before* `3.0.0-2`.
    #[test]
    fn is_newer_numeric_prerelease_ordering() {
        assert!(is_newer("3.0.0-2", "3.0.0-10"));
        assert!(is_newer("3.0.0-1", "3.0.0-2"));
        assert!(!is_newer("3.0.0-10", "3.0.0-2"));
        // Pre-release is always less than the corresponding release.
        assert!(is_newer("3.0.0-5", "3.0.0"));
        assert!(!is_newer("3.0.0", "3.0.0-5"));
    }

    #[test]
    fn is_newer_malformed_returns_false() {
        assert!(!is_newer("3.0.11", "not-a-version"));
        assert!(!is_newer("garbage", "3.0.12"));
        assert!(!is_newer("", ""));
    }

    #[test]
    fn parse_sha256_digest_accepts_valid_form() {
        let raw = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(parse_sha256_digest(raw), Some(raw[7..].to_owned()));
    }

    #[test]
    fn parse_sha256_digest_normalises_case() {
        let raw = "SHA256:ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789";
        assert_eq!(parse_sha256_digest(raw).unwrap(), raw[7..].to_ascii_lowercase());
    }

    #[test]
    fn parse_sha256_digest_rejects_wrong_algo_or_shape() {
        assert!(parse_sha256_digest("md5:abc").is_none());
        assert!(parse_sha256_digest("sha256:").is_none());
        assert!(parse_sha256_digest("sha256:notenoughhex").is_none());
        assert!(parse_sha256_digest("nocolonhere").is_none());
        // Not 64 chars
        assert!(parse_sha256_digest("sha256:abc123").is_none());
        // Non-hex char
        let bad = "sha256:".to_owned() + &"z".repeat(64);
        assert!(parse_sha256_digest(&bad).is_none());
    }

    fn release_with(tag: &str, assets: Vec<Asset>) -> ReleaseJson {
        ReleaseJson {
            tag_name: Some(tag.to_owned()),
            name: None,
            body: Some("notes".into()),
            html_url: Some("https://example.com/release".into()),
            assets,
        }
    }

    #[test]
    fn build_update_info_returns_none_for_equal_version() {
        let release = release_with(
            "v3.0.11",
            vec![asset("akagi-3.0.11-linux-x64.zip", None, Some(1))],
        );
        let info = build_update_info("3.0.11", "linux-x64", &release).unwrap();
        assert!(info.is_none());
    }

    #[test]
    fn build_update_info_returns_some_for_newer_version() {
        let release = release_with(
            "v3.0.12",
            vec![
                asset(
                    "akagi-3.0.12-linux-x64.zip",
                    Some(
                        "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
                    ),
                    Some(12345),
                ),
                asset("akagi-3.0.12-windows-x64.zip", None, None),
            ],
        );
        let info = build_update_info("3.0.11", "linux-x64", &release)
            .unwrap()
            .expect("should detect newer version");
        assert_eq!(info.current, "3.0.11");
        assert_eq!(info.latest_tag, "v3.0.12");
        assert_eq!(info.latest_version, "3.0.12");
        assert_eq!(info.asset_name, "akagi-3.0.12-linux-x64.zip");
        assert_eq!(info.asset_size, 12345);
        assert!(info.asset_digest_sha256.is_some());
    }

    #[test]
    fn build_update_info_returns_none_when_no_matching_asset() {
        let release = release_with(
            "v3.0.12",
            vec![asset("akagi-3.0.12-windows-x64.zip", None, None)],
        );
        let info = build_update_info("3.0.11", "linux-x64", &release).unwrap();
        assert!(info.is_none());
    }

    #[test]
    fn build_update_info_errors_on_missing_tag() {
        let release = ReleaseJson {
            tag_name: None,
            name: None,
            body: None,
            html_url: None,
            assets: vec![],
        };
        assert!(build_update_info("3.0.11", "linux-x64", &release).is_err());
    }
}
