//! Python interpreter + `uv` locator and per-bot venv sync.
//!
//! Two modes:
//!
//! - **Bundled**: `runtime/python/<triple>/...` and `runtime/uv/<triple>/uv`
//!   ship next to the binary in the portable zip distribution. The locator
//!   checks the exe-adjacent layout first; the Tauri-managed
//!   `app.path().resource_dir()` is checked as a secondary fallback so
//!   `cargo run` from a checkout (and any future Tauri-bundled target) keeps
//!   working. Zero Python install required for end users.
//! - **System**: `python3` and `uv` are looked up on `PATH` via the `which`
//!   crate. Used during development (`cargo run` from a checkout without a
//!   populated `runtime/`) and as a graceful fallback if the bundled
//!   binaries are missing.
//!
//! Per-bot venvs live under `<bot_dir>/.akagi/venv` so they don't clash with
//! a developer's own `.venv` if they happen to keep one in the bot folder.
//! `uv sync` is run on demand and skipped via a stamp file when neither
//! `pyproject.toml` nor `uv.lock` have changed since the last successful
//! sync.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::process::Command;
use tracing::{info, warn};

const STAMP_FILE: &str = "synced.stamp";
const VENV_DIR: &str = "venv";
const AKAGI_DIR: &str = ".akagi";

/// Origin of the python + uv binaries this runtime points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// Bundled python-build-standalone + uv from the Tauri resource dir.
    Bundled,
    /// `python3` + `uv` discovered on `PATH`. Dev-mode fallback.
    System,
}

#[derive(Debug, Clone)]
pub struct PythonRuntime {
    /// Interpreter that uv uses to seed venvs (`UV_PYTHON`).
    python: PathBuf,
    /// `uv` binary.
    uv: PathBuf,
    mode: RuntimeMode,
}

impl PythonRuntime {
    /// Direct construction. Tests use this; production code uses `locate`.
    pub fn from_paths(python: PathBuf, uv: PathBuf, mode: RuntimeMode) -> Self {
        Self { python, uv, mode }
    }

    /// Locate bundled binaries first, then fall back to system PATH.
    ///
    /// Lookup order:
    /// 1. **Exe-adjacent**: `<exe_parent>/runtime/{python,uv}/<triple>/...` —
    ///    this is the portable zip layout users get from Releases.
    /// 2. **Resource dir**: the Tauri-managed `app.path().resource_dir()` —
    ///    secondary fallback so `cargo run` and any future Tauri-bundled
    ///    install (`/usr/lib/akagi/`, `.app/Contents/Resources/`) keep
    ///    working.
    /// 3. **System PATH**: `python3` and `uv` resolved via the `which` crate.
    ///
    /// Pass `None` for `resource_dir` outside Tauri (tests, CLI tools).
    pub fn locate(resource_dir: Option<&Path>) -> Result<Self> {
        if let Some(rt) = try_bundled_exe_adjacent() {
            return Ok(rt);
        }
        if let Some(rd) = resource_dir {
            if let Some(rt) = try_bundled(rd) {
                return Ok(rt);
            }
        }
        try_system().context("no bundled runtime found and neither `python3` nor `uv` is on PATH")
    }

    pub fn python(&self) -> &Path {
        &self.python
    }

    pub fn uv(&self) -> &Path {
        &self.uv
    }

    pub fn mode(&self) -> RuntimeMode {
        self.mode
    }

    /// Run `uv sync` against the bot's `pyproject.toml` if the on-disk
    /// signature has changed since the last successful sync. Idempotent.
    pub async fn ensure_synced(&self, bot_dir: &Path) -> Result<()> {
        let pyproject = bot_dir.join("pyproject.toml");
        if !pyproject.is_file() {
            bail!(
                "pyproject.toml missing in {} — every Akagi bot must declare its deps",
                bot_dir.display()
            );
        }
        let lock = bot_dir.join("uv.lock");
        let venv = bot_dir.join(AKAGI_DIR).join(VENV_DIR);
        let stamp_path = bot_dir.join(AKAGI_DIR).join(STAMP_FILE);

        let current = current_signature(&pyproject, &lock)?;
        if venv.is_dir() && stamp_matches(&stamp_path, &current).await? {
            if venv_python_alive(&venv) {
                return Ok(());
            }
            // Stamp says the deps are in sync, but `bin/python` is a
            // dangling symlink. The AppImage case: each launch creates a
            // fresh `/tmp/.mount_Akagi_<rand>/` mount, and uv bakes that
            // absolute path into `bin/python` + `pyvenv.cfg.home` at
            // sync time, so the venv built under a previous mount has
            // dead pointers on the next launch. The standalone python +
            // installed wheels are binary-identical across launches, so
            // we repoint the venv to the current python without paying
            // for a full re-sync (which would otherwise re-run on every
            // single launch under AppImage).
            match repoint_venv(&venv, &self.python).await {
                Ok(()) => {
                    info!(
                        bot = %bot_dir.display(),
                        python = %self.python.display(),
                        "repointed venv to current python (AppImage mount changed)"
                    );
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        bot = %bot_dir.display(),
                        "venv repoint failed ({e:#}); wiping for full re-sync"
                    );
                    reset_sync_state(bot_dir).await;
                }
            }
        }

