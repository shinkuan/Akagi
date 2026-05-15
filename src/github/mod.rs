//! Shared primitives for talking to the GitHub Releases API.
//!
//! Used by `bot::install` (download + extract a bot from a release zip)
//! and `updater` (download + extract Akagi's own binary from a release
//! zip). Both clients hit `api.github.com/repos/<repo>/releases/latest`
//! anonymously, with a `User-Agent: akagi/<version>` header.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Component, Path};

pub const GITHUB_API: &str = "https://api.github.com";
pub const USER_AGENT: &str = concat!("akagi/", env!("CARGO_PKG_VERSION"));

/// One asset entry from `releases/latest`.
///
/// `size` and `digest` are optional because the older bot-install path
/// doesn't need them, and the digest field only started appearing in
/// 2024 — old releases lack it. The updater verifies SHA-256 when
/// present and warns (but doesn't fail) when absent.
#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReleaseJson {
    #[serde(default)]
    pub tag_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// Build a reqwest client preconfigured with the Akagi `User-Agent`.
pub fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("build http client")
}

/// Anonymous fetch of the `releases/latest` endpoint for `<repo>`.
/// `repo` is `owner/name` and is assumed pre-validated by the caller.
pub async fn fetch_latest_release(client: &reqwest::Client, repo: &str) -> Result<ReleaseJson> {
    client
        .get(format!("{GITHUB_API}/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("fetch release metadata")?
        .error_for_status()
        .context("github release endpoint returned an error")?
        .json()
        .await
        .context("parse release JSON")
}

/// Extract `zip_path` into `dest_dir`. Rejects entries whose normalised
/// path escapes the destination root (`..`, absolute paths) — standard
/// "zip slip" defence. Preserves unix mode bits.
pub fn extract_zip_safe(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file =
        std::fs::File::open(zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("not a valid zip: {}", zip_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("read zip entry {i}"))?;
        let raw_name = entry.name().to_owned();

        let Some(rel) = entry.enclosed_name() else {
            bail!("zip entry {raw_name:?} has an unsafe path");
        };
        if rel.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("zip entry {raw_name:?} contains `..`");
        }
        if rel.is_absolute() {
            bail!("zip entry {raw_name:?} is absolute");
        }

        let out_path = dest_dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .with_context(|| format!("mkdir {}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let mut out = std::fs::File::create(&out_path)
            .with_context(|| format!("create {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .with_context(|| format!("write {}", out_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = File::create(path).unwrap();
        let mut z = ZipWriter::new(f);
        let opts = SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file(name.to_string(), opts).unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn extract_zip_safe_writes_files() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(
            &zip,
            &[("bot.py", b"print('hi')\n"), ("README.md", b"# hi\n")],
        );

        let out = TempDir::new().unwrap();
        extract_zip_safe(&zip, out.path()).unwrap();
        assert!(out.path().join("bot.py").is_file());
        assert!(out.path().join("README.md").is_file());
    }

    #[test]
    fn extract_zip_safe_preserves_directory_layout() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(
            &zip,
            &[
                ("mortal-v1/bot.py", b"print('hi')\n"),
                ("mortal-v1/sub/x.txt", b"x"),
            ],
        );

        let out = TempDir::new().unwrap();
        extract_zip_safe(&zip, out.path()).unwrap();
        assert!(out.path().join("mortal-v1/bot.py").is_file());
        assert!(out.path().join("mortal-v1/sub/x.txt").is_file());
    }

    #[test]
    fn extract_zip_safe_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(&zip, &[("../escape.txt", b"nope")]);

        let out = TempDir::new().unwrap();
        let err = extract_zip_safe(&zip, out.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unsafe path") || msg.contains("contains `..`"),
            "got: {msg}"
        );
    }

    #[test]
    fn asset_parses_with_optional_fields() {
        let raw = r#"{
            "name": "akagi-3.0.12-linux-x64.zip",
            "browser_download_url": "https://example.com/x.zip",
            "size": 12345,
            "digest": "sha256:abc"
        }"#;
        let asset: Asset = serde_json::from_str(raw).unwrap();
        assert_eq!(asset.size, Some(12345));
        assert_eq!(asset.digest.as_deref(), Some("sha256:abc"));
    }

    #[test]
    fn asset_parses_without_optional_fields() {
        let raw = r#"{
            "name": "mortal.zip",
            "browser_download_url": "https://example.com/m.zip"
        }"#;
        let asset: Asset = serde_json::from_str(raw).unwrap();
        assert!(asset.size.is_none());
        assert!(asset.digest.is_none());
    }
}
