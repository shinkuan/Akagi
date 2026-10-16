//! Which browser pages may feed the ingest endpoint.
//!
//! Every page a browser loads can open a WebSocket to `127.0.0.1`, and
//! whatever reaches the endpoint goes straight into the protocol bridges. A
//! browser always says which page is asking, in the handshake's `Origin`
//! header, and a page cannot forge it, so the endpoint only accepts the
//! game sites listed here.
//!
//! The same list renders the userscript's `@match` lines (see
//! [`super::userscript`]), so the pages the script runs on and the origins
//! the endpoint accepts cannot drift apart.

use crate::config::Platform;

/// A site whose pages may push frames, and the platform its frames belong
/// to.
#[derive(Debug, Clone, Copy)]
pub struct GameSite {
    pub platform: Platform,
    pub host: &'static str,
    /// Also accept any subdomain of `host`.
    pub subdomains: bool,
}

/// The web clients Akagi can capture.
///
/// - Mahjong Soul: `game.maj-soul.com` (CN), `game.mahjongsoul.com` (JP) and
///   `mahjongsoul.game.yo-star.com` (EN/KR).
/// - Tenhou: `tenhou.net` (`/3/` and `/4/`).
///
/// Riichi City has no web client, so it is not here.
pub const GAME_SITES: &[GameSite] = &[
    GameSite {
        platform: Platform::Majsoul,
        host: "maj-soul.com",
        subdomains: true,
    },
    GameSite {
        platform: Platform::Majsoul,
        host: "mahjongsoul.com",
        subdomains: true,
    },
    GameSite {
        platform: Platform::Majsoul,
        host: "mahjongsoul.game.yo-star.com",
        subdomains: false,
    },
    GameSite {
        platform: Platform::Tenhou,
        host: "tenhou.net",
        subdomains: true,
    },
];

impl GameSite {
    fn matches_host(&self, host: &str) -> bool {
        host == self.host
            || (self.subdomains
                && host
                    .strip_suffix(self.host)
                    .is_some_and(|rest| rest.ends_with('.')))
    }

    /// The userscript `@match` pattern for this site. In match-pattern
    /// syntax `*.host` covers `host` itself as well as its subdomains.
    pub fn match_pattern(&self) -> String {
        if self.subdomains {
            format!("https://*.{}/*", self.host)
        } else {
            format!("https://{}/*", self.host)
        }
    }
}

/// The platform whose web client is served from `origin`, or `None` when it
/// is not a known game site.
///
/// `origin` is the serialised form a browser sends: `scheme://host[:port]`.
/// Only `https` counts — every game site is served over it, and a page on
/// plain `http` claiming one of their hostnames is not one of them.
pub fn platform_for_origin(origin: &str) -> Option<Platform> {
    let rest = origin.strip_prefix("https://")?;
    let host = rest.split(':').next().unwrap_or(rest);
    let host = host.to_ascii_lowercase();
    GAME_SITES
        .iter()
        .find(|site| site.matches_host(&host))
        .map(|site| site.platform)
}

/// Outcome of checking a handshake's `Origin` header.
#[derive(Debug, PartialEq, Eq)]
pub enum OriginCheck {
    /// No `Origin` header: not a browser page but a local program, which
    /// already has every privilege the endpoint could grant.
    Local,
    /// A game site, and the platform it serves.
    Game(Platform),
    /// Any other page. Refused.
    Foreign,
}

pub fn check_origin(origin: Option<&str>) -> OriginCheck {
    match origin {
        None => OriginCheck::Local,
        Some(o) => match platform_for_origin(o) {
            Some(p) => OriginCheck::Game(p),
            None => OriginCheck::Foreign,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_portals_map_to_their_platform() {
        for (origin, platform) in [
            ("https://game.maj-soul.com", Platform::Majsoul),
            ("https://game.mahjongsoul.com", Platform::Majsoul),
            ("https://mahjongsoul.game.yo-star.com", Platform::Majsoul),
            ("https://tenhou.net", Platform::Tenhou),
            ("https://www.tenhou.net", Platform::Tenhou),
        ] {
            assert_eq!(platform_for_origin(origin), Some(platform), "{origin}");
        }
    }

    #[test]
    fn ports_and_case_do_not_matter() {
        assert_eq!(
            platform_for_origin("https://Game.Maj-Soul.com:443"),
            Some(Platform::Majsoul)
        );
    }

    /// A suffix match must stop at a label boundary, or `evil-tenhou.net`
    /// would pass as Tenhou.
    #[test]
    fn lookalike_hosts_are_not_game_sites() {
        for origin in [
            "https://evil-tenhou.net",
            "https://tenhou.net.example",
            "https://notmaj-soul.com",
            "https://example.com",
            "https://x.mahjongsoul.game.yo-star.com",
        ] {
            assert_eq!(platform_for_origin(origin), None, "{origin}");
        }
    }

    #[test]
    fn plain_http_is_not_a_game_site() {
        assert_eq!(platform_for_origin("http://tenhou.net"), None);
        assert_eq!(platform_for_origin("null"), None);
    }

    #[test]
    fn handshake_origins_are_classified() {
        assert_eq!(check_origin(None), OriginCheck::Local);
        assert_eq!(
            check_origin(Some("https://tenhou.net")),
            OriginCheck::Game(Platform::Tenhou)
        );
        assert_eq!(
            check_origin(Some("https://evil.example")),
            OriginCheck::Foreign
        );
    }

    #[test]
    fn match_patterns_cover_each_site() {
        let patterns: Vec<String> = GAME_SITES.iter().map(GameSite::match_pattern).collect();
        assert!(patterns.contains(&"https://*.maj-soul.com/*".to_string()));
        assert!(patterns.contains(&"https://mahjongsoul.game.yo-star.com/*".to_string()));
        assert!(patterns.contains(&"https://*.tenhou.net/*".to_string()));
    }
}
