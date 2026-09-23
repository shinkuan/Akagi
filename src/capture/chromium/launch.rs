//! Spawn the controlled Chromium process and read its CDP endpoint.
//!
//! Lifecycle:
//! 1. `spawn` runs the chrome binary with `--user-data-dir`, a remote debugging
//!    port, and assorted "don't be annoying" flags.
//! 2. `wait_for_devtools_endpoint` polls `DevToolsActivePort` and, when the
//!    port is known, the browser's HTTP discovery endpoint. It returns a
//!    `ws://...` URL suitable for `chromiumoxide::Browser::connect`.
//! 3. `terminate` does the staged shutdown: SIGTERM (Unix) / `taskkill`
//!    (Windows) → wait → SIGKILL.

use crate::config::ChromiumConfig;
#[cfg(windows)]
use crate::util::NoConsoleWindow;
use anyhow::{anyhow, Context, Result};
use std::net::TcpListener;
use std::path::Path;
#[cfg(windows)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::process::{Child, Command};
use tracing::{debug, warn};

const DEVTOOLS_FILE: &str = "DevToolsActivePort";
const PORT_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const HTTP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(1);
const TERM_GRACE: Duration = Duration::from_secs(5);
const KILL_GRACE: Duration = Duration::from_secs(2);

// The overlay uses this PID only to follow the visible top-level window of the
// Chromium instance that this process launched. It never opens the process,
// injects code, reads memory, or touches the page DOM.
#[cfg(windows)]
static CONTROLLED_BROWSER_PID: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
pub fn controlled_browser_pid() -> u32 {
    CONTROLLED_BROWSER_PID.load(Ordering::Relaxed)
}

pub struct SpawnedChromium {
    pub child: Child,
    pub remote_debugging_port: Option<u16>,
}

#[derive(Debug, PartialEq, Eq)]
struct RemoteDebuggingConfig {
    arg: Option<String>,
    port: Option<u16>,
}

pub fn spawn(exe: &Path, profile: &Path, cfg: &ChromiumConfig) -> Result<SpawnedChromium> {
    let remote_debugging = remote_debugging_config(&cfg.extra_args)?;
    let mut cmd = Command::new(exe);
    cmd.arg(format!("--user-data-dir={}", profile.display()));
    if let Some(arg) = &remote_debugging.arg {
        cmd.arg(arg);
    }
    cmd.arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-features=TranslateUI,InterestFeedContentSuggestions")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-component-update")
        .arg("--disable-background-networking")
        .arg("--disable-sync")
        .arg("--metrics-recording-only")
        .arg("--no-pings");
    if cfg!(target_os = "linux") {
        // Chromium occasionally crashes on Linux when /dev/shm is small (e.g. Docker).
        cmd.arg("--disable-dev-shm-usage");
    }
    for extra in &cfg.extra_args {
        cmd.arg(extra);
    }
    if !cfg.start_url.is_empty() {
        cmd.arg(&cfg.start_url);
    }
    // Stale port file from a previous run would confuse the polling read.
    let port_file = profile.join(DEVTOOLS_FILE);
    if port_file.exists() {
        let _ = std::fs::remove_file(&port_file);
    }
    // Wipe session-restore state so each launch opens exactly the
    // configured `start_url`, never the tabs the user happened to have
    // open last time. Cookies / login state under `Default/` are NOT
    // touched — the user stays logged in to Mahjong Soul.
    clear_session_state(profile);
    // Suppress "Restore tabs from crashed session?" bubble when our
    // previous run was force-killed (SIGKILL fallback marks the profile
    // as crashed; without this the user sees the bubble on every relaunch).
    suppress_crash_recovery_prompt(profile);
    cmd.kill_on_drop(true);
    let child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn chromium binary at {}", exe.display()))?;
    #[cfg(windows)]
    CONTROLLED_BROWSER_PID.store(child.id().unwrap_or(0), Ordering::Relaxed);
    debug!(
        "spawned chromium pid={:?} remote_debugging_port={}",
        child.id(),
        remote_debugging
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "auto".to_string())
    );
    Ok(SpawnedChromium {
        child,
        remote_debugging_port: remote_debugging.port,
    })
}

