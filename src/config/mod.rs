mod autoplay;
mod bot;
mod capture;
mod general;
mod logging;
mod platform;
mod proxy;

pub use autoplay::{AutoplayConfig, MajsoulAutoplayConfig};
pub use bot::BotConfig;
pub use capture::{CaptureConfig, CaptureMode, ChromiumConfig};
pub use general::GeneralConfig;
pub use logging::LoggingConfig;
pub use platform::{Platform, PlatformConfig};
pub use proxy::ProxyConfig;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub logging: LoggingConfig,
    pub platform: PlatformConfig,
    pub proxy: ProxyConfig,
    pub bot: BotConfig,
    pub capture: CaptureConfig,
    pub autoplay: AutoplayConfig,
}

enum ResolvedPath {
    Existing(PathBuf),
    Missing(PathBuf),
}

fn resolve_config_path(cli_path: Option<&Path>) -> ResolvedPath {
    resolve_config_path_inner(
        cli_path,
        crate::util::user_config_root(),
        crate::util::is_appimage(),
    )
}

fn resolve_config_path_inner(
    cli_path: Option<&Path>,
    user_cfg_root: Option<PathBuf>,
    appimage: bool,
) -> ResolvedPath {
    if let Some(p) = cli_path {
        if p.exists() {
            return ResolvedPath::Existing(p.to_path_buf());
        }
        return ResolvedPath::Missing(p.to_path_buf());
    }

    // Existing-file search: prefer user config dir, then exe-dir, then cwd.
    if let Some(user_cfg) = &user_cfg_root {
        let candidate = user_cfg.join("config.toml");
        if candidate.exists() {
            return ResolvedPath::Existing(candidate);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join("configs").join("config.toml");
            if candidate.exists() {
                return ResolvedPath::Existing(candidate);
            }
        }
    }

    let cwd_candidate = PathBuf::from("configs.toml");
    if cwd_candidate.exists() {
        return ResolvedPath::Existing(cwd_candidate);
    }

    // No existing config. Choose a writable target. Under AppImage (or
    // whenever exe dir is read-only), write to the user config dir.
    if appimage {
        if let Some(user_cfg) = user_cfg_root {
            return ResolvedPath::Missing(user_cfg.join("config.toml"));
        }
    }

    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("configs").join("config.toml")))
        .unwrap_or(cwd_candidate);
    ResolvedPath::Missing(target)
}

fn write_default_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let defaults = AppConfig::default();
    let body = toml::to_string_pretty(&defaults).map_err(std::io::Error::other)?;
    std::fs::write(path, body)
}

/// Load and parse the config. Returns the parsed `AppConfig` and the
/// path it was loaded from (so callers can persist updates back to the
/// same file via `commands::update_config`).
///
/// On any failure path the in-memory default is returned, but the path
/// returned is the one we *would* have written to — keeping `update_config`
/// from silently writing to an unexpected location.
pub fn load_config(cli_path: Option<&Path>) -> (AppConfig, PathBuf) {
    let path = match resolve_config_path(cli_path) {
        ResolvedPath::Existing(p) => p,
        ResolvedPath::Missing(target) => {
            eprintln!(
                "No config file found, writing defaults to: {}",
                target.display()
            );
            match write_default_config(&target) {
                Ok(()) => target,
                Err(e) => {
                    // Read-only fs (AppImage on hosts that don't set $APPIMAGE,
                    // or system installs in /usr): retry under user config dir.
                    if let Some(user_cfg) = crate::util::user_config_root() {
                        let fallback = user_cfg.join("config.toml");
                        if fallback != target {
                            eprintln!(
                                "Write to {} failed: {e}. Retrying at {}",
                                target.display(),
                                fallback.display()
                            );
                            match write_default_config(&fallback) {
                                Ok(()) => fallback,
                                Err(e2) => {
                                    eprintln!(
                                        "Failed to write default config: {e2}, using in-memory defaults"
                                    );
                                    return (AppConfig::default(), fallback);
                                }
                            }
                        } else {
                            eprintln!(
                                "Failed to write default config: {e}, using in-memory defaults"
                            );
                            return (AppConfig::default(), target);
                        }
                    } else {
                        eprintln!("Failed to write default config: {e}, using in-memory defaults");
                        return (AppConfig::default(), target);
                    }
                }
            }
        }
    };

    eprintln!("Loading config from: {}", path.display());

    let mut cfg = match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<AppConfig>(&content) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to parse config: {e}, using defaults");
                AppConfig::default()
            }
        },
        Err(e) => {
            eprintln!("Failed to read config: {e}, using defaults");
            AppConfig::default()
        }
    };
    // Migrate legacy `[bot] active = "..."` into `active_4p` once.
    cfg.bot.migrate_legacy_active();
    // Pre-existing configs (created before the first-run wizard landed)
    // shouldn't be hijacked into the wizard. Detect by presence of any
    // non-default field that the user must have written deliberately.
    migrate_first_run_marker(&mut cfg, &path);
    (cfg, path)
}