        if let Some(parent) = stamp_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create {}", parent.display()))?;
        }

        let mut sync_cmd = Command::new(&self.uv);
        sync_cmd
            .arg("sync")
            .arg("--project")
            .arg(bot_dir)
            .env("UV_PYTHON", &self.python)
            .env("UV_PROJECT_ENVIRONMENT", &venv);
        scrub_python_env(&mut sync_cmd);
        let status = sync_cmd
            .status()
            .await
            .with_context(|| format!("spawn `uv` at {}", self.uv.display()))?;
        if !status.success() {
            bail!("uv sync failed in {} ({status})", bot_dir.display());
        }

        write_stamp(&stamp_path, &current).await?;
        Ok(())
    }

    /// Build a `tokio::process::Command` that runs the venv's python with
    /// the given args, with `current_dir` set to the bot directory so
    /// `bot.py` can resolve relative paths.
    pub fn command_for(&self, bot_dir: &Path, args: &[&str]) -> Command {
        let py = venv_python(&bot_dir.join(AKAGI_DIR).join(VENV_DIR));
        let mut cmd = Command::new(py);
        cmd.current_dir(bot_dir).args(args);
        scrub_python_env(&mut cmd);
        cmd
    }
}

/// Drop Python env vars that the AppImage runtime (and some AUR
/// wrappers) export for *Akagi's* host process. Inherited as-is they
/// override the bot venv's `pyvenv.cfg`, so the venv python looks for
/// its stdlib under the AppImage mount and dies with
/// `Fatal Python error: init_fs_encoding: failed to get the Python
/// codec of the filesystem encoding / No module named 'encodings'`
/// before the bot ever reads stdin — the next `react()` then surfaces
/// as `Broken pipe (os error 32)`. Bundled python-build-standalone
/// (used both for `uv sync` and for the venv it seeds) is relocatable
/// and resolves its stdlib via `sys._base_executable`, so removing
/// these is strictly safer than inheriting them.
fn scrub_python_env(cmd: &mut Command) {
    cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
}

/// Wipe the stamp file and the venv so the next `ensure_synced` runs from
/// scratch. Used by the user-triggered "Reinstall environment" path —
/// stamp-only invalidation lets `uv sync` re-run, but uv's sync is
/// incremental against an existing venv, so a corrupted venv (the actual
/// failure mode) can survive a stamp-only retry. Wiping the venv forces a
/// clean seed. Errors are swallowed — missing files are the expected case.
pub async fn reset_sync_state(bot_dir: &Path) {
    let akagi = bot_dir.join(AKAGI_DIR);
    let _ = tokio::fs::remove_file(akagi.join(STAMP_FILE)).await;
    let _ = tokio::fs::remove_dir_all(akagi.join(VENV_DIR)).await;
}

/// Look for the bundled runtime in the directory containing the running
/// executable. This is the layout shipped by the portable zip
/// distribution: `<exe_parent>/runtime/{python,uv}/<triple>/...`.
///
/// On Linux/macOS, `tauri::path::resource_dir()` does not return
/// exe-adjacent paths in a portable layout — it tries Tauri-bundled
/// install locations like `/usr/lib/akagi/` and returns `Err` or a
/// non-existent path otherwise. Checking exe-adjacent here ensures the
/// portable zip works without depending on Tauri's resource resolution.
fn try_bundled_exe_adjacent() -> Option<PythonRuntime> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    try_bundled(exe_dir)
}