fn remote_debugging_config(extra_args: &[String]) -> Result<RemoteDebuggingConfig> {
    let mut user_port = None;
    for (idx, arg) in extra_args.iter().enumerate() {
        if let Some(port) = arg.strip_prefix("--remote-debugging-port=") {
            user_port = Some(parse_remote_debugging_port(port)?);
        } else if arg == "--remote-debugging-port" {
            let Some(port) = extra_args.get(idx + 1) else {
                return Err(anyhow!("--remote-debugging-port requires a value"));
            };
            user_port = Some(parse_remote_debugging_port(port)?);
        }
    }

    if let Some(port) = user_port {
        return Ok(RemoteDebuggingConfig {
            arg: None,
            port: port.nonzero(),
        });
    }

    let port = pick_remote_debugging_port()?;
    Ok(RemoteDebuggingConfig {
        arg: Some(format!("--remote-debugging-port={port}")),
        port: Some(port),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteDebuggingPort {
    Auto,
    Fixed(u16),
}

impl RemoteDebuggingPort {
    fn nonzero(self) -> Option<u16> {
        match self {
            RemoteDebuggingPort::Auto => None,
            RemoteDebuggingPort::Fixed(port) => Some(port),
        }
    }
}

fn parse_remote_debugging_port(value: &str) -> Result<RemoteDebuggingPort> {
    let port: u16 = value
        .parse()
        .with_context(|| format!("invalid --remote-debugging-port value: {value}"))?;
    if port == 0 {
        Ok(RemoteDebuggingPort::Auto)
    } else {
        Ok(RemoteDebuggingPort::Fixed(port))
    }
}

fn pick_remote_debugging_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .context("failed to choose a chromium remote debugging port")?;
    let port = listener
        .local_addr()
        .context("failed to read chromium remote debugging port")?
        .port();
    if port == 0 {
        return Err(anyhow!("operating system returned port 0"));
    }
    Ok(port)
}

/// Poll Chromium until its CDP endpoint is ready, then return the `ws://...` URL.
pub async fn wait_for_devtools_endpoint(
    profile: &Path,
    remote_debugging_port: Option<u16>,
) -> Result<String> {
    let port_file = profile.join(DEVTOOLS_FILE);
    let version_url =
        remote_debugging_port.map(|port| format!("http://127.0.0.1:{port}/json/version"));
    let client = reqwest::Client::builder()
        .timeout(HTTP_DISCOVERY_TIMEOUT)
        .build()
        .context("creating chromium devtools HTTP client")?;
    let start = std::time::Instant::now();
    loop {
        if let Ok(body) = std::fs::read_to_string(&port_file) {
            if let Some(endpoint) = parse_devtools_port(&body) {
                return Ok(endpoint);
            }
        }
        if let Some(version_url) = &version_url {
            if let Ok(endpoint) = fetch_devtools_endpoint(&client, version_url).await {
                return Ok(endpoint);
            }
        }
        if start.elapsed() > PORT_WAIT_TIMEOUT {
            let wait_target = if let Some(version_url) = &version_url {
                format!("{} or {}", port_file.display(), version_url)
            } else {
                port_file.display().to_string()
            };
            return Err(anyhow!(
                "timed out after {:?} waiting for {}",
                PORT_WAIT_TIMEOUT,
                wait_target
            ));
        }
        tokio::time::sleep(PORT_POLL_INTERVAL).await;
    }
}

/// Port of a `ws://127.0.0.1:PORT/devtools/...` endpoint URL.
pub fn endpoint_port(ws_url: &str) -> Option<u16> {
    let rest = ws_url.strip_prefix("ws://")?;
    let hostport = rest.split('/').next()?;
    hostport.rsplit(':').next()?.parse().ok()
}

/// True when a Chromium DevTools HTTP endpoint still answers on `port`.
///
/// Used to distinguish a real browser death from a **launcher handoff**:
/// Edge (and Chrome in some setups, e.g. de-elevation or startup boost) may
/// relaunch itself with the same arguments and exit the process we spawned
/// with code 0 — while the relaunched browser keeps serving our debugging
/// port. A few quick retries paper over the probe racing that relaunch.
pub async fn devtools_http_alive(port: u16) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(HTTP_DISCOVERY_TIMEOUT)
        .build()
    else {
        return false;
    };
    let url = format!("http://127.0.0.1:{port}/json/version");
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return true,
            _ => {}
        }
    }
    false
}

async fn fetch_devtools_endpoint(client: &reqwest::Client, url: &str) -> Result<String> {
    let json: serde_json::Value = client
        .get(url)
        .send()
        .await
        .context("requesting chromium devtools version endpoint")?
        .error_for_status()
        .context("chromium devtools version endpoint returned an error")?
        .json()
        .await
        .context("parsing chromium devtools version response")?;
    let endpoint = json
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing webSocketDebuggerUrl in chromium devtools response"))?;
    Ok(endpoint.to_string())
}

