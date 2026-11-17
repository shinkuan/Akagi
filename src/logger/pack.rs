//! Bundle log sessions into one zip for a bug report.
//!
//! Everything outside a session's `sensitive/` directory goes in as is. The
//! `sensitive/` part is left out unless asked for. With a password it goes
//! into an AES-256 encrypted [`SENSITIVE_ARCHIVE`] stored inside the pack, so
//! the pack itself extracts without one. A session recorded before
//! `sensitive/` existed kept frame content in every file, so all of it counts
//! as sensitive.

use super::SENSITIVE_DIR;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use tempfile::NamedTempFile;
use walkdir::WalkDir;
use zip::write::{FileOptions, SimpleFileOptions};
use zip::{AesMode, CompressionMethod, ZipWriter};

/// Prefix of the temp files a pack is built in; skipped if the destination
/// sits inside a session being packed.
const TEMP_PREFIX: &str = ".akagi-pack-";

/// The encrypted inner archive at the top of a pack. Its entries keep their
/// `<session>/...` paths, so extracting it next to the rest fills the sessions in.
pub const SENSITIVE_ARCHIVE: &str = "sensitive.zip";

/// No `Debug`: it carries the password.
#[derive(Clone, Default)]
pub struct PackOptions {
    pub include_sensitive: bool,
    /// Encrypts the sensitive part. `None` or empty stores it unencrypted.
    pub password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackSummary {
    pub path: PathBuf,
    pub sessions: usize,
    pub files: usize,
    /// Sensitive files that went in, encrypted or not.
    pub sensitive_files: usize,
    /// Sensitive files left out.
    pub withheld_files: usize,
    /// The sensitive files are in an encrypted [`SENSITIVE_ARCHIVE`].
    pub encrypted: bool,
}

/// Write `sessions` (directory names under `root`) to a zip at `dest`. Each
/// session lands under its own name inside the archive.
pub fn pack(
    root: &Path,
    sessions: &[String],
    opts: &PackOptions,
    dest: &Path,
) -> Result<PackSummary> {
    if sessions.is_empty() {
        bail!("no sessions to pack");
    }
    let password = opts
        .password
        .as_deref()
        .filter(|p| opts.include_sensitive && !p.is_empty());
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp_file = || {
        tempfile::Builder::new()
            .prefix(TEMP_PREFIX)
            .suffix(".zip")
            .tempfile_in(parent)
            .with_context(|| format!("create a temp file in {}", parent.display()))
    };
    let mut zip = ZipWriter::new(temp_file()?);
    let mut inner = match password {
        Some(_) => Some(ZipWriter::new(temp_file()?)),
        None => None,
    };
    let mut summary = PackSummary {
        path: dest.to_path_buf(),
        sessions: sessions.len(),
        files: 0,
        sensitive_files: 0,
        withheld_files: 0,
        encrypted: false,
    };

    for name in sessions {
        let dir = root.join(name);
        if !dir.is_dir() {
            bail!("log session {name:?} not found");
        }
        let legacy = !dir.join(SENSITIVE_DIR).is_dir();
        for entry in WalkDir::new(&dir).sort_by_file_name() {
            let entry = entry.with_context(|| format!("walk {}", dir.display()))?;
            if !entry.file_type().is_file()
                || entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX)
            {
                continue;
            }
            let rel = entry.path().strip_prefix(&dir)?;
            let sensitive = legacy
                || rel.components().next() == Some(Component::Normal(SENSITIVE_DIR.as_ref()));
            if sensitive && !opts.include_sensitive {
                summary.withheld_files += 1;
                continue;
            }
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .large_file(entry.metadata()?.len() >= u64::from(u32::MAX));
            let archive_name = std::iter::once(name.clone())
                .chain(rel.iter().map(|c| c.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
                .join("/");
            match (&mut inner, password) {
                (Some(inner), Some(pw)) if sensitive => add_file(
                    inner,
                    &archive_name,
                    options.with_aes_encryption(AesMode::Aes256, pw),
                    entry.path(),
                )?,
                _ => add_file(&mut zip, &archive_name, options, entry.path())?,
            }
            summary.files += 1;
            if sensitive {
                summary.sensitive_files += 1;
            }
        }
    }

    if let Some(inner) = inner.filter(|_| summary.sensitive_files > 0) {
        let mut inner = inner.finish().context("finish the sensitive zip")?;
        inner.flush()?;
        inner.seek(SeekFrom::Start(0))?;
        let len = inner.as_file().metadata()?.len();
        // Encrypted data does not compress.
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(len >= u64::from(u32::MAX));
        zip.start_file(SENSITIVE_ARCHIVE, options)?;
        std::io::copy(&mut inner, &mut zip).context("pack the sensitive zip")?;
        summary.encrypted = true;
    }

    let mut tmp = zip.finish().context("finish zip")?;
    tmp.flush()?;
    tmp.persist(dest)
        .with_context(|| format!("write {}", dest.display()))?;
    Ok(summary)
}

fn add_file(
    zip: &mut ZipWriter<NamedTempFile>,
    name: &str,
    options: FileOptions<'_, ()>,
    path: &Path,
) -> Result<()> {
    zip.start_file(name, options)?;
    let mut src = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    std::io::copy(&mut src, zip).with_context(|| format!("pack {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::TempDir;
    use zip::ZipArchive;

    const NEW: &str = "20260101-000000";
    const LEGACY: &str = "20250101-000000";

    fn write(path: PathBuf, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn sessions() -> TempDir {
        let root = TempDir::new().unwrap();
        let new = root.path().join(NEW);
        write(new.join("all.log"), "app log");
        write(new.join("inspector.jsonl"), "shareable");
        write(new.join("majsoul/000001-gateway.log"), "flow");
        write(new.join("sensitive/proxy.binlog"), "fake-token");
        write(new.join("sensitive/inspector.jsonl"), "full");
        let legacy = root.path().join(LEGACY);
        write(legacy.join("all.log"), "old app log");
        write(legacy.join("proxy.binlog"), "old-fake-token");
        root
    }

    fn names(zip: &Path) -> Vec<String> {
        let archive = ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
        let mut v: Vec<String> = archive.file_names().map(str::to_string).collect();
        v.sort();
        v
    }

    fn read(zip: &Path, name: &str, password: Option<&[u8]>) -> zip::result::ZipResult<String> {
        let mut archive = ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
        let mut file = match password {
            Some(pw) => archive.by_name_decrypt(name, pw)?,
            None => archive.by_name(name)?,
        };
        let mut s = String::new();
        file.read_to_string(&mut s).unwrap();
        Ok(s)
    }

    #[test]
    fn leaves_the_sensitive_part_out_by_default() {
        let root = sessions();
        let dest = root.path().join("out.zip");
        let all = [NEW.to_string(), LEGACY.to_string()];
        let s = pack(root.path(), &all, &PackOptions::default(), &dest).unwrap();
        assert_eq!(
            names(&dest),
            [
                "20260101-000000/all.log",
                "20260101-000000/inspector.jsonl",
                "20260101-000000/majsoul/000001-gateway.log",
            ]
        );
        assert_eq!((s.files, s.sensitive_files, s.withheld_files), (3, 0, 4));
        assert!(!s.encrypted);
    }

    /// The pack itself must extract without a password; only the inner
    /// sensitive zip asks for one.
    #[test]
    fn encrypts_the_sensitive_part_into_an_inner_zip() {
        let root = sessions();
        let dest = root.path().join("out.zip");
        let opts = PackOptions {
            include_sensitive: true,
            password: Some("pw".into()),
        };
        let s = pack(
            root.path(),
            &[NEW.to_string(), LEGACY.to_string()],
            &opts,
            &dest,
        )
        .unwrap();
        assert_eq!((s.files, s.sensitive_files, s.withheld_files), (7, 4, 0));
        assert!(s.encrypted);

        assert_eq!(
            names(&dest),
            [
                "20260101-000000/all.log",
                "20260101-000000/inspector.jsonl",
                "20260101-000000/majsoul/000001-gateway.log",
                "sensitive.zip",
            ]
        );
        let mut outer = ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        for i in 0..outer.len() {
            assert!(
                !outer.by_index(i).unwrap().encrypted(),
                "outer entry {i} is encrypted"
            );
        }
        let mut inner_bytes = Vec::new();
        outer
            .by_name(SENSITIVE_ARCHIVE)
            .unwrap()
            .read_to_end(&mut inner_bytes)
            .unwrap();
        let inner = root.path().join("inner.zip");
        std::fs::write(&inner, inner_bytes).unwrap();

        assert_eq!(
            names(&inner),
            [
                "20250101-000000/all.log",
                "20250101-000000/proxy.binlog",
                "20260101-000000/sensitive/inspector.jsonl",
                "20260101-000000/sensitive/proxy.binlog",
            ]
        );
        for name in names(&inner) {
            assert!(
                read(&inner, &name, None).is_err(),
                "{name} must need the password"
            );
        }
        assert_eq!(
            read(
                &inner,
                "20260101-000000/sensitive/proxy.binlog",
                Some(b"pw")
            )
            .unwrap(),
            "fake-token"
        );
    }

    #[test]
    fn stores_the_sensitive_part_unencrypted_without_a_password() {
        let root = sessions();
        let dest = root.path().join("out.zip");
        let opts = PackOptions {
            include_sensitive: true,
            password: Some(String::new()),
        };
        let s = pack(root.path(), &[NEW.to_string()], &opts, &dest).unwrap();
        assert!(!s.encrypted);
        assert_eq!(
            read(&dest, "20260101-000000/sensitive/proxy.binlog", None).unwrap(),
            "fake-token"
        );
    }

    #[test]
    fn rejects_a_missing_session() {
        let root = sessions();
        let dest = root.path().join("out.zip");
        let err = pack(
            root.path(),
            &["nope".into()],
            &PackOptions::default(),
            &dest,
        );
        assert!(err.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn skips_its_own_temp_file_inside_a_session() {
        let root = sessions();
        let dest = root.path().join(NEW).join("pack.zip");
        pack(
            root.path(),
            &[NEW.to_string()],
            &PackOptions::default(),
            &dest,
        )
        .unwrap();
        assert!(names(&dest).iter().all(|n| !n.contains(TEMP_PREFIX)));
    }
}
