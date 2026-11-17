//! Inspector pipeline records — the unified data model for the
//! Logs → Inspector tab.
//!
//! One canonical struct per pipeline stage:
//!
//! - `WsFrame` — raw bytes the proxy or chromium capture saw, with the
//!   bridge's first-pass parsed view alongside.
//! - `MjaiEvent` — translated game event from `mjai_bus`.
//! - `BotReaction` — bot's response with the triggering mjai event and
//!   reaction latency, so "why did the bot do that?" is answerable from a
//!   single record.
//! - `Telemetry` — an analytics beacon the *game client* sent about
//!   itself, decoded by `crate::telemetry`. Not a pipeline stage: it is
//!   here because "what did the game report while Akagi was running?" is
//!   only answerable by putting it on the same timeline as everything
//!   else.
//!
//! Same shape on the wire (live tail over `tauri::ipc::Channel`) and on
//! disk (`<session>/inspector.jsonl`). The on-disk file is the source of
//! truth for past-session viewing; the bus/channel is the live tail.

use super::MjaiEvent;
use serde::{Deserialize, Serialize};

/// Direction of a captured WS frame relative to the proxied client.
/// Self-contained so the schema crate doesn't depend on `crate::bridge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameDirection {
    Up,
    Down,
}

/// Raw wire bytes of a WS frame, in the form Chrome / hudsucker delivered.
///
/// `Text` carries opcode-1 frames as their literal UTF-8 string (Tenhou's
/// `{tag:…}` JSON, the `<Z/>` heartbeat). `Binary` carries opcode-2 frames
/// as base64 — the JSONL line stays printable, the frontend can hex-render
/// it on demand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", content = "data", rename_all = "lowercase")]
pub enum FrameRaw {
    Text(String),
    Binary(String),
    /// Left out of the shareable log; the full frame is under `sensitive/`.
    Withheld,
}

/// Bridge's structured view of a parsed frame.
///
/// `method` is the platform-native message identifier (Majsoul method
/// name like `.lq.ActionPrototype`, Tenhou tag like `INIT`/`T0`/`AGARI`),
/// `args` is whatever the bridge already produced internally — protobuf
/// decoded to JSON for Majsoul, the JSON dict for Tenhou. Bridges that
/// can't decode a particular frame (handshake, unsupported method) return
/// `None`; the inspector then only shows raw bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedFrame {
    pub method: String,
    pub args: serde_json::Value,
}

/// Bot reaction record. Captures the triggering mjai event AND the bot's
/// response in one payload so the user can debug "why did the bot do
/// that?" without cross-referencing two files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BotReaction {
    pub bot: String,
    pub actor_id: u8,
    pub trigger: MjaiEvent,
    pub action: MjaiEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    /// React-call latency in milliseconds.
    pub reaction_ms: u64,
}

/// Which capture backend observed an event.
///
/// The two see the game from opposite sides — the MITM proxy sees what a
/// *standalone* client puts on the wire, the chromium backend sees what
/// the *web* client's page requests. Recording which one produced a row
/// is load-bearing: the same game behaves differently depending on how it
/// is being run, and the two backends also have different blind spots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureSource {
    /// `proxy::ProxyHandler` — the MITM leg.
    Mitm,
    /// `capture::chromium::cdp` — the `Network` domain.
    Chromium,
}

/// Which half of an HTTP exchange a row is.
///
/// Request and response are separate rows rather than one merged record,
/// because a request that never gets a response is exactly the case worth
/// seeing, and merging would drop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpPhase {
    Request,
    Response,
}

/// One header, in the order it appeared. Order is preserved because it is
/// a client fingerprint in its own right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// What happened to a message body.
///
/// A body we chose not to keep records *why*, rather than being silently
/// absent. A timeline that looks identical whether a body was empty or
/// merely skipped is the kind of blind spot this whole record exists to
/// remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpBody {
    /// The body as text, when we kept it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Length on the wire, before any content-encoding was undone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<usize>,
    /// Why `text` is absent. `None` means the body was captured whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

/// An optional, recognizer-supplied reading of an exchange.
///
/// This is where vendor-specific vocabulary lives — **never in the fields
/// above**. A new analytics format, a new game, or a note about our own
/// behaviour is a new annotation kind and touches nothing else: not this
/// struct, not the reader, not the UI.
///
/// `summary` is what a timeline row shows; `data` is the recognizer's
/// full structured output, deliberately untyped here so the schema does
/// not have to grow a variant per recognizer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpAnnotation {
    /// Recognizer identifier, e.g. `sls_beacon`, `akagi_bypass`.
    pub kind: String,
    pub summary: String,
    pub data: serde_json::Value,
}