/// Existing users upgrading to a build that introduces the wizard get
/// `first_run_completed = true` automatically — they've already run the
/// app at least once and don't need onboarding. Detected by the config
/// file existing on disk *and* not being a freshly-written defaults file.
///
/// A defaults file written by `write_default_config` contains the full
/// serialised AppConfig; we treat any config file that lacks the new
/// `general.first_run_completed` key as legacy (serde fills the default
/// `false` on parse, so we flip it to `true` after parsing).
fn migrate_first_run_marker(cfg: &mut AppConfig, path: &Path) {
    if cfg.general.first_run_completed {
        return;
    }
    let Ok(body) = std::fs::read_to_string(path) else {
        return;
    };
    // If the file lacks the explicit key, it's a legacy file: respect the
    // user's prior config (whatever is on disk works for them already)
    // and skip the wizard. Fresh defaults files written by us *do* contain
    // the explicit `first_run_completed = false` line, so they still trigger
    // the wizard.
    if !body.contains("first_run_completed") {
        cfg.general.first_run_completed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        d.push(format!("akagi-cfg-{tag}-{pid}-{nanos}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_defaults_when_cli_path_missing() {
        let dir = temp_dir("cli-missing");
        let target = dir.join("nested").join("config.toml");
        assert!(!target.exists());

        let (cfg, path) = load_config(Some(&target));

        assert!(target.exists(), "default config file should be created");
        assert_eq!(path, target);
        let body = std::fs::read_to_string(&target).unwrap();
        let round_trip: AppConfig = toml::from_str(&body).unwrap();
        assert_eq!(round_trip.general.language, cfg.general.language);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reuses_existing_cli_path() {
        let dir = temp_dir("cli-existing");
        let target = dir.join("config.toml");
        std::fs::write(&target, "[general]\nlanguage = \"jp\"\n").unwrap();

        let (cfg, path) = load_config(Some(&target));
        assert_eq!(cfg.general.language, "jp");
        assert_eq!(path, target);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression: under AppImage the exe dir is read-only squashfs. The
    /// missing-config target must point at the user config dir instead of
    /// `<exe_dir>/configs/config.toml`, otherwise the default-write fails
    /// with `Read-only file system (os error 30)`.
    #[test]
    fn appimage_routes_missing_target_to_user_config_dir() {
        let user_cfg = temp_dir("appimage-user-cfg");
        let resolved = resolve_config_path_inner(None, Some(user_cfg.clone()), true);
        match resolved {
            ResolvedPath::Missing(p) => {
                assert_eq!(p, user_cfg.join("config.toml"));
            }
            ResolvedPath::Existing(p) => panic!("expected Missing, got Existing({})", p.display()),
        }
        std::fs::remove_dir_all(&user_cfg).ok();
    }

    #[test]
    fn non_appimage_does_not_route_to_user_config_dir() {
        let user_cfg = temp_dir("non-appimage-user-cfg");
        let resolved = resolve_config_path_inner(None, Some(user_cfg.clone()), false);
        match resolved {
            ResolvedPath::Missing(p) => {
                assert!(
                    !p.starts_with(&user_cfg),
                    "non-appimage should not route to user cfg dir, got {}",
                    p.display()
                );
            }
            ResolvedPath::Existing(p) => panic!("expected Missing, got Existing({})", p.display()),
        }
        std::fs::remove_dir_all(&user_cfg).ok();
    }
}