/// Parse the two-line `DevToolsActivePort` content into a `ws://...` URL.
/// Returns `None` if the file is partially written (Chrome writes the
/// port first, then the path on a separate line).
pub fn parse_devtools_port(body: &str) -> Option<String> {
    let mut lines = body.lines();
    let port_line = lines.next()?.trim();
    let path_line = lines.next()?.trim();
    if port_line.is_empty() || path_line.is_empty() {
        return None;
    }
    let port: u16 = port_line.parse().ok()?;
    Some(format!("ws://127.0.0.1:{port}{path_line}"))
}

/// Wipe the session-restore files Chromium uses to repopulate tabs on
/// the next launch. Best-effort: missing files are fine, errors logged
/// at debug level. Targets the `Default/` profile only (which is what
/// our isolated `--user-data-dir` always uses).
fn clear_session_state(profile: &Path) {
    let default_dir = profile.join("Default");
    // Files
    for name in [
        "Current Session",
        "Current Tabs",
        "Last Session",
        "Last Tabs",
    ] {
        let p = default_dir.join(name);
        if p.exists() {
            if let Err(e) = std::fs::remove_file(&p) {
                debug!("clear session: remove_file {}: {e}", p.display());
            }
        }
    }
    // Directories (newer Chromium stores per-window session protos here).
    for name in ["Sessions", "Tabs"] {
        let p = default_dir.join(name);
        if p.exists() {
            if let Err(e) = std::fs::remove_dir_all(&p) {
                debug!("clear session: remove_dir_all {}: {e}", p.display());
            }
        }
    }
}

/// If the previous run was force-killed (`exit_type == "Crashed"`),
/// Chromium shows a "Restore tabs from previous session?" bubble. We
/// already wiped the session files; flip `exit_type` back to `"Normal"`
/// so the bubble doesn't show up either. Best-effort JSON patch.
fn suppress_crash_recovery_prompt(profile: &Path) {
    let prefs_path = profile.join("Default").join("Preferences");
    let Ok(body) = std::fs::read_to_string(&prefs_path) else {
        return; // first launch — no Preferences yet, nothing to do
    };
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&body) else {
        debug!("Preferences JSON parse failed — leaving as-is");
        return;
    };
    let Some(profile_obj) = json.get_mut("profile").and_then(|v| v.as_object_mut()) else {
        return;
    };
    let needs_write = profile_obj.get("exit_type").and_then(|v| v.as_str()) != Some("Normal")
        || profile_obj.get("exited_cleanly").and_then(|v| v.as_bool()) != Some(true);
    if !needs_write {
        return;
    }
    profile_obj.insert(
        "exit_type".into(),
        serde_json::Value::String("Normal".into()),
    );
    profile_obj.insert("exited_cleanly".into(), serde_json::Value::Bool(true));
    if let Ok(out) = serde_json::to_string(&json) {
        if let Err(e) = std::fs::write(&prefs_path, out) {
            debug!("write Preferences: {e}");
        }
    }
}

