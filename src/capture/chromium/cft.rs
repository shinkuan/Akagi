//! Chrome-for-Testing (CfT) downloader.
//!
//! Lets users without a system Chrome install one on demand from
//! Google's official CfT distribution. Avoids bundling a ~150MB browser
//! with the Akagi binary; downloads are runtime-only into a
//! `chrome-for-testing/` directory next to the binary (or the user
//! config dir as a fallback — see [`install_root`]).
//!
//! Manifest URLs (Google):
//! - All versions: <https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json>
//! - Channel pins: <https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json>
//!
//! Install layout under `<install_root>/<version>/`:
//! - Linux: `chrome-linux64/chrome`
//! - macOS: `chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing`
//!   (or `chrome-mac-x64/...` on Intel)
//! - Windows: `chrome-win64\chrome.exe`
//!
//! Triggers (per design):
//! - Settings UI button (explicit)
//! - First-run wizard button (explicit)
//! - **Never** automatic on supervisor start — if no system browser AND
//!   no CfT installed, the supervisor surfaces a notification and
//!   refuses to start. The user clicks Download themselves.

use crate::event_bus::NotifyBus;
use crate::schema::Notification;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// CfT release channel + literal-version pin format used by
/// `ChromiumConfig.cft_channel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
    Dev,
    Canary,
    Literal(String),
}

impl Channel {
    pub fn parse(input: &str) -> Self {
        match input.to_ascii_lowercase().as_str() {
            "stable" | "" => Channel::Stable,
            "beta" => Channel::Beta,
            "dev" => Channel::Dev,
            "canary" => Channel::Canary,
            _ => Channel::Literal(input.to_string()),
        }
    }
}

/// CfT platform identifier expected by the manifest (e.g. `linux64`,
/// `mac-arm64`). Returns `None` on unsupported platforms.
pub fn cft_platform() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("linux64")
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Some("mac-arm64")
        } else {
            Some("mac-x64")
        }
    } else if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            Some("win64")
        } else {
            Some("win32")
        }
    } else {
        None
    }
}

/// Top-level CfT install dir.
///
/// Resolved via [`crate::util::resolve_dir`] so a portable zip keeps the
/// downloaded browser next to the binary (single-folder install). On
/// AppImage / read-only mounts the resolver falls back to
/// `<user_config_root>/chrome-for-testing/`.
pub fn install_root() -> Result<PathBuf> {
    Ok(crate::util::resolve_dir(Path::new(
        "./chrome-for-testing",
    )))
}

/// Per-version install dir: `<install_root>/<version>/`.
pub fn install_dir_for(version: &str) -> Result<PathBuf> {
    Ok(install_root()?.join(version))
}

/// Map an install dir + platform to the chrome executable inside.
pub fn executable_path(install_dir: &Path, platform: &str) -> PathBuf {
    match platform {
        "linux64" => install_dir.join("chrome-linux64").join("chrome"),
        "mac-arm64" => install_dir
            .join("chrome-mac-arm64")
            .join("Google Chrome for Testing.app")
            .join("Contents/MacOS/Google Chrome for Testing"),
        "mac-x64" => install_dir
            .join("chrome-mac-x64")
            .join("Google Chrome for Testing.app")
            .join("Contents/MacOS/Google Chrome for Testing"),
        "win64" => install_dir.join("chrome-win64").join("chrome.exe"),
        "win32" => install_dir.join("chrome-win32").join("chrome.exe"),
        _ => install_dir.join(platform).join("chrome"),
    }
}

/// List installed CfT versions, newest first (lex-sort descending —
/// `131.0.6778.85 > 130.0.6723.92` because the dotted segments are
/// fixed-width-ish for the same major). Skips entries whose chrome
/// executable for the current platform is missing.
pub fn list_installed() -> Vec<String> {
    let Ok(root) = install_root() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return vec![];
    };
    let Some(platform) = cft_platform() else {
        return vec![];
    };
    let mut versions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let exe = executable_path(&path, platform);
        if !exe.exists() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            versions.push(name.to_string());
        }
    }
    versions.sort_by_key(|v| std::cmp::Reverse(version_sort_key(v)));
    versions
}

