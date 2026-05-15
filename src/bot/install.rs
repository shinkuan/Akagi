//! Install a Mjai bot from a GitHub release.
//!
//! Flow:
//!
//! 1. Fetch `https://api.github.com/repos/<repo>/releases/latest` (anonymous).
//! 2. Pick one asset — by glob if `asset_glob` is set, else the first `.zip`.
//! 3. Stream the asset into a tempfile under `<dest_root>/.downloads/`.
//! 4. Open the tempfile as a zip, validate every entry's path is enclosed
//!    inside the destination (no `..`, no absolute paths).
//! 5. Extract to a sibling tempdir.
//! 6. If the archive has a single top-level directory, strip it.
//! 7. Validate `bot.py` is present in the extracted layout.
//! 8. Atomic-move the tempdir into `<dest_root>/<name>/`.
//!
//! Existing `<dest_root>/<name>/` is treated as an error — frontend can
//! offer an explicit "remove and reinstall" toggle later. Authenticated
//! installs (private repos / token), tarballs, and source-tree clones are
//! out of scope for v1.

use crate::bot::manifest::Manifest;
use crate::bot::registry::BotEntry;
use crate::bot::runtime::PythonRuntime;
use crate::event_bus::NotifyBus;
use crate::github::{build_client, extract_zip_safe, fetch_latest_release, Asset};
use crate::schema::Notification;
use anyhow::{bail, Context, Result};
use globset::Glob;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const DOWNLOADS_DIR: &str = ".downloads";

/// Pointer to a GitHub release-zip install.
#[derive(Debug, Clone)]
pub struct GithubInstallSpec {
    /// `owner/name`.
    pub repo: String,
    /// Glob to pick one asset out of the release. `None` → first `.zip`.
    pub asset_glob: Option<String>,
    /// Override target subdir name. `None` → second segment of `repo`.
    pub name: Option<String>,
}

impl GithubInstallSpec {
    /// Resolve the target subdir name. Repo must already be validated.
    fn target_name(&self) -> String {
        if let Some(n) = self.name.as_deref() {
            return n.to_owned();
        }
        // `repo` is already validated to be `owner/name`; second segment
        // is the default install name.
        self.repo.split('/').nth(1).unwrap_or(&self.repo).to_owned()
    }
}