/// One half of an HTTP exchange, as seen by a capture backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpExchange {
    /// Ties a response back to its request. `None` when we could not
    /// attribute it — see the backends for why (HTTP/2 multiplexing on
    /// the MITM leg; nothing on the chromium leg, which gets a real
    /// request id from CDP).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_id: Option<String>,
    pub phase: HttpPhase,
    /// Carried on both halves so a response row is readable on its own.
    pub method: String,
    pub url: String,
    pub host: String,
    /// `HTTP/1.1`, `HTTP/2.0`, … Empty when the backend does not say.
    pub version: String,
    /// Response only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub headers: Vec<HttpHeader>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<HttpBody>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<HttpAnnotation>,
}

/// One row in the inspector timeline.
///
/// Tagged on `kind` so the React side can switch on a string discriminant
/// without ever having to know the field shape of the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InspectorEntry {
    /// WebSocket frame seen by the capture backend.
    WsFrame {
        ts_ms: i64,
        direction: FrameDirection,
        /// Bridge instance identifier — one per WS connection. Lets the
        /// frontend group frames by flow when multiple flows are live.
        flow_id: String,
        size: usize,
        raw: FrameRaw,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parsed: Option<ParsedFrame>,
        /// Number of mjai events the bridge emitted from this frame —
        /// surfaced in the row so the user spots "frame parsed but
        /// produced 0 events" at a glance.
        emitted: usize,
    },
    /// MJAI event observed on `mjai_bus`.
    MjaiEvent { ts_ms: i64, event: MjaiEvent },
    /// Bot reaction captured at the bot manager's response site.
    BotReaction {
        ts_ms: i64,
        #[serde(flatten)]
        reaction: BotReaction,
    },
    /// One half of an HTTP exchange the capture backend saw. Generic by
    /// construction: anything a recognizer knows how to read — analytics
    /// beacons included — arrives here with an annotation attached.
    Http {
        ts_ms: i64,
        source: CaptureSource,
        #[serde(flatten)]
        exchange: HttpExchange,
    },
}

/// Stand-in for content the shareable logs leave out.
pub const WITHHELD: &str = "<withheld>";

/// Annotation kinds starting with this are Akagi's notes about its own
/// handling (`akagi_bypass`, `akagi_unpaired`, …), not a reading of content.
pub const AKAGI_NOTE_PREFIX: &str = "akagi_";

/// Header names whose values are credentials by definition.
const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
];

impl InspectorEntry {
    /// This entry as the shareable log may hold it: a frame keeps only its
    /// direction, size and method, and an HTTP exchange is dropped.
    pub fn withheld(&self) -> Option<InspectorEntry> {
        match self {
            InspectorEntry::WsFrame {
                ts_ms,
                direction,
                flow_id,
                size,
                parsed,
                emitted,
                ..
            } => Some(InspectorEntry::WsFrame {
                ts_ms: *ts_ms,
                direction: *direction,
                flow_id: flow_id.clone(),
                size: *size,
                raw: FrameRaw::Withheld,
                parsed: parsed.as_ref().map(|p| ParsedFrame {
                    method: p.method.clone(),
                    args: serde_json::Value::Null,
                }),
                emitted: *emitted,
            }),
            InspectorEntry::Http { .. } => None,
            other => Some(other.clone()),
        }
    }

