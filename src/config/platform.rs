use serde::{Deserialize, Serialize};

use crate::schema;

/// Game platform whose traffic the proxy intercepts. Determines which
/// [`crate::bridge::Bridge`] is instantiated per WebSocket flow.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[default]
    Majsoul,
    Tenhou,
    /// Riichi City (麻雀一番街). Native client only (no web build), so it is
    /// captured exclusively through the MITM proxy — the Chromium/CDP backend
    /// does not apply. Observe-only (no autoplay).
    RiichiCity,
    /// ホロじゃん / HoloMahjong (Aiming). Native Windows client only, so MITM
    /// only. Its battle server speaks gRPC over cleartext HTTP/2, which the
    /// proxy relays and taps itself (see `proxy::h2c`). Observe-only.
    HoloMahjong,
}

impl Platform {
    /// Short name used as a subdirectory under the log session
    /// (e.g. `<session>/<subdir>/<flow>.log`).
    pub fn subdir(self) -> &'static str {
        match self {
            Platform::Majsoul => "majsoul",
            Platform::Tenhou => "tenhou",
            Platform::RiichiCity => "riichi_city",
            Platform::HoloMahjong => "holomahjong",
        }
    }
}

/// Map the bridge selector to its history-record tag. Keep the two enums
/// separate because the schema enum needs extra variants (`Mjai`, `Unknown`,
/// `RiichiCity`) that don't make sense as a runtime bridge selector.
impl From<Platform> for schema::Platform {
    fn from(p: Platform) -> Self {
        match p {
            Platform::Majsoul => schema::Platform::Majsoul,
            Platform::Tenhou => schema::Platform::Tenhou,
            Platform::RiichiCity => schema::Platform::RiichiCity,
            Platform::HoloMahjong => schema::Platform::HoloMahjong,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformConfig {
    pub kind: Platform,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_to_schema_platform_round_trip() {
        assert_eq!(
            schema::Platform::from(Platform::Majsoul),
            schema::Platform::Majsoul
        );
        assert_eq!(
            schema::Platform::from(Platform::Tenhou),
            schema::Platform::Tenhou
        );
        assert_eq!(
            schema::Platform::from(Platform::RiichiCity),
            schema::Platform::RiichiCity
        );
        assert_eq!(
            schema::Platform::from(Platform::HoloMahjong),
            schema::Platform::HoloMahjong
        );
    }
}