fn try_bundled(resource_dir: &Path) -> Option<PythonRuntime> {
    let triple = host_triple();
    let py = resource_dir
        .join("runtime")
        .join("python")
        .join(triple)
        .join(if cfg!(windows) {
            "python.exe"
        } else {
            "bin/python3"
        });
    let uv = resource_dir
        .join("runtime")
        .join("uv")
        .join(triple)
        .join(if cfg!(windows) { "uv.exe" } else { "uv" });
    if py.is_file() && uv.is_file() {
        Some(PythonRuntime::from_paths(py, uv, RuntimeMode::Bundled))
    } else {
        None
    }
}

fn try_system() -> Result<PythonRuntime> {
    let python = which::which("python3")
        .or_else(|_| which::which("python"))
        .context("locate python3/python on PATH")?;
    let uv = which::which("uv").context("locate uv on PATH")?;
    Ok(PythonRuntime::from_paths(python, uv, RuntimeMode::System))
}

fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// True when the venv's python interpreter resolves to an existing
/// file. `metadata` follows symlinks, so a dangling symlink (the
/// AppImage mount-changed case) returns Err and we report dead.
fn venv_python_alive(venv: &Path) -> bool {
    std::fs::metadata(venv_python(venv))
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// Repoint a venv at `new_python` without re-running `uv sync`. Used
/// when the venv was sync'd under a previous AppImage mount whose
/// `/tmp/.mount_Akagi_<rand>/` path is gone. Rewrites the `bin/python`
/// symlink and the `home = …` line in `pyvenv.cfg`; everything else in
/// the venv (site-packages, .pyc) stays valid because
/// python-build-standalone is binary-identical across launches.
///
/// Unix-only — the AppImage failure mode doesn't exist on Windows
/// (resource dir is stable there).
#[cfg(unix)]
async fn repoint_venv(venv: &Path, new_python: &Path) -> Result<()> {
    let target = tokio::fs::canonicalize(new_python)
        .await
        .with_context(|| format!("canonicalize {}", new_python.display()))?;
    let new_home = target
        .parent()
        .with_context(|| format!("python {} has no parent dir", target.display()))?
        .to_path_buf();

    let py_link = venv_python(venv);
    // Use symlink_metadata so a dangling symlink is still detected and
    // removed (plain metadata would error and skip the unlink).
    if std::fs::symlink_metadata(&py_link).is_ok() {
        tokio::fs::remove_file(&py_link)
            .await
            .with_context(|| format!("remove stale {}", py_link.display()))?;
    }
    tokio::fs::symlink(&target, &py_link)
        .await
        .with_context(|| format!("symlink {} -> {}", py_link.display(), target.display()))?;

    let cfg_path = venv.join("pyvenv.cfg");
    if cfg_path.is_file() {
        let cfg = tokio::fs::read_to_string(&cfg_path)
            .await
            .with_context(|| format!("read {}", cfg_path.display()))?;
        let new_home_str = new_home.display().to_string();
        let mut rewrote = false;
        let mut out = String::with_capacity(cfg.len());
        for line in cfg.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("home") && trimmed.split_once('=').is_some() {
                out.push_str(&format!("home = {new_home_str}"));
                rewrote = true;
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        if rewrote {
            tokio::fs::write(&cfg_path, out)
                .await
                .with_context(|| format!("write {}", cfg_path.display()))?;
        }
    }

    if !venv_python_alive(venv) {
        bail!("venv python still dead after repoint");
    }
    Ok(())
}

#[cfg(not(unix))]
async fn repoint_venv(_venv: &Path, _new_python: &Path) -> Result<()> {
    bail!("venv repoint not supported on this platform")
}

/// Build target triple — used to pick the right bundled runtime.
fn host_triple() -> &'static str {
    // `cargo` doesn't expose the runtime target triple, only the build-time
    // one — which is exactly what we want here, since the bundled binary
    // matches what we compiled for.
    env!(
        "TARGET_TRIPLE",
        "TARGET_TRIPLE not set; build.rs should pass it"
    )
}

/// `mtime:size` for `pyproject.toml` plus the same for `uv.lock` if it
/// exists. Cheap to compute (no file read) and stable across reboots
/// (mtime is filesystem-persistent). Granularity is 1 s, which is fine —
/// `uv sync` writes its lockfile with the current second.
fn current_signature(pyproject: &Path, lock: &Path) -> Result<String> {
    let proj = file_meta(pyproject)?;
    let lock_part = if lock.exists() {
        let l = file_meta(lock)?;
        format!("{}:{}", l.0, l.1)
    } else {
        "0:0".into()
    };
    Ok(format!("v1|{}:{}|{}", proj.0, proj.1, lock_part))
}

fn file_meta(p: &Path) -> Result<(u64, u64)> {
    let m = std::fs::metadata(p).with_context(|| format!("stat {}", p.display()))?;
    let mtime = m
        .modified()
        .with_context(|| format!("mtime {}", p.display()))?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok((mtime, m.len()))
}

async fn stamp_matches(stamp_path: &Path, current: &str) -> Result<bool> {
    match tokio::fs::read_to_string(stamp_path).await {
        Ok(saved) => Ok(saved.trim() == current),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("read {}", stamp_path.display())),
    }
}