    /// This entry as the shareable log may hold it. An exchange a content
    /// recognizer annotated keeps its content minus credential header values.
    /// One carrying only Akagi's own notes (`akagi_*`) says nothing about what
    /// is inside it, so its URL query, header values and body are withheld.
    pub fn redacted_http(&self) -> InspectorEntry {
        let mut entry = self.clone();
        if let InspectorEntry::Http { exchange, .. } = &mut entry {
            let recognized = exchange
                .annotations
                .iter()
                .any(|a| !a.kind.starts_with(AKAGI_NOTE_PREFIX));
            for h in &mut exchange.headers {
                if !recognized
                    || CREDENTIAL_HEADERS
                        .iter()
                        .any(|c| h.name.eq_ignore_ascii_case(c))
                {
                    h.value = WITHHELD.to_string();
                }
            }
            if !recognized {
                exchange.url = crate::util::redact_query(&exchange.url).into_owned();
                if let Some(body) = &mut exchange.body {
                    if body.text.take().is_some() {
                        body.skipped = Some(WITHHELD.to_string());
                    }
                }
            }
        }
        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_frame_text_round_trips() {
        let entry = InspectorEntry::WsFrame {
            ts_ms: 1_700_000_000_000,
            direction: FrameDirection::Down,
            flow_id: "tenhou:000001".into(),
            size: 38,
            raw: FrameRaw::Text(r#"{"tag":"INIT","seed":"1,0,0,2,5,134"}"#.into()),
            parsed: Some(ParsedFrame {
                method: "INIT".into(),
                args: serde_json::json!({"seed":"1,0,0,2,5,134"}),
            }),
            emitted: 1,
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(j.contains(r#""kind":"ws_frame""#));
        assert!(j.contains(r#""direction":"down""#));
        assert!(j.contains(r#""format":"text""#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn ws_frame_binary_round_trips() {
        let entry = InspectorEntry::WsFrame {
            ts_ms: 1,
            direction: FrameDirection::Up,
            flow_id: "majsoul:000001".into(),
            size: 5,
            raw: FrameRaw::Binary("AAECA2g=".into()),
            parsed: None,
            emitted: 0,
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(j.contains(r#""format":"binary""#));
        // `parsed: None` is skipped, not emitted as `null`.
        assert!(!j.contains(r#""parsed""#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn mjai_event_round_trips() {
        let entry = InspectorEntry::MjaiEvent {
            ts_ms: 5,
            event: MjaiEvent::Tsumo {
                actor: 0,
                pai: "5m".into(),
            },
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(j.contains(r#""kind":"mjai_event""#));
        assert!(j.contains(r#""type":"tsumo""#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn bot_reaction_round_trips() {
        let entry = InspectorEntry::BotReaction {
            ts_ms: 7,
            reaction: BotReaction {
                bot: "mortal".into(),
                actor_id: 2,
                trigger: MjaiEvent::Tsumo {
                    actor: 2,
                    pai: "5m".into(),
                },
                action: MjaiEvent::Dahai {
                    actor: 2,
                    pai: "W".into(),
                    tsumogiri: false,
                },
                meta: Some(serde_json::json!({"q": [0.1, 0.2]})),
                reaction_ms: 44,
            },
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(j.contains(r#""kind":"bot_reaction""#));
        assert!(j.contains(r#""bot":"mortal""#));
        assert!(j.contains(r#""reaction_ms":44"#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn http_request_round_trips() {
        let entry = InspectorEntry::Http {
            ts_ms: 9,
            source: CaptureSource::Mitm,
            exchange: HttpExchange {
                exchange_id: Some("1".into()),
                phase: HttpPhase::Request,
                method: "GET".into(),
                url: "https://example.com/logstores/client/track?x=1".into(),
                host: "example.com".into(),
                version: "HTTP/1.1".into(),
                status: None,
                headers: vec![HttpHeader {
                    name: "user-agent".into(),
                    value: "Example/1.0".into(),
                }],
                body: None,
                annotations: vec![HttpAnnotation {
                    kind: "sls_beacon".into(),
                    summary: "client/login_stats".into(),
                    data: serde_json::json!({"logstore": "client"}),
                }],
            },
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(j.contains(r#""kind":"http""#));
        assert!(j.contains(r#""source":"mitm""#));
        // `exchange` is flattened, so its fields sit beside `kind`/`ts_ms`.
        assert!(j.contains(r#""phase":"request""#));
        // Vendor vocabulary lives inside the annotation, never in the
        // exchange itself — that separation is the point of the design.
        assert!(j.contains(r#""kind":"sls_beacon""#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn http_response_round_trips_with_skipped_body() {
        let entry = InspectorEntry::Http {
            ts_ms: 11,
            source: CaptureSource::Chromium,
            exchange: HttpExchange {
                exchange_id: Some("cdp-42".into()),
                phase: HttpPhase::Response,
                method: "GET".into(),
                url: "https://example.com/bundle.data".into(),
                host: "example.com".into(),
                version: String::new(),
                status: Some(200),
                headers: Vec::new(),
                body: Some(HttpBody {
                    text: None,
                    bytes: Some(9_000_000),
                    skipped: Some("9000000 bytes exceeds the 262144-byte cap".into()),
                }),
                annotations: Vec::new(),
            },
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(j.contains(r#""status":200"#));
        // A skipped body says so; it is not silently absent.
        assert!(j.contains(r#""skipped""#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }

    /// Absent optionals must not serialize as `null` — an unpaired
    /// response and an empty annotation list should cost nothing on disk.
    #[test]
    fn http_omits_absent_optionals() {
        let entry = InspectorEntry::Http {
            ts_ms: 1,
            source: CaptureSource::Mitm,
            exchange: HttpExchange {
                exchange_id: None,
                phase: HttpPhase::Response,
                method: "GET".into(),
                url: "https://example.com/".into(),
                host: "example.com".into(),
                version: "HTTP/2.0".into(),
                status: Some(204),
                headers: Vec::new(),
                body: None,
                annotations: Vec::new(),
            },
        };
        let j = serde_json::to_string(&entry).unwrap();
        assert!(!j.contains(r#""exchange_id""#));
        assert!(!j.contains(r#""annotations""#));
        assert!(!j.contains(r#""body""#));
        let back: InspectorEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, entry);
    }
}