/// End-to-end install. Returns the registry entry for the freshly
/// extracted bot.
///
/// When `runtime` is `Some(_)` and the extracted bot has a `pyproject.toml`,
/// `uv sync` is run as the final step. Failures abort the install (the bot
/// dir stays in place so the user can retry via Reinstall environment without
/// re-downloading). When `runtime` is `None`, sync is skipped with a warning
/// notification — the install is considered successful.
pub async fn install_from_github_release(
    spec: GithubInstallSpec,
    dest_root: &Path,
    notify: &NotifyBus,
    runtime: Option<&PythonRuntime>,
) -> Result<BotEntry> {
    let mut spec = spec;
    spec.repo = normalize_repo(&spec.repo);
    validate_repo(&spec.repo)?;
    let target_name = spec.target_name();
    validate_target_name(&target_name)?;
    let dest_dir = dest_root.join(&target_name);
    if dest_dir.exists() {
        bail!(
            "{} already exists — remove it before reinstalling",
            dest_dir.display()
        );
    }

    let notify_id = format!("bot-install-{target_name}");
    let _ = notify.send(
        Notification::info(format!("Installing {target_name}"))
            .body(format!("Fetching release metadata for {}", spec.repo))
            .sticky()
            .id(notify_id.clone()),
    );

    let client = build_client()?;
    let release = fetch_latest_release(&client, &spec.repo).await?;

    let asset = pick_asset(&release.assets, spec.asset_glob.as_deref())?;

    let _ = notify.send(
        Notification::info(format!("Installing {target_name}"))
            .body(format!(
                "Downloading {} ({})",
                asset.name,
                release.tag_name.as_deref().unwrap_or("latest"),
            ))
            .sticky()
            .id(notify_id.clone()),
    );

    let downloads_dir = dest_root.join(DOWNLOADS_DIR);
    tokio::fs::create_dir_all(&downloads_dir)
        .await
        .with_context(|| format!("mkdir {}", downloads_dir.display()))?;

    let tempfile_path = download_asset(&client, &asset.browser_download_url, &downloads_dir)
        .await
        .with_context(|| format!("download {}", asset.browser_download_url))?;

    let _ = notify.send(
        Notification::info(format!("Installing {target_name}"))
            .body("Extracting…")
            .sticky()
            .id(notify_id.clone()),
    );

    // Extract into a sibling dir of the eventual destination so the
    // final rename stays on the same filesystem. We use a manually-named
    // dir (not TempDir) because we may rename the dir itself away — Drop
    // semantics on a renamed TempDir are awkward.
    let staging = downloads_dir.join(format!(
        "akagi-extract-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let install_result = (|| -> Result<()> {
        std::fs::create_dir(&staging).with_context(|| format!("mkdir {}", staging.display()))?;
        extract_zip_safe(&tempfile_path, &staging)
            .with_context(|| format!("extract {}", tempfile_path.display()))?;

        let resolved_root = strip_single_top_level(&staging)?;
        validate_layout(&resolved_root)?;

        std::fs::rename(&resolved_root, &dest_dir).with_context(|| {
            format!(
                "rename {} -> {}",
                resolved_root.display(),
                dest_dir.display()
            )
        })?;
        Ok(())
    })();

    // Best-effort cleanup of any leftover staging contents. If the
    // staging dir was renamed away wholesale, this is a no-op error.
    let _ = std::fs::remove_dir_all(&staging);
    let _ = tokio::fs::remove_file(&tempfile_path).await;

    install_result?;

    // Post-install: run `uv sync` so dependency failures surface here rather
    // than at game-start. Skip silently for pyproject-less bots — only the
    // explicit Reinstall-environment path treats that as an error.
    let pyproject = dest_dir.join("pyproject.toml");
    if pyproject.is_file() {
        match runtime {
            Some(rt) => {
                let _ = notify.send(
                    Notification::info(format!("Installing {target_name}"))
                        .body("Installing Python dependencies (uv sync)…")
                        .sticky()
                        .id(notify_id.clone()),
                );
                rt.ensure_synced(&dest_dir)
                    .await
                    .with_context(|| format!("uv sync after install of {target_name}"))?;
            }
            None => {
                let _ = notify.send(
                    Notification::warn(format!("{target_name} installed without sync"))
                        .body("No python3+uv runtime found on PATH; deps will be installed at game start.")
                        .id(notify_id.clone()),
                );
            }
        }
    }

    let _ = notify.send(
        Notification::success(format!("{target_name} installed"))
            .body(format!(
                "{} ({})",
                spec.repo,
                release.tag_name.as_deref().unwrap_or("latest"),
            ))
            .id(notify_id),
    );

    Ok(BotEntry {
        name: target_name.clone(),
        dir: dest_dir.clone(),
        pyproject: pyproject.is_file().then_some(pyproject),
        manifest: Manifest::load(&dest_dir).ok().flatten(),
    })
}

fn validate_repo(repo: &str) -> Result<()> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        bail!("repo {repo:?} is not in the form `owner/name`");
    }
    for part in parts {
        if !part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            bail!("repo {repo:?} has illegal characters; expected [A-Za-z0-9._-]+/[A-Za-z0-9._-]+");
        }
    }
    Ok(())
}