/// Best-effort `127.0.6778.85` → `[127, 0, 6778, 85]` for comparison.
/// Non-numeric components sort as `i64::MIN` so malformed names trail.
fn version_sort_key(v: &str) -> Vec<i64> {
    v.split('.')
        .map(|seg| seg.parse::<i64>().unwrap_or(i64::MIN))
        .collect()
}

/// Find an installed CfT executable. `pinned` (from
/// `ChromiumConfig.cft_channel`) is preferred when it parses as a
/// literal version and that version is installed; otherwise returns
/// the newest installed version's executable. `None` if nothing is
/// installed for the current platform.
pub fn installed_executable(pinned: &Channel) -> Option<PathBuf> {
    let platform = cft_platform()?;
    let installed = list_installed();
    if installed.is_empty() {
        return None;
    }
    let pick = match pinned {
        Channel::Literal(v) if installed.iter().any(|x| x == v) => v.clone(),
        _ => installed[0].clone(),
    };
    let dir = install_dir_for(&pick).ok()?;
    Some(executable_path(&dir, platform))
}

/// Remove a single installed CfT version. Idempotent — missing dir
/// returns Ok.
pub fn remove(version: &str) -> Result<()> {
    let dir = install_dir_for(version)?;
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))
}

// ---------- Manifest fetching ----------

