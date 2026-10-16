//! The userscript Akagi hands out, rendered for this install.
//!
//! The template (`akagi.user.js`) is valid JavaScript as committed — every
//! placeholder sits in a string literal or a comment — so it can be loaded
//! and exercised by the frontend's test runner without rendering. Rendering
//! fills in the endpoint the script reports to, the game sites it runs on
//! (from [`super::origin::GAME_SITES`], the same list the endpoint accepts)
//! and Akagi's version, so a userscript manager offers an update whenever
//! Akagi changes.

use super::origin::GAME_SITES;
use std::net::SocketAddr;

const TEMPLATE: &str = include_str!("akagi.user.js");
const ENDPOINT: &str = "__AKAGI_ENDPOINT__";
const VERSION: &str = "__AKAGI_VERSION__";
const MATCH_LINE: &str = "// @match        __AKAGI_MATCH__";

/// Path the ingest server accepts sources on.
pub const INGEST_PATH: &str = "/ingest";
/// Path the ingest server serves the userscript from. Ending in `.user.js`
/// is what makes a userscript manager offer to install it.
pub const USERSCRIPT_PATH: &str = "/akagi.user.js";
/// Path of the install page ([`super::install_page`]), which is what the
/// UI's Install button opens.
pub const INSTALL_PATH: &str = "/";

/// The userscript for an endpoint listening on `addr`.
pub fn render(addr: SocketAddr) -> String {
    let matches = GAME_SITES
        .iter()
        .map(|site| format!("// @match        {}", site.match_pattern()))
        .collect::<Vec<_>>()
        .join("\n");
    TEMPLATE
        .replace(MATCH_LINE, &matches)
        .replace(ENDPOINT, &format!("ws://{addr}{INGEST_PATH}"))
        .replace(VERSION, env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> SocketAddr {
        "127.0.0.1:23411".parse().unwrap()
    }

    #[test]
    fn every_placeholder_is_filled() {
        let script = render(addr());
        assert!(
            !script.contains("__AKAGI_"),
            "unfilled placeholder:\n{script}"
        );
        assert!(script.contains("'ws://127.0.0.1:23411/ingest'"));
        assert!(script.contains(&format!("@version      {}", env!("CARGO_PKG_VERSION"))));
    }

    /// The pages the script runs on are exactly the origins the endpoint
    /// accepts.
    #[test]
    fn match_lines_follow_the_game_site_list() {
        let script = render(addr());
        let matches: Vec<&str> = script
            .lines()
            .filter_map(|l| l.strip_prefix("// @match"))
            .map(str::trim)
            .collect();
        let expected: Vec<String> = GAME_SITES.iter().map(|s| s.match_pattern()).collect();
        assert_eq!(matches, expected);
    }

    #[test]
    fn ipv6_loopback_endpoints_are_bracketed() {
        let script = render("[::1]:23411".parse().unwrap());
        assert!(script.contains("'ws://[::1]:23411/ingest'"));
    }

    #[test]
    fn the_header_is_a_userscript_header() {
        let script = render(addr());
        assert!(script.starts_with("// ==UserScript=="));
        assert!(script.contains("// @run-at       document-start"));
        assert!(script.contains("// @grant        none"));
    }
}