/// Reduce a user-supplied repo identifier to canonical `owner/name`.
/// Accepts: bare `owner/name`, `https?://[www.]github.com/owner/name`,
/// optional `.git` suffix, trailing slash, query string, hash fragment.
/// The result still goes through `validate_repo`; this function only
/// strips the parts that are unambiguously safe to drop.
pub fn normalize_repo(input: &str) -> String {
    let mut s = input.trim();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest;
    }
    if let Some(rest) = s.strip_prefix("github.com/") {
        s = rest;
    }
    if let Some(idx) = s.find(['?', '#']) {
        s = &s[..idx];
    }
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        s.to_owned()
    }
}

fn validate_target_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('.') {
        bail!("target name {name:?} is invalid");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("target name {name:?} contains path separators");
    }
    if name == "base" {
        bail!("target name {name:?} is reserved");
    }
    Ok(())
}

/// Choose one asset out of the release.
///
/// Public for unit tests so we can verify the glob and fallback rules
/// without spinning up a network mock.
pub fn pick_asset<'a>(assets: &'a [Asset], glob: Option<&str>) -> Result<&'a Asset> {
    if assets.is_empty() {
        bail!("release has no assets");
    }

    if let Some(pat) = glob {
        let matcher = Glob::new(pat)
            .with_context(|| format!("compile glob {pat:?}"))?
            .compile_matcher();
        let matches: Vec<&Asset> = assets
            .iter()
            .filter(|a| matcher.is_match(&a.name))
            .collect();
        match matches.len() {
            0 => bail!("no asset in release matches glob {pat:?}"),
            1 => Ok(matches[0]),
            n => bail!(
                "{n} assets matched glob {pat:?}; tighten the pattern (matched: {:?})",
                matches.iter().map(|a| &a.name).collect::<Vec<_>>()
            ),
        }
    } else {
        assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no .zip asset in release; pass asset_glob to pick a non-zip explicitly"
                )
            })
    }
}

async fn download_asset(
    client: &reqwest::Client,
    url: &str,
    downloads_dir: &Path,
) -> Result<PathBuf> {
    let path = downloads_dir.join(format!(
        "akagi-bot-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let mut response = client
        .get(url)
        .send()
        .await
        .context("send download request")?
        .error_for_status()
        .context("download endpoint returned error")?;

    let mut file = tokio::fs::File::create(&path)
        .await
        .with_context(|| format!("create {}", path.display()))?;
    while let Some(chunk) = response.chunk().await.context("read body chunk")? {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", path.display()))?;
    }
    file.flush().await.ok();
    Ok(path)
}

/// If `dir` contains exactly one entry and that entry is a directory,
/// return that nested directory. Otherwise return `dir` unchanged.
///
/// Real-world release zips usually wrap everything in a single top-level
/// dir like `mortal-v0.5.0/…`. Strip it so the bot's `bot.py` ends up
/// directly under `<bot>/bot.py` rather than `<bot>/mortal-v0.5.0/bot.py`.
pub fn strip_single_top_level(dir: &Path) -> Result<PathBuf> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();
    if entries.len() != 1 {
        return Ok(dir.to_path_buf());
    }
    let only = entries.remove(0);
    if only.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
        Ok(only.path())
    } else {
        Ok(dir.to_path_buf())
    }
}