#[derive(Debug, Clone, Deserialize)]
struct ManifestDownload {
    platform: String,
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestDownloads {
    #[serde(default)]
    chrome: Vec<ManifestDownload>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelEntry {
    version: String,
    downloads: ManifestDownloads,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelsManifest {
    channels: ChannelsBag,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelsBag {
    #[serde(rename = "Stable")]
    stable: Option<ChannelEntry>,
    #[serde(rename = "Beta")]
    beta: Option<ChannelEntry>,
    #[serde(rename = "Dev")]
    dev: Option<ChannelEntry>,
    #[serde(rename = "Canary")]
    canary: Option<ChannelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionEntry {
    version: String,
    downloads: ManifestDownloads,
}

#[derive(Debug, Clone, Deserialize)]
struct AllVersionsManifest {
    versions: Vec<VersionEntry>,
}

const CHANNELS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";
const ALL_VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json";

/// Resolve a `Channel` to a concrete `(version, asset_url)` pair for
/// the host platform. Two manifest fetches at most:
/// - For Stable/Beta/Dev/Canary, only the channels endpoint.
/// - For a literal version, only the all-versions endpoint.
async fn resolve_asset(client: &reqwest::Client, channel: &Channel) -> Result<(String, String)> {
    let platform = cft_platform()
        .ok_or_else(|| anyhow!("Chrome-for-Testing is not available for this platform"))?;
    match channel {
        Channel::Stable | Channel::Beta | Channel::Dev | Channel::Canary => {
            let m: ChannelsManifest = client
                .get(CHANNELS_URL)
                .send()
                .await
                .context("fetch CfT channels manifest")?
                .error_for_status()?
                .json()
                .await
                .context("parse CfT channels manifest")?;
            let entry = match channel {
                Channel::Stable => m.channels.stable,
                Channel::Beta => m.channels.beta,
                Channel::Dev => m.channels.dev,
                Channel::Canary => m.channels.canary,
                Channel::Literal(_) => unreachable!(),
            }
            .ok_or_else(|| anyhow!("channel not present in CfT manifest"))?;
            let url = entry
                .downloads
                .chrome
                .into_iter()
                .find(|d| d.platform == platform)
                .ok_or_else(|| {
                    anyhow!("CfT manifest has no {platform} asset for {}", entry.version)
                })?
                .url;
            Ok((entry.version, url))
        }
        Channel::Literal(v) => {
            let m: AllVersionsManifest = client
                .get(ALL_VERSIONS_URL)
                .send()
                .await
                .context("fetch CfT all-versions manifest")?
                .error_for_status()?
                .json()
                .await
                .context("parse CfT all-versions manifest")?;
            let entry = m
                .versions
                .into_iter()
                .find(|e| &e.version == v)
                .ok_or_else(|| anyhow!("CfT version {v:?} not found in manifest"))?;
            let url = entry
                .downloads
                .chrome
                .into_iter()
                .find(|d| d.platform == platform)
                .ok_or_else(|| anyhow!("CfT manifest has no {platform} asset for {v}"))?
                .url;
            Ok((entry.version, url))
        }
    }
}

// ---------- Download + extract ----------

/// Download + extract the requested CfT into the install dir. Reports
/// progress via NotifyBus with sticky id `capture-cft-download` so the
/// frontend can show a single live toast. Idempotent: if the version
/// is already installed, returns early.
pub async fn install(channel: &Channel, notify: &NotifyBus) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("akagi-cft-downloader")
        .build()
        .context("build http client")?;
    let toast = "capture-cft-download";

    let _ = notify.send(
        Notification::info("Resolving Chrome for Testing")
            .body(format!("Channel: {channel:?}"))
            .sticky()
            .id(toast),
    );
    let (version, url) = resolve_asset(&client, channel).await?;
    let install_dir = install_dir_for(&version)?;
    if install_dir.exists() && installed_chrome_exists(&install_dir) {
        let _ = notify.send(
            Notification::success(format!("Chrome for Testing {version} already installed"))
                .id(toast),
        );
        return Ok(version);
    }

    info!("downloading CfT {version} from {url}");
    let _ = notify.send(
        Notification::info(format!("Downloading Chrome for Testing {version}"))
            .body("0%")
            .sticky()
            .id(toast),
    );

    // Stage download under <root>/.downloads/ so a partial transfer
    // doesn't pollute the version dir.
    let staging_root = install_root()?.join(".downloads");
    std::fs::create_dir_all(&staging_root)
        .with_context(|| format!("create {}", staging_root.display()))?;
    let zip_path = staging_root.join(format!("cft-{version}.zip"));
    download_with_progress(&client, &url, &zip_path, notify, toast, &version).await?;

    let _ = notify.send(
        Notification::info(format!("Installing Chrome for Testing {version}"))
            .body("Extracting…")
            .sticky()
            .id(toast),
    );
    std::fs::create_dir_all(&install_dir)
        .with_context(|| format!("create {}", install_dir.display()))?;
    crate::bot::install::extract_zip_safe(&zip_path, &install_dir).context("extract CfT zip")?;
    let _ = std::fs::remove_file(&zip_path);

    if let Some(platform) = cft_platform() {
        let exe = executable_path(&install_dir, platform);
        if !exe.exists() {
            // Cleanup the half-installed dir to avoid `list_installed`
            // returning a broken version.
            let _ = std::fs::remove_dir_all(&install_dir);
            bail!(
                "CfT extract finished but expected executable {} is missing",
                exe.display()
            );
        }
        post_extract_fixup(&exe, &install_dir);
    }

    let _ = notify
        .send(Notification::success(format!("Chrome for Testing {version} installed")).id(toast));
    Ok(version)
}

fn installed_chrome_exists(install_dir: &Path) -> bool {
    let Some(platform) = cft_platform() else {
        return false;
    };
    executable_path(install_dir, platform).exists()
}

/// Defensive `chmod +x` (Unix) and macOS quarantine strip after
/// extraction. CfT zips DO carry unix mode bits via `extract_zip_safe`,
/// but a stray archive without them shouldn't leave a non-executable
/// binary behind. macOS Gatekeeper blocks unsigned binaries with the
/// quarantine xattr — strip it so first launch doesn't show a
/// "cannot verify developer" prompt.
fn post_extract_fixup(exe: &Path, install_dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(exe) {
            let mut perms = meta.permissions();
            // Add user/group/other execute bits without dropping read/write.
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(exe, perms);
        }
    }
    if cfg!(target_os = "macos") {
        let status = std::process::Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(install_dir)
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {
                debug!("stripped quarantine xattr from {}", install_dir.display())
            }
            Ok(s) => warn!("xattr exited {s} (non-fatal)"),
            Err(e) => warn!("xattr not found / failed: {e} (non-fatal)"),
        }
    }
}

async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    notify: &NotifyBus,
    toast: &str,
    version: &str,
) -> Result<()> {
    let mut response = client
        .get(url)
        .send()
        .await
        .context("send CfT download request")?
        .error_for_status()
        .context("CfT download endpoint returned error")?;
    let total = response.content_length();

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk) = response.chunk().await.context("read body chunk")? {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", dest.display()))?;
        downloaded += chunk.len() as u64;
        // Cap UI updates to ~4Hz; otherwise we drown the broadcast bus.
        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            last_emit = std::time::Instant::now();
            let body = match total {
                Some(t) if t > 0 => format!(
                    "{:.0}% / {:.1} MB",
                    (downloaded as f64 / t as f64) * 100.0,
                    t as f64 / 1_048_576.0,
                ),
                _ => format!("{:.1} MB", downloaded as f64 / 1_048_576.0),
            };
            let _ = notify.send(
                Notification::info(format!("Downloading Chrome for Testing {version}"))
                    .body(body)
                    .sticky()
                    .id(toast),
            );
        }
    }
    file.flush().await.ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_parses() {
        assert_eq!(Channel::parse("stable"), Channel::Stable);
        assert_eq!(Channel::parse("STABLE"), Channel::Stable);
        assert_eq!(Channel::parse(""), Channel::Stable);
        assert_eq!(Channel::parse("beta"), Channel::Beta);
        assert_eq!(Channel::parse("dev"), Channel::Dev);
        assert_eq!(Channel::parse("canary"), Channel::Canary);
        assert_eq!(
            Channel::parse("131.0.6778.85"),
            Channel::Literal("131.0.6778.85".into())
        );
    }

    #[test]
    fn cft_platform_returns_known_string_on_supported_targets() {
        // Just exercise the cfg branches — actual value depends on host.
        let p = cft_platform();
        if cfg!(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows"
        )) {
            assert!(p.is_some());
            let s = p.unwrap();
            assert!(["linux64", "mac-arm64", "mac-x64", "win64", "win32"].contains(&s));
        }
    }

    #[test]
    fn executable_path_per_platform() {
        let dir = PathBuf::from("/tmp/cft/131.0.6778.85");
        assert_eq!(
            executable_path(&dir, "linux64"),
            PathBuf::from("/tmp/cft/131.0.6778.85/chrome-linux64/chrome")
        );
        assert_eq!(
            executable_path(&dir, "win64"),
            PathBuf::from("/tmp/cft/131.0.6778.85/chrome-win64/chrome.exe")
        );
        let mac = executable_path(&dir, "mac-arm64");
        assert!(
            mac.ends_with("Contents/MacOS/Google Chrome for Testing"),
            "got {}",
            mac.display()
        );
    }

    #[test]
    fn version_sort_orders_numerically() {
        let mut v = vec![
            "129.0.6668.58".to_string(),
            "131.0.6778.85".into(),
            "130.0.6723.92".into(),
        ];
        v.sort_by_key(|x| std::cmp::Reverse(version_sort_key(x)));
        assert_eq!(
            v,
            vec![
                "131.0.6778.85".to_string(),
                "130.0.6723.92".into(),
                "129.0.6668.58".into(),
            ]
        );
    }

    #[test]
    fn version_sort_handles_garbage_gracefully() {
        let key_good = version_sort_key("131.0.6778.85");
        let key_garbage = version_sort_key("not-a-version");
        // garbage should sort *less* than well-formed (i64::MIN component)
        assert!(key_garbage < key_good);
    }

    #[test]
    fn channels_manifest_parses_real_shape() {
        // Real-world shape sample (truncated). Keep the test offline.
        let body = r#"{
            "timestamp": "2026-01-01",
            "channels": {
                "Stable": {
                    "channel": "Stable",
                    "version": "131.0.6778.85",
                    "revision": "1",
                    "downloads": {
                        "chrome": [
                            { "platform": "linux64", "url": "https://example.test/chrome-linux64.zip" },
                            { "platform": "mac-arm64", "url": "https://example.test/chrome-mac-arm64.zip" }
                        ]
                    }
                }
            }
        }"#;
        let m: ChannelsManifest = serde_json::from_str(body).unwrap();
        let stable = m.channels.stable.unwrap();
        assert_eq!(stable.version, "131.0.6778.85");
        assert_eq!(stable.downloads.chrome.len(), 2);
    }
}