async fn write_stamp(stamp_path: &Path, sig: &str) -> Result<()> {
    tokio::fs::write(stamp_path, sig)
        .await
        .with_context(|| format!("write {}", stamp_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn dummy_runtime() -> PythonRuntime {
        PythonRuntime::from_paths(
            PathBuf::from("/dev/null/python"),
            PathBuf::from("/dev/null/uv"),
            RuntimeMode::System,
        )
    }

    #[tokio::test]
    async fn ensure_synced_bails_when_pyproject_missing() {
        let tmp = TempDir::new().unwrap();
        let rt = dummy_runtime();
        let err = rt.ensure_synced(tmp.path()).await.unwrap_err();
        assert!(
            err.to_string().contains("pyproject.toml missing"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn signature_changes_when_pyproject_changes() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("pyproject.toml");
        let lock = tmp.path().join("uv.lock");
        write(&py, "[project]\nname='a'\n");
        let s1 = current_signature(&py, &lock).unwrap();

        // Bump mtime by sleeping 1.1s + writing different content. mtime
        // granularity on most filesystems is 1s, so we need to clear at
        // least one whole second.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&py, "[project]\nname='b'\n");
        let s2 = current_signature(&py, &lock).unwrap();
        assert_ne!(s1, s2, "signature must change after pyproject edit");
    }

    #[test]
    fn signature_includes_lock_when_present() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("pyproject.toml");
        let lock = tmp.path().join("uv.lock");
        write(&py, "[project]\nname='a'\n");

        let no_lock = current_signature(&py, &lock).unwrap();
        write(&lock, "version = 1\n");
        let with_lock = current_signature(&py, &lock).unwrap();
        assert_ne!(no_lock, with_lock);
    }

    #[tokio::test]
    async fn stamp_round_trip() {
        let tmp = TempDir::new().unwrap();
        let stamp = tmp.path().join(AKAGI_DIR).join(STAMP_FILE);
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();

        assert!(!stamp_matches(&stamp, "v1|abc").await.unwrap());
        write_stamp(&stamp, "v1|abc").await.unwrap();
        assert!(stamp_matches(&stamp, "v1|abc").await.unwrap());
        assert!(!stamp_matches(&stamp, "v1|xyz").await.unwrap());
    }

    #[test]
    fn venv_python_path_per_platform() {
        let venv = Path::new("/foo/.akagi/venv");
        let p = venv_python(venv);
        if cfg!(windows) {
            assert!(p.ends_with("Scripts/python.exe") || p.ends_with("Scripts\\python.exe"));
        } else {
            assert!(p.ends_with("bin/python"));
        }
    }

    #[test]
    fn try_bundled_returns_none_when_runtime_dir_empty() {
        let tmp = TempDir::new().unwrap();
        assert!(try_bundled(tmp.path()).is_none());
    }

    /// Regression: portable zip relies on `try_bundled_exe_adjacent` to
    /// find `<exe_parent>/runtime/...` because Tauri's `resource_dir()`
    /// doesn't return exe-adjacent on Linux/macOS in a portable layout.
    /// In the test runner the binary lives in `target/<profile>/deps/`
    /// with no `runtime/` next to it, so this must return `None` (and
    /// must not panic on the optional chain).
    #[test]
    fn try_bundled_exe_adjacent_returns_none_when_runtime_missing() {
        assert!(try_bundled_exe_adjacent().is_none());
    }

    /// Regression: AppImage runtimes export `PYTHONHOME` / `PYTHONPATH`
    /// for Akagi's host process. If we let those leak into the bot
    /// venv's python, the venv crashes at startup with
    /// `init_fs_encoding ... No module named 'encodings'` and the next
    /// `react()` writes hit a broken pipe (manager.rs surfaces this as
    /// `bot react failed: write events to bot stdin: Broken pipe`). The
    /// `command_for` builder must explicitly remove them so the venv
    /// python falls back to its `pyvenv.cfg`-based stdlib resolution.
    #[test]
    fn command_for_strips_pythonhome_and_pythonpath() {
        use std::ffi::OsStr;
        let rt = dummy_runtime();
        let tmp = TempDir::new().unwrap();
        let cmd = rt.command_for(tmp.path(), &["bot.py"]);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.as_std().get_envs().collect();
        assert!(
            envs.iter()
                .any(|(k, v)| *k == OsStr::new("PYTHONHOME") && v.is_none()),
            "PYTHONHOME must be removed (got envs={envs:?})"
        );
        assert!(
            envs.iter()
                .any(|(k, v)| *k == OsStr::new("PYTHONPATH") && v.is_none()),
            "PYTHONPATH must be removed (got envs={envs:?})"
        );
    }

    /// Regression: under AppImage, every launch creates a new
    /// `/tmp/.mount_Akagi_<rand>/` mount, and uv bakes that absolute
    /// path into the venv at sync time. On the next launch the venv's
    /// `bin/python` symlink target is gone and `cmd.spawn()` returns
    /// ENOENT, surfacing as `spawn bot mortal: No such file or
    /// directory` from `runner.rs`. `repoint_venv` must rewrite the
    /// symlink and `pyvenv.cfg` `home =` line so the venv works again
    /// without re-running uv sync (which would otherwise re-run on
    /// every launch and cost minutes).
    #[cfg(unix)]
    #[tokio::test]
    async fn repoint_venv_rewrites_symlink_and_pyvenv_cfg() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join(AKAGI_DIR).join(VENV_DIR);
        let bin = venv.join("bin");
        std::fs::create_dir_all(&bin).unwrap();

        // Stale mount path — neither file exists. This mirrors what an
        // AppImage second-launch venv looks like on disk.
        let stale = tmp.path().join("mount_OLD/python3");
        std::os::unix::fs::symlink(&stale, bin.join("python")).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            format!(
                "home = {}\nimplementation = CPython\nversion_info = 3.12.13\n",
                stale.parent().unwrap().display()
            ),
        )
        .unwrap();
        assert!(!venv_python_alive(&venv), "stale venv must read as dead");

        // Fresh mount: real python that does exist.
        let fresh_dir = tmp.path().join("mount_NEW/bin");
        std::fs::create_dir_all(&fresh_dir).unwrap();
        let fresh_python = fresh_dir.join("python3");
        std::fs::write(&fresh_python, b"#!/bin/sh\nexit 0\n").unwrap();

        repoint_venv(&venv, &fresh_python).await.unwrap();

        assert!(
            venv_python_alive(&venv),
            "venv python must resolve after repoint"
        );
        let new_link = std::fs::read_link(bin.join("python")).unwrap();
        assert_eq!(
            new_link,
            std::fs::canonicalize(&fresh_python).unwrap(),
            "symlink must point at the canonical fresh python"
        );
        let cfg = std::fs::read_to_string(venv.join("pyvenv.cfg")).unwrap();
        assert!(
            cfg.contains(&format!("home = {}", fresh_dir.display())),
            "pyvenv.cfg `home` must be rewritten to the fresh bin dir, got:\n{cfg}"
        );
        assert!(
            !cfg.contains("mount_OLD"),
            "pyvenv.cfg must not retain the stale mount path, got:\n{cfg}"
        );
    }

    #[tokio::test]
    async fn reset_sync_state_removes_stamp_and_venv() {
        let tmp = TempDir::new().unwrap();
        let akagi = tmp.path().join(AKAGI_DIR);
        let venv = akagi.join(VENV_DIR);
        let stamp = akagi.join(STAMP_FILE);
        std::fs::create_dir_all(venv.join("bin")).unwrap();
        std::fs::write(stamp, "v1|abc").unwrap();
        std::fs::write(venv.join("bin").join("python"), "").unwrap();

        reset_sync_state(tmp.path()).await;

        assert!(!akagi.join(STAMP_FILE).exists(), "stamp should be removed");
        assert!(!akagi.join(VENV_DIR).exists(), "venv should be removed");
        // .akagi/ dir itself may stay — only the wipe targets are stamp + venv.
    }

    #[tokio::test]
    async fn reset_sync_state_is_silent_when_paths_missing() {
        let tmp = TempDir::new().unwrap();
        // No `.akagi/` dir at all — must not panic.
        reset_sync_state(tmp.path()).await;
    }
}