/// Reject installs that don't look like bots — `bot.py` is the registry
/// contract. Pyproject is recommended but not enforced (some bots may
/// run on system python without uv).
pub fn validate_layout(bot_root: &Path) -> Result<()> {
    let bot_py = bot_root.join("bot.py");
    if !bot_py.is_file() {
        bail!("extracted archive does not contain bot.py at the top level — refusing install");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.com/{name}"),
            size: None,
            digest: None,
        }
    }

    #[test]
    fn validate_repo_accepts_owner_slash_name() {
        validate_repo("Equim-chan/Mortal").unwrap();
        validate_repo("a.b/c-d_e.f").unwrap();
    }

    #[test]
    fn validate_repo_rejects_bad_input() {
        for bad in [
            "",
            "noslash",
            "/leading",
            "trailing/",
            "a/b/c",
            "owner with space/name",
            "owner/name?query",
        ] {
            let err = validate_repo(bad).unwrap_err();
            assert!(
                err.to_string().contains("repo"),
                "expected error for {bad:?}: {err:#}"
            );
        }
    }

    #[test]
    fn validate_target_name_rejects_separators_and_reserved() {
        validate_target_name("mortal").unwrap();
        for bad in ["", ".hidden", "a/b", "a\\b", "..", "with..parent", "base"] {
            assert!(
                validate_target_name(bad).is_err(),
                "{bad:?} should be invalid"
            );
        }
    }

    #[test]
    fn target_name_defaults_to_repo_second_segment() {
        let s = GithubInstallSpec {
            repo: "owner/name-with-dashes".into(),
            asset_glob: None,
            name: None,
        };
        assert_eq!(s.target_name(), "name-with-dashes");
    }

    #[test]
    fn target_name_override_takes_precedence() {
        let s = GithubInstallSpec {
            repo: "owner/upstream".into(),
            asset_glob: None,
            name: Some("local-alias".into()),
        };
        assert_eq!(s.target_name(), "local-alias");
    }

    #[test]
    fn pick_asset_first_zip_when_no_glob() {
        let assets = vec![
            asset("checksum.txt"),
            asset("mortal-v1.zip"),
            asset("mortal-v2.zip"),
        ];
        let chosen = pick_asset(&assets, None).unwrap();
        assert_eq!(chosen.name, "mortal-v1.zip");
    }

    #[test]
    fn pick_asset_no_zip_errors_without_glob() {
        let assets = vec![asset("checksum.txt"), asset("source.tar.gz")];
        let err = pick_asset(&assets, None).unwrap_err();
        assert!(err.to_string().contains("no .zip asset"));
    }

    #[test]
    fn pick_asset_glob_single_match() {
        let assets = vec![
            asset("mortal-v1.zip"),
            asset("mortal-v1-debug.zip"),
            asset("mortal-v1.sig"),
        ];
        let chosen = pick_asset(&assets, Some("mortal-v?.zip")).unwrap();
        assert_eq!(chosen.name, "mortal-v1.zip");
    }

    #[test]
    fn pick_asset_glob_no_match_errors() {
        let assets = vec![asset("mortal-v1.zip")];
        let err = pick_asset(&assets, Some("nope-*.zip")).unwrap_err();
        assert!(err.to_string().contains("no asset"));
    }

    #[test]
    fn pick_asset_glob_multiple_matches_errors() {
        let assets = vec![asset("mortal-v1.zip"), asset("mortal-v2.zip")];
        let err = pick_asset(&assets, Some("mortal-*.zip")).unwrap_err();
        assert!(err.to_string().contains("matched glob"));
    }

    #[test]
    fn pick_asset_empty_release_errors() {
        let err = pick_asset(&[], None).unwrap_err();
        assert!(err.to_string().contains("no assets"));
    }

    #[test]
    fn strip_top_level_strips_single_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("mortal-v1/sub")).unwrap();
        std::fs::write(tmp.path().join("mortal-v1/bot.py"), b"").unwrap();

        let resolved = strip_single_top_level(tmp.path()).unwrap();
        assert_eq!(resolved, tmp.path().join("mortal-v1"));
    }

    #[test]
    fn strip_top_level_keeps_root_when_multiple_entries() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("bot.py"), b"").unwrap();
        std::fs::write(tmp.path().join("README.md"), b"").unwrap();

        let resolved = strip_single_top_level(tmp.path()).unwrap();
        assert_eq!(resolved, tmp.path());
    }

    #[test]
    fn validate_layout_requires_bot_py() {
        let tmp = TempDir::new().unwrap();
        let err = validate_layout(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bot.py"));

        std::fs::write(tmp.path().join("bot.py"), b"").unwrap();
        validate_layout(tmp.path()).unwrap();
    }
}
