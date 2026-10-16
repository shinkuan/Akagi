//! What a source sends the ingest endpoint: one JSON text message per
//! event, tagged by `t`.
//!
//! ```text
//! {"t":"hello","v":1,"agent":"userscript/3.7.1","origin":"https://tenhou.net","url":"https://tenhou.net/4/"}
//! {"t":"open","flow":1,"url":"wss://example.mjv.jp/"}
//! {"t":"frame","flow":1,"dir":"down","kind":"text","data":"{\"tag\":\"HELO\"}"}
//! {"t":"frame","flow":2,"dir":"up","kind":"binary","data":"AgEA..."}
//! {"t":"close","flow":1}
//! ```
//!
//! `flow` numbers the page's WebSockets, so it is only unique within one
//! connection to the endpoint. `data` is the frame verbatim for text and
//! base64 for binary. Both directions are sent: Mahjong Soul pairs each
//! response with the request that asked for it, so its parser needs the
//! client's frames too.

use crate::bridge::Direction;
use base64::Engine;
use serde::Deserialize;

/// The protocol version this build speaks. A source announcing another one
/// is turned away with a message saying so, rather than half-understood.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Envelope {
    Hello {
        v: u32,
        #[serde(default)]
        agent: String,
        /// Origin of the page the frames come from. Only consulted when the
        /// handshake carried no `Origin` header (a local program); a browser
        /// page's own header is what it is judged by.
        #[serde(default)]
        origin: String,
        #[serde(default)]
        url: String,
    },
    Open {
        flow: u32,
        url: String,
    },
    Frame {
        flow: u32,
        dir: FrameDir,
        kind: FrameKind,
        data: String,
    },
    Close {
        flow: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameDir {
    /// Page → game server.
    Up,
    /// Game server → page.
    Down,
}

impl From<FrameDir> for Direction {
    fn from(d: FrameDir) -> Self {
        match d {
            FrameDir::Up => Direction::Up,
            FrameDir::Down => Direction::Down,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Text,
    Binary,
}

/// The frame's bytes, as the bridge wants them. `None` for a binary frame
/// whose base64 does not decode.
pub fn decode_payload(kind: FrameKind, data: &str) -> Option<Vec<u8>> {
    match kind {
        FrameKind::Text => Some(data.as_bytes().to_vec()),
        FrameKind::Binary => base64::engine::general_purpose::STANDARD.decode(data).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_shape_parses() {
        let hello: Envelope = serde_json::from_str(
            r#"{"t":"hello","v":1,"agent":"userscript/0","origin":"https://tenhou.net","url":"https://tenhou.net/4/"}"#,
        )
        .unwrap();
        assert!(matches!(hello, Envelope::Hello { v: 1, .. }));

        let open: Envelope =
            serde_json::from_str(r#"{"t":"open","flow":3,"url":"wss://example.test/gateway"}"#)
                .unwrap();
        assert_eq!(
            open,
            Envelope::Open {
                flow: 3,
                url: "wss://example.test/gateway".into()
            }
        );

        let frame: Envelope = serde_json::from_str(
            r#"{"t":"frame","flow":3,"dir":"up","kind":"binary","data":"AQID"}"#,
        )
        .unwrap();
        assert_eq!(
            frame,
            Envelope::Frame {
                flow: 3,
                dir: FrameDir::Up,
                kind: FrameKind::Binary,
                data: "AQID".into()
            }
        );

        let close: Envelope = serde_json::from_str(r#"{"t":"close","flow":3}"#).unwrap();
        assert_eq!(close, Envelope::Close { flow: 3 });
    }

    /// A hello from a source that only knows the version still parses, so
    /// the endpoint can answer with a version mismatch instead of silence.
    #[test]
    fn hello_needs_only_a_version() {
        let hello: Envelope = serde_json::from_str(r#"{"t":"hello","v":2}"#).unwrap();
        assert!(matches!(hello, Envelope::Hello { v: 2, .. }));
    }

    #[test]
    fn unknown_message_types_are_rejected_not_misread() {
        assert!(serde_json::from_str::<Envelope>(r#"{"t":"rewrite","flow":1}"#).is_err());
    }

    #[test]
    fn payloads_decode_per_kind() {
        assert_eq!(
            decode_payload(FrameKind::Text, "<HELO/>").as_deref(),
            Some(b"<HELO/>".as_slice())
        );
        assert_eq!(
            decode_payload(FrameKind::Binary, "AQID").as_deref(),
            Some([1u8, 2, 3].as_slice())
        );
        assert_eq!(decode_payload(FrameKind::Binary, "not base64!"), None);
    }

    #[test]
    fn directions_map_onto_the_bridge() {
        assert!(matches!(Direction::from(FrameDir::Up), Direction::Up));
        assert!(matches!(Direction::from(FrameDir::Down), Direction::Down));
    }
}
