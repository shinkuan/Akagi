//! Profile directory resolution and stale-singleton-lock recovery.
//!
//! Chromium refuses to launch if `<user-data-dir>/SingletonLock` (Unix
//! symlink) or `SingletonLock` (Windows file) points at a live PID. If
//! the previous Akagi run was force-killed (SIGKILL, OOM, power loss),
//! the lock survives and we'd be unable to launch a fresh browser.
//!
//! `clear_stale_singleton` reads the lock, identifies the prior PID,
//! checks whether that process still exists, and unlinks the lock if
//! and only if it doesn't. This NEVER kills a live process — if a real
//! user-controlled Chrome happens to be running with our profile (rare
//! but possible if the user fiddled with `--user-data-dir` manually),
//! the function returns an error and the supervisor surfaces it.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Resolve the user-data-dir path for the controlled Chromium instance.
/// `configured` empty → exe-adjacent `chrome-profile/` via
/// [`crate::util::resolve_dir`], so a portable zip keeps everything
/// (config, logs, profile) in one folder. Otherwise `configured` is
/// treated as an absolute path (relative paths are not supported here
/// — there's no meaningful root for them).
pub fn resolve_profile_dir(configured: &str) -> Result<PathBuf> {
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        if !p.is_absolute() {
            return Err(anyhow!(
                "capture.chromium.user_data_dir must be absolute (got {configured:?})"
            ));
        }
        return Ok(p);
    }
    Ok(crate::util::resolve_dir(Path::new("./chrome-profile")))
}

/// Detect and clear `SingletonLock` / `SingletonSocket` / `SingletonCookie`
/// when they reference a process that no longer exists. Returns `Ok` if
/// the dir is in a launchable state afterwards (no lock, or lock cleared).
/// Returns `Err` if a live PID owns the lock — caller surfaces that to
/// the user; we MUST NOT auto-kill.
pub fn clear_stale_singleton(profile: &Path) -> Result<()> {
    if !profile.exists() {
        return Ok(()); // fresh dir, nothing to clean
    }
    clear_stale_singleton_inner(profile)
}

#[cfg(unix)]
fn clear_stale_singleton_inner(profile: &Path) -> Result<()> {
    let lock = profile.join("SingletonLock");
    let metadata = match std::fs::symlink_metadata(&lock) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(anyhow!(
                "failed to stat singleton lock {}: {e}",
                lock.display()
            ));
        }
    };
    if !metadata.file_type().is_symlink() {
        // Unexpected — not a singleton lock we recognise. Leave alone.
        debug!(
            "singleton path {} is not a symlink; leaving untouched",
            lock.display()
        );
        return Ok(());
    }
    let target = std::fs::read_link(&lock)
        .with_context(|| format!("reading singleton symlink {}", lock.display()))?;
    let target_str = target.to_string_lossy();
    // target format is "<hostname>-<pid>"
    let pid = target_str
        .rsplit_once('-')
        .and_then(|(_, n)| n.parse::<i32>().ok());
    let Some(pid) = pid else {
        warn!(
            "singleton lock target {} doesn't match expected <host>-<pid>; leaving alone",
            target_str
        );
        return Ok(());
    };
    if process_alive_unix(pid) {
        return Err(anyhow!(
            "chromium profile {} is locked by live PID {} — close that browser before retrying \
             (target={})",
            profile.display(),
            pid,
            target_str
        ));
    }
    info!(
        "removing stale chromium singleton lock {} → {} (pid {pid} gone)",
        lock.display(),
        target_str
    );
    if let Err(e) = std::fs::remove_file(&lock) {
        warn!("failed to remove {}: {e}", lock.display());
    }
    // Also remove sibling Singleton{Socket,Cookie} which may be orphaned.
    for name in ["SingletonSocket", "SingletonCookie"] {
        let p = profile.join(name);
        if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn process_alive_unix(pid: i32) -> bool {
    // Shell out to `kill -0 <pid>` — POSIX signal-0 probe. Exit 0 means
    // alive (or alive-but-unsignalable, which we treat the same: don't
    // touch the lock). Avoids pulling libc in directly. Absolute path
    // is consistent across Linux and macOS — no PATH ambiguity.
    let status = std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

#[cfg(windows)]
fn clear_stale_singleton_inner(profile: &Path) -> Result<()> {
    let lock = profile.join("SingletonLock");
    let pid = match std::fs::read_to_string(&lock) {
        Ok(s) => s.trim().parse::<u32>().ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(anyhow!(
                "failed to read singleton lock {}: {e}",
                lock.display()
            ));
        }
    };
    let alive = pid.map(process_alive_windows).unwrap_or(false);
    if alive {
        return Err(anyhow!(
            "chromium profile {} is locked by live PID {:?}",
            profile.display(),
            pid
        ));
    }
    info!(
        "removing stale chromium singleton lock {} (pid {:?} gone)",
        lock.display(),
        pid
    );
    if let Err(e) = std::fs::remove_file(&lock) {
        warn!("failed to remove {}: {e}", lock.display());
    }
    Ok(())
}

#[cfg(windows)]
fn process_alive_windows(pid: u32) -> bool {
    // Use `tasklist /FI "PID eq <pid>"` — no extra crate, returns "INFO: No tasks…"
    // line if the PID is gone. Avoids pulling in `windows-sys` for one syscall.
    let out = match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().lines().any(|line| line.contains(&pid.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_default_lands_at_chrome_profile() {
        let p = resolve_profile_dir("").unwrap();
        // ends with chrome-profile regardless of which arm of resolve_dir
        // fired (exe-adjacent on portable, user_root on AppImage).
        assert!(
            p.ends_with("chrome-profile"),
            "expected ending in chrome-profile: {}",
            p.display()
        );
    }

    #[test]
    fn resolve_explicit_absolute() {
        let abs = if cfg!(windows) {
            r"C:\tmp\custom"
        } else {
            "/tmp/custom"
        };
        let p = resolve_profile_dir(abs).unwrap();
        assert_eq!(p, PathBuf::from(abs));
    }

    #[test]
    fn resolve_relative_rejected() {
        let r = resolve_profile_dir("./relative");
        assert!(r.is_err());
    }

    #[test]
    fn clear_singleton_no_lock_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        clear_stale_singleton(dir.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn clear_singleton_unlinks_dead_pid() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("SingletonLock");
        // Use PID 0xFFFFFFFE — far past any realistic max_pid; certainly dead.
        symlink("akagi-test-host-4294967294", &lock).unwrap();
        clear_stale_singleton(dir.path()).unwrap();
        assert!(!lock.exists(), "stale lock should have been unlinked");
    }

    #[cfg(unix)]
    #[test]
    fn clear_singleton_refuses_live_pid() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("SingletonLock");
        let our_pid = std::process::id();
        symlink(format!("akagi-test-host-{our_pid}"), &lock).unwrap();
        let r = clear_stale_singleton(dir.path());
        assert!(r.is_err(), "should refuse to unlink lock owned by live pid");
        // `exists()` follows symlinks; we want to check the symlink itself.
        assert!(
            std::fs::symlink_metadata(&lock).is_ok(),
            "live-pid lock symlink must be left alone"
        );
    }
}