/// Best-effort staged shutdown: try a polite term, escalate to kill if
/// the child doesn't exit. Always returns; never panics.
pub async fn terminate(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-TERM", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .no_console_window()
                .status();
        }
    }
    if tokio::time::timeout(TERM_GRACE, child.wait())
        .await
        .is_err()
    {
        warn!("chromium did not exit after SIGTERM, sending SIGKILL");
        let _ = child.kill().await;
        let _ = tokio::time::timeout(KILL_GRACE, child.wait()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_session_state_removes_known_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let default_dir = dir.path().join("Default");
        std::fs::create_dir_all(&default_dir).unwrap();
        for name in [
            "Current Session",
            "Current Tabs",
            "Last Session",
            "Last Tabs",
        ] {
            std::fs::write(default_dir.join(name), b"stale").unwrap();
        }
        std::fs::create_dir_all(default_dir.join("Sessions")).unwrap();
        std::fs::write(default_dir.join("Sessions/Session_1"), b"x").unwrap();
        std::fs::create_dir_all(default_dir.join("Tabs")).unwrap();

        clear_session_state(dir.path());

        for name in [
            "Current Session",
            "Current Tabs",
            "Last Session",
            "Last Tabs",
        ] {
            assert!(!default_dir.join(name).exists(), "still exists: {name}");
        }
        assert!(!default_dir.join("Sessions").exists());
        assert!(!default_dir.join("Tabs").exists());
    }

    #[test]
    fn clear_session_state_no_default_dir_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        // No Default/ — should not panic, should not create anything.
        clear_session_state(dir.path());
        assert!(!dir.path().join("Default").exists());
    }

    #[test]
    fn picks_nonzero_remote_debugging_port() {
        let port = pick_remote_debugging_port().unwrap();
        assert_ne!(port, 0);
    }

    #[test]
    fn remote_debugging_config_defaults_to_nonzero_port() {
        let cfg = remote_debugging_config(&[]).unwrap();
        let arg = cfg.arg.unwrap();
        assert!(arg.starts_with("--remote-debugging-port="), "got {arg}");
        assert_ne!(cfg.port, Some(0));
        assert!(cfg.port.is_some());
    }

    #[test]
    fn remote_debugging_config_respects_user_fixed_port() {
        let cfg = remote_debugging_config(&["--remote-debugging-port=9223".to_string()]).unwrap();
        assert_eq!(
            cfg,
            RemoteDebuggingConfig {
                arg: None,
                port: Some(9223)
            }
        );
    }

    #[test]
    fn remote_debugging_config_respects_user_split_fixed_port() {
        let cfg =
            remote_debugging_config(&["--remote-debugging-port".to_string(), "9224".to_string()])
                .unwrap();
        assert_eq!(
            cfg,
            RemoteDebuggingConfig {
                arg: None,
                port: Some(9224)
            }
        );
    }

    #[test]
    fn remote_debugging_config_respects_user_auto_port() {
        let cfg = remote_debugging_config(&["--remote-debugging-port=0".to_string()]).unwrap();
        assert_eq!(
            cfg,
            RemoteDebuggingConfig {
                arg: None,
                port: None
            }
        );
    }

    #[test]
    fn remote_debugging_config_rejects_malformed_user_port() {
        assert!(remote_debugging_config(&["--remote-debugging-port=abc".to_string()]).is_err());
        assert!(remote_debugging_config(&["--remote-debugging-port".to_string()]).is_err());
    }

    #[test]
    fn suppress_crash_recovery_prompt_flips_exit_type() {
        let dir = tempfile::tempdir().unwrap();
        let default_dir = dir.path().join("Default");
        std::fs::create_dir_all(&default_dir).unwrap();
        let prefs = default_dir.join("Preferences");
        std::fs::write(
            &prefs,
            r#"{"profile":{"exit_type":"Crashed","exited_cleanly":false,"name":"keep me"}}"#,
        )
        .unwrap();

        suppress_crash_recovery_prompt(dir.path());

        let body = std::fs::read_to_string(&prefs).unwrap();
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let profile = json.get("profile").unwrap().as_object().unwrap();
        assert_eq!(profile.get("exit_type").unwrap(), "Normal");
        assert_eq!(profile.get("exited_cleanly").unwrap(), true);
        // Unrelated keys preserved.
        assert_eq!(profile.get("name").unwrap(), "keep me");
    }

    #[test]
    fn suppress_crash_recovery_prompt_no_prefs_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        suppress_crash_recovery_prompt(dir.path()); // must not panic
    }

    #[test]
    fn endpoint_port_parses_ws_url() {
        assert_eq!(
            endpoint_port("ws://127.0.0.1:62211/devtools/browser/abc"),
            Some(62211)
        );
        assert_eq!(endpoint_port("ws://127.0.0.1:9222/"), Some(9222));
        assert_eq!(endpoint_port("http://127.0.0.1:9222/json"), None);
        assert_eq!(endpoint_port("ws://127.0.0.1/devtools"), None);
    }

    /// `devtools_http_alive` must answer true for a live HTTP listener and
    /// false for a closed port (fake local server; no real browser).
    #[tokio::test]
    async fn devtools_http_alive_detects_listener_state() {
        use std::io::{Read, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let body = r#"{"webSocketDebuggerUrl":"ws://127.0.0.1:1/devtools/browser/x"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
        assert!(
            devtools_http_alive(port).await,
            "live listener must probe true"
        );
        server.join().unwrap();

        // Closed port: bind-then-drop guarantees nothing is listening.
        let dead_port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(
            !devtools_http_alive(dead_port).await,
            "closed port must probe false"
        );
    }

    #[test]
    fn parses_devtools_port_well_formed() {
        let body = "9222\n/devtools/browser/abc-123\n";
        assert_eq!(
            parse_devtools_port(body),
            Some("ws://127.0.0.1:9222/devtools/browser/abc-123".to_string())
        );
    }

    #[test]
    fn returns_none_for_partial_write() {
        assert!(parse_devtools_port("").is_none());
        assert!(parse_devtools_port("9222").is_none()); // path line missing
        assert!(parse_devtools_port("\n").is_none());
    }

    #[test]
    fn rejects_garbage_port() {
        assert!(parse_devtools_port("not-a-port\n/path\n").is_none());
    }
}
