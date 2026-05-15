//! Download a release zip, verify SHA-256, extract just the binary,
//! atomically swap it into place via `self_replace`, then trigger a
//! restart.
//!
//! Only the running platform's binary is extracted — the bundled
//! Python+UV runtime tree stays untouched. That's fine for normal patch
//! releases (which only change Rust code); the user is reminded in the
//! UI that runtime-bump releases may require a full reinstall.

use crate::github::{build_client, extract_zip_safe};
use crate::updater::check::UpdateInfo;
use crate::updater::error::UpdateError;
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

/// Top-level entry point called by the `apply_update` IPC command.
/// On success the function does NOT return — `app.restart()` exits the
/// process. Failures map to `UpdateError` so the frontend can decide
/// between a generic toast and a "fall back to release page" hint.
pub async fn download_and_apply(app: &AppHandle, info: &UpdateInfo) -> Result<(), UpdateError> {
    // Determine the running exe path and verify its parent is writable
    // before we burn bandwidth on a download we can't apply.
    let current_exe = std::env::current_exe()
        .context("std::env::current_exe failed")
        .map_err(UpdateError::from)?;
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("current_exe has no parent"))
        .map_err(UpdateError::from)?
        .to_path_buf();
    if !is_dir_writable(&install_dir) {
        return Err(UpdateError::ReadOnlyInstall { path: install_dir });
    }

    // Stream-download the asset into a tempfile that lives next to the
    // exe so the rename in `self_replace::self_replace` stays on the
    // same filesystem.
    let tempdir = tempfile::Builder::new()
        .prefix(".akagi-update-")
        .tempdir_in(&install_dir)
        .context("create staging dir next to current exe")
        .map_err(UpdateError::from)?;
    let zip_path = tempdir.path().join("release.zip");
    let downloaded_digest = download_zip(&info.asset_url, &zip_path)
        .await
        .map_err(UpdateError::from)?;

    if let Some(expected) = info.asset_digest_sha256.as_deref() {
        if !downloaded_digest.eq_ignore_ascii_case(expected) {
            return Err(UpdateError::DigestMismatch);
        }
    }

    // Extract the whole zip into a sibling dir, then locate the binary
    // entry. The release zip wraps everything in a single
    // `akagi-<version>-<triple>/` directory, so the binary lives one
    // level down. We don't extract the runtime tree — too large and the
    // user is told to manually reinstall when the runtime version
    // changes.
    let extract_dir = tempdir.path().join("extracted");
    std::fs::create_dir(&extract_dir)
        .with_context(|| format!("mkdir {}", extract_dir.display()))
        .map_err(UpdateError::from)?;
    extract_zip_safe(&zip_path, &extract_dir)
        .context("extract release zip")
        .map_err(UpdateError::from)?;

    let binary_name = current_exe
        .file_name()
        .ok_or_else(|| anyhow!("current_exe has no file name"))
        .map_err(UpdateError::from)?
        .to_owned();
    let new_binary = find_binary(&extract_dir, &binary_name)
        .ok_or(UpdateError::NoMatchingAsset)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&new_binary, std::fs::Permissions::from_mode(0o755));
    }

    // Atomic in-place swap. On Unix this is a `rename` over the
    // currently-running ELF/Mach-O (the kernel keeps the running inode
    // alive). On Windows it's the documented copy + delete-on-close
    // dance — see the self_replace crate docs for the gory details.
    self_replace::self_replace(&new_binary)
        .context("self_replace::self_replace")
        .map_err(UpdateError::from)?;

    // Trigger restart. The call shuts the process down and re-execs the
    // (now swapped) binary, so anything after this point in the source
    // is unreachable.
    app.restart();
}

/// Returns true if `dir` looks writable. We do an explicit write-probe
/// rather than relying on `metadata().permissions().readonly()` because
/// (a) on Unix permissions don't capture mount-level read-only flags
/// (squashfs, AppImage), and (b) on Windows readonly directories are
/// not actually a thing — ACLs are richer.
fn is_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".akagi-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Stream `url` to `path`, returning the hex SHA-256 of the bytes
/// written. We hash on the fly so a multi-hundred-MB zip never has to
/// be re-read just to digest it.
async fn download_zip(url: &str, path: &Path) -> Result<String> {
    let client = build_client()?;
    let mut response = client
        .get(url)
        .send()
        .await
        .context("send download request")?
        .error_for_status()
        .context("download endpoint returned error")?;

    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("create {}", path.display()))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response.chunk().await.context("read body chunk")? {
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", path.display()))?;
    }
    file.flush().await.ok();
    Ok(hex_encode(hasher.finalize().as_slice()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Walk `dir` looking for an entry whose file name matches `binary_name`.
/// The release zip wraps everything in a single top-level
/// `akagi-<version>-<triple>/` directory so we have to descend at least
/// one level — but we keep the walk bounded to avoid scanning the whole
/// runtime tree if the layout ever changes.
fn find_binary(dir: &Path, binary_name: &std::ffi::OsStr) -> Option<PathBuf> {
    // Direct hit at the top level — handles a hypothetical future
    // layout that drops the wrapping directory.
    let direct = dir.join(binary_name);
    if direct.is_file() {
        return Some(direct);
    }
    // Otherwise descend exactly one level. The release zip's wrapping
    // dir is the only thing we expect at the root, so this is enough.
    for entry in std::fs::read_dir(dir).ok()? {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let candidate = entry.path().join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
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
    fn hex_encode_matches_known_value() {
        // SHA-256 of empty input.
        let h = Sha256::digest(b"");
        assert_eq!(
            hex_encode(h.as_slice()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn find_binary_locates_inside_wrapped_dir() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(
            &zip,
            &[
                ("akagi-3.0.12-linux-x64/akagi", b"#!/bin/sh\nexit 0\n"),
                ("akagi-3.0.12-linux-x64/README.txt", b"hello"),
                (
                    "akagi-3.0.12-linux-x64/runtime/python/keep.txt",
                    b"not the binary",
                ),
            ],
        );
        let out = TempDir::new().unwrap();
        extract_zip_safe(&zip, out.path()).unwrap();
        let found = find_binary(out.path(), std::ffi::OsStr::new("akagi"))
            .expect("should find the binary one level down");
        assert!(found.ends_with("akagi-3.0.12-linux-x64/akagi"));
    }

    #[test]
    fn find_binary_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("akagi-3.0.12-linux-x64")).unwrap();
        std::fs::write(
            tmp.path().join("akagi-3.0.12-linux-x64/README.txt"),
            b"hi",
        )
        .unwrap();
        assert!(find_binary(tmp.path(), std::ffi::OsStr::new("akagi")).is_none());
    }

    #[test]
    fn find_binary_prefers_direct_hit() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("akagi"), b"top-level").unwrap();
        std::fs::create_dir_all(tmp.path().join("subdir")).unwrap();
        std::fs::write(tmp.path().join("subdir/akagi"), b"nested").unwrap();
        let found = find_binary(tmp.path(), std::ffi::OsStr::new("akagi")).unwrap();
        assert_eq!(found, tmp.path().join("akagi"));
    }

    #[test]
    fn is_dir_writable_for_tempdir() {
        let tmp = TempDir::new().unwrap();
        assert!(is_dir_writable(tmp.path()));
    }
}
