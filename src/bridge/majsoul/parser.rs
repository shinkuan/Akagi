//! Majsoul liqi protocol parser.
//!
//! Wire format (5 layers):
//! ```text
//! [type byte][msg_id u16 LE?][Wrapper protobuf]
//!                             └─ name + data(bytes)
//!                                       └─ inner DynamicMessage
//!                                          └─ {name, data: base64(XOR(protobuf))}  // action only
//! ```
//!
//! Type byte: `01`=Notify, `02`=Request, `03`=Response.
//! Request/Response carry a little-endian u16 message id at offset 1..3.
//! Notify has no msg_id.
//!
//! Method name routing comes from the embedded `liqi.json`, a flat rpc-map
//! keyed by the fully-qualified method name:
//!   - 2-part (`lq.NotifyX`) → look up `lq.NotifyX` in the descriptor pool
//!   - 3-part (`lq.Service.method`) → `liqi.json[".lq.Service.method"]` yields
//!     `{req, resp}` fully-qualified type names, resolved in the pool
//!
//! Action data XOR: certain inner messages carry `{name, data}` where `data`
//! is a base64 string of XOR-encrypted protobuf for an action of type `name`.
//! See `wtf_decode` for the position-dependent XOR scheme.
//!
//! Each WS flow needs its own `LiqiParser` because Response packets carry
//! only a `msg_id` — the matching method name and response type must be
//! looked up from a per-flow `pending` map populated by Request packets.

use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, SerializeOptions};
use serde_json::Value as JsonValue;
use std::{collections::HashMap, sync::Arc, sync::LazyLock};

const LIQI_DESC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/liqi_desc.bin"));
const LIQI_JSON: &str = include_str!("liqi.json");

pub static POOL: LazyLock<DescriptorPool> =
    LazyLock::new(|| DescriptorPool::decode(LIQI_DESC).expect("failed to decode liqi_desc.bin"));

pub static ROUTES: LazyLock<JsonValue> =
    LazyLock::new(|| serde_json::from_str(LIQI_JSON).expect("failed to parse liqi.json"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Notify,
    Request,
    Response,
}

#[derive(Debug, Clone)]
pub struct ParsedMessage {
    pub msg_type: MessageType,
    /// Request/Response only. `None` for Notify.
    pub msg_id: Option<u16>,
    /// Fully-qualified liqi method name, e.g. `.lq.Lobby.oauth2Login`.
    pub method_name: Arc<str>,
    /// Inner message decoded as JSON. Action wrappers have their nested
    /// `data` field rewritten to the decoded action JSON in-place.
    pub payload: JsonValue,
}

/// Per-flow parser. Holds a `pending` map for request/response correlation.
pub struct LiqiParser {
    pending: HashMap<u16, (Arc<str>, MessageDescriptor)>,
}

impl LiqiParser {
    pub fn new() -> Self {
        // Force-init the static descriptors so first-call latency is paid up front.
        LazyLock::force(&POOL);
        LazyLock::force(&ROUTES);
        Self {
            pending: HashMap::new(),
        }
    }

    pub fn parse(&mut self, buf: &[u8]) -> Result<ParsedMessage> {
        ensure!(!buf.is_empty(), "empty frame");
        match buf[0] {
            1 => self.parse_notify(buf),
            2 => self.parse_request(buf),
            3 => self.parse_response(buf),
            t => bail!("invalid liqi message type byte: {t}"),
        }
    }

    fn parse_notify(&self, buf: &[u8]) -> Result<ParsedMessage> {
        let wrapper = decode_wrapper(&buf[1..])?;
        let method_name: Arc<str> = Arc::from(wrapper.name.as_str());
        let msg_desc = lookup_notify_type(&wrapper.name)?;
        let payload = decode_to_json(&msg_desc, &wrapper.data)?;
        let payload = maybe_decode_action(payload)?;
        Ok(ParsedMessage {
            msg_type: MessageType::Notify,
            msg_id: None,
            method_name,
            payload,
        })
    }

    fn parse_request(&mut self, buf: &[u8]) -> Result<ParsedMessage> {
        ensure!(buf.len() >= 3, "request frame too short");
        let msg_id = u16::from_le_bytes([buf[1], buf[2]]);
        let wrapper = decode_wrapper(&buf[3..])?;
        let method_name: Arc<str> = Arc::from(wrapper.name.as_str());
        let (req_desc, resp_desc) = lookup_method_types(&wrapper.name)?;
        let payload = decode_to_json(&req_desc, &wrapper.data)?;
        self.pending
            .insert(msg_id, (method_name.clone(), resp_desc));
        Ok(ParsedMessage {
            msg_type: MessageType::Request,
            msg_id: Some(msg_id),
            method_name,
            payload,
        })
    }

    fn parse_response(&mut self, buf: &[u8]) -> Result<ParsedMessage> {
        ensure!(buf.len() >= 3, "response frame too short");
        let msg_id = u16::from_le_bytes([buf[1], buf[2]]);
        let wrapper = decode_wrapper(&buf[3..])?;
        ensure!(
            wrapper.name.is_empty(),
            "response wrapper should have empty name, got {:?}",
            wrapper.name
        );
        let (method_name, resp_desc) = self
            .pending
            .remove(&msg_id)
            .with_context(|| format!("no pending request for response msg_id={msg_id}"))?;
        let payload = decode_to_json(&resp_desc, &wrapper.data)?;
        Ok(ParsedMessage {
            msg_type: MessageType::Response,
            msg_id: Some(msg_id),
            method_name,
            payload,
        })
    }
}

impl Default for LiqiParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper { string name = 1; bytes data = 2; } — decoded inline to avoid
/// needing the prost-generated module.
struct Wrapper {
    name: String,
    data: Vec<u8>,
}

fn decode_wrapper(buf: &[u8]) -> Result<Wrapper> {
    #[derive(::prost::Message)]
    struct Raw {
        #[prost(string, tag = "1")]
        name: ::prost::alloc::string::String,
        #[prost(bytes = "vec", tag = "2")]
        data: ::prost::alloc::vec::Vec<u8>,
    }
    let raw = Raw::decode(buf).context("failed to decode Wrapper")?;
    Ok(Wrapper {
        name: raw.name,
        data: raw.data,
    })
}

fn lookup_notify_type(name: &str) -> Result<MessageDescriptor> {
    let parts: Vec<&str> = name.split('.').filter(|s| !s.is_empty()).collect();
    ensure!(
        parts.len() == 2,
        "expected 2-part notify name (lq.X), got {name:?}"
    );
    let fqn = format!("{}.{}", parts[0], parts[1]);
    POOL.get_message_by_name(&fqn)
        .with_context(|| format!("unknown notify message: {fqn}"))
}

fn lookup_method_types(name: &str) -> Result<(MessageDescriptor, MessageDescriptor)> {
    let parts: Vec<&str> = name.split('.').filter(|s| !s.is_empty()).collect();
    ensure!(
        parts.len() == 3,
        "expected 3-part method name (lq.Service.method), got {name:?}"
    );
    let key = format!(".{}.{}.{}", parts[0], parts[1], parts[2]);
    let entry = &ROUTES[key.as_str()];
    let req_ref = entry["req"]
        .as_str()
        .with_context(|| format!("no route/req for {name}"))?;
    let resp_ref = entry["resp"]
        .as_str()
        .with_context(|| format!("no route/resp for {name}"))?;
    // rpc-map values are fully-qualified with a leading dot (`.lq.ReqX`);
    // the descriptor pool is keyed without it (`lq.ReqX`).
    let req_desc = POOL
        .get_message_by_name(req_ref.trim_start_matches('.'))
        .with_context(|| format!("unknown request type: {req_ref}"))?;
    let resp_desc = POOL
        .get_message_by_name(resp_ref.trim_start_matches('.'))
        .with_context(|| format!("unknown response type: {resp_ref}"))?;
    Ok((req_desc, resp_desc))
}

fn decode_to_json(desc: &MessageDescriptor, bytes: &[u8]) -> Result<JsonValue> {
    let dyn_msg = DynamicMessage::decode(desc.clone(), bytes)
        .with_context(|| format!("failed to decode {}", desc.full_name()))?;
    dyn_to_json(&dyn_msg)
}

fn dyn_to_json(msg: &DynamicMessage) -> Result<JsonValue> {
    // `skip_default_fields(false)` makes proto defaults explicit in the
    // serialized JSON: `moqie: false`, `seat: 0` (dealer), `tile: ""` for
    // other players' draws — all of which the wire format omits. Mjai
    // mapping in `mod.rs` relies on these being present (or at least
    // unambiguously inferrable from absence + defaults).
    let opts = SerializeOptions::new()
        .stringify_64_bit_integers(false)
        .use_proto_field_name(true)
        .skip_default_fields(false);
    let mut ser = serde_json::Serializer::new(Vec::new());
    msg.serialize_with_options(&mut ser, &opts)
        .context("failed to serialize DynamicMessage")?;
    let bytes = ser.into_inner();
    Ok(serde_json::from_slice(&bytes)?)
}

/// If `payload` matches the `{name, data}` action-wrapper shape, base64-decode
/// `data`, XOR-decrypt, and decode the resulting bytes as the action message
/// named by `name`. Replaces `data` in place with the decoded JSON.
fn maybe_decode_action(mut payload: JsonValue) -> Result<JsonValue> {
    let needs_decode = payload.is_object()
        && payload.get("name").and_then(JsonValue::as_str).is_some()
        && payload.get("data").and_then(JsonValue::as_str).is_some();
    if !needs_decode {
        return Ok(payload);
    }
    let action_name = payload["name"].as_str().unwrap().to_string();
    let b64 = payload["data"].as_str().unwrap();
    let decoded = decode_action(&action_name, b64)?;
    payload
        .as_object_mut()
        .unwrap()
        .insert("data".to_string(), decoded);
    Ok(payload)
}

/// Resolve an action `name` (`.lq.ActionFoo`, or a bare `ActionFoo`) to its
/// liqi message descriptor and decode `bytes` as that message into JSON.
/// Shared by the live-action path (post-XOR) and the GameRestore path
/// (no XOR).
fn decode_named_action(name: &str, bytes: &[u8]) -> Result<JsonValue> {
    let parts: Vec<&str> = name.split('.').filter(|s| !s.is_empty()).collect();
    ensure!(!parts.is_empty(), "empty action name");
    // Action names are typically `.lq.ActionFoo`; fall back to last segment.
    let fqn = if parts.len() == 1 {
        format!("lq.{}", parts[0])
    } else {
        format!("{}.{}", parts[0], parts[parts.len() - 1])
    };
    let desc = POOL
        .get_message_by_name(&fqn)
        .with_context(|| format!("unknown action type: {fqn}"))?;
    decode_to_json(&desc, bytes)
}

/// Decode a live `.lq.ActionPrototype.data` payload: base64 → XOR-decrypt →
/// protobuf. The live notify obfuscates the action bytes (see [`wtf_decode`]).
pub fn decode_action(name: &str, b64: &str) -> Result<JsonValue> {
    let mut bytes = BASE64
        .decode(b64)
        .with_context(|| format!("base64 decode failed for action {name}"))?;
    wtf_decode(&mut bytes);
    decode_named_action(name, &bytes)
}

/// Decode an action embedded in a `GameRestore` replay (the `game_restore`
/// field of a `.lq.FastTest.syncGame` / `.lq.FastTest.enterGame` response).
///
/// Unlike the live `.lq.ActionPrototype.data` notify, actions stored inside a
/// `GameRestore` are **plain base64 protobuf** — the server does not apply the
/// position-dependent XOR ([`wtf_decode`]) to them. Running the XOR here would
/// corrupt the bytes, so this path base64-decodes and protobuf-decodes
/// directly, with no XOR step.
pub fn decode_restore_action(name: &str, b64: &str) -> Result<JsonValue> {
    let bytes = BASE64
        .decode(b64)
        .with_context(|| format!("base64 decode failed for restore action {name}"))?;
    decode_named_action(name, &bytes)
}

/// Position-dependent XOR scheme used by Majsoul for action payloads.
/// Mirrors the algorithm in MajsoulMax-rs `parser.rs::wtf_decode`.
fn wtf_decode(data: &mut [u8]) {
    const KEYS: [u8; 9] = [0x84, 0x5E, 0x4E, 0x42, 0x39, 0xA2, 0x1F, 0x60, 0x1C];
    let base = 23 ^ data.len();
    for (i, b) in data.iter_mut().enumerate() {
        let k = KEYS[i % KEYS.len()] as usize;
        *b ^= (base + 5 * i + k) as u8;
    }
}

/// Encode a liqi frame as the client or server would send it. `type_name` is
/// the inner message (`lq.ReqAuthGame`); a response's wrapper name is empty.
#[cfg(test)]
pub(crate) fn encode_frame(
    kind: MessageType,
    msg_id: u16,
    method: &str,
    type_name: &str,
    payload: JsonValue,
) -> Vec<u8> {
    #[derive(::prost::Message)]
    struct Raw {
        #[prost(string, tag = "1")]
        name: ::prost::alloc::string::String,
        #[prost(bytes = "vec", tag = "2")]
        data: ::prost::alloc::vec::Vec<u8>,
    }
    let desc = POOL.get_message_by_name(type_name).expect("known type");
    let inner = DynamicMessage::deserialize(desc, payload).expect("payload fits the type");
    let name = if kind == MessageType::Response {
        String::new()
    } else {
        method.to_string()
    };
    let wrapper = Raw {
        name,
        data: inner.encode_to_vec(),
    };
    let mut out = match kind {
        MessageType::Notify => vec![1],
        MessageType::Request => vec![2],
        MessageType::Response => vec![3],
    };
    if kind != MessageType::Notify {
        out.extend(msg_id.to_le_bytes());
    }
    out.extend(wrapper.encode_to_vec());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_loads() {
        assert!(POOL.get_message_by_name("lq.Wrapper").is_some());
    }

    #[test]
    fn routes_load() {
        // Flat rpc-map: `.lq.Service.method` -> { req, resp } (leading-dot FQNs).
        let entry = &ROUTES[".lq.Lobby.fetchConnectionInfo"];
        assert_eq!(entry["req"].as_str(), Some(".lq.ReqCommon"));
        assert_eq!(entry["resp"].as_str(), Some(".lq.ResConnectionInfo"));
    }

    /// Every rpc route's req/resp type must resolve in the descriptor pool —
    /// the runtime invariant `lookup_method_types` depends on. Guards against a
    /// proto/liqi.json mismatch slipping through a proto regeneration.
    #[test]
    fn all_routes_resolve() {
        let routes = ROUTES.as_object().expect("rpc-map is a JSON object");
        assert!(
            routes.len() > 300,
            "suspiciously few routes: {}",
            routes.len()
        );
        for (method, spec) in routes {
            for role in ["req", "resp"] {
                let fqn = spec[role]
                    .as_str()
                    .unwrap_or_else(|| panic!("{method} missing {role}"));
                assert!(
                    POOL.get_message_by_name(fqn.trim_start_matches('.'))
                        .is_some(),
                    "{method} {role} type {fqn} not in pool",
                );
            }
        }
    }

    /// mjai mapping in `mod.rs` reads decoded fields by JSON key, so a renamed or
    /// renumbered field would NOT fail compilation — only decode wrong at
    /// runtime. Lock the field numbers of the gameplay messages it relies on so a
    /// future extraction that drifts them fails loudly here instead.
    #[test]
    fn gameplay_field_numbers_stable() {
        let expected: &[(&str, &[(&str, u32)])] = &[
            (
                "lq.ActionPrototype",
                &[("step", 1), ("name", 2), ("data", 3)],
            ),
            (
                "lq.ActionNewRound",
                &[
                    ("chang", 1),
                    ("ju", 2),
                    ("ben", 3),
                    ("tiles", 4),
                    ("scores", 6),
                    ("liqibang", 8),
                    ("doras", 14),
                ],
            ),
            (
                "lq.ActionDiscardTile",
                &[
                    ("seat", 1),
                    ("tile", 2),
                    ("is_liqi", 3),
                    ("moqie", 5),
                    ("doras", 8),
                    ("is_wliqi", 9),
                ],
            ),
            (
                "lq.ActionDealTile",
                &[("seat", 1), ("tile", 2), ("liqi", 5), ("doras", 6)],
            ),
            (
                "lq.ActionChiPengGang",
                &[("seat", 1), ("type", 2), ("tiles", 3), ("froms", 4)],
            ),
            (
                "lq.ActionHule",
                &[("hules", 1), ("delta_scores", 3), ("scores", 5)],
            ),
            (
                "lq.ReqSelfOperation",
                &[("type", 1), ("tile", 3), ("moqie", 5)],
            ),
        ];
        for (msg_name, fields) in expected {
            let desc = POOL
                .get_message_by_name(msg_name)
                .unwrap_or_else(|| panic!("message {msg_name} missing from pool"));
            for (field_name, number) in *fields {
                let field = desc
                    .get_field_by_name(field_name)
                    .unwrap_or_else(|| panic!("{msg_name}.{field_name} missing"));
                assert_eq!(
                    field.number(),
                    *number,
                    "{msg_name}.{field_name} renumbered",
                );
            }
        }
    }

    #[test]
    fn wtf_decode_roundtrip() {
        let original = b"hello majsoul world".to_vec();
        let mut buf = original.clone();
        wtf_decode(&mut buf);
        wtf_decode(&mut buf);
        assert_eq!(buf, original);
    }

    /// Real captured `ActionNewRound` from a GameRestore replay. The action
    /// `data` is plain base64 protobuf (NOT XOR-obfuscated), so the no-XOR
    /// `decode_restore_action` path must recover the readable tehai, while the
    /// XOR-applying `decode_action` path must NOT (it sees corrupted bytes).
    /// This locks in the "restore actions skip the XOR" decision.
    const RESTORE_NEW_ROUND_B64: &str = "CAAQABgAIgI0cCICMW0iAjNwIgI0eiICN3oiAjZ6IgIwcCICM3MiAjBtIgI0cyICMXMiAjlzIgIyejIMqMMBqMMBqMMBqMMBQABYAGhFcgIxenoCCAB6AggBegIIAnoCCAOaAUA1NWQ2NzQ3MTRjNjAzODFhNGJjOTJmYzBmOWIwNWVjNDU4OWZlMzI0NTQ4OWVmOGY3NTc5NTVmMzIzZWIzZTE1qgFAYzFmNmY1YTQwOGFjMzgyZGEyZGE1MTA3OTUzYmUzYTg1N2M5OTdmNGNkMjg2M2JlZjczY2M3ZjE3MDllMDA4Mw==";

    #[test]
    fn decode_restore_action_skips_xor() {
        let expected = vec![
            "4p", "1m", "3p", "4z", "7z", "6z", "0p", "3s", "0m", "4s", "1s", "9s", "2z",
        ];

        let json = decode_restore_action("ActionNewRound", RESTORE_NEW_ROUND_B64)
            .expect("restore action should decode without XOR");
        let tiles: Vec<&str> = json["tiles"]
            .as_array()
            .expect("tiles array")
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(tiles, expected);

        // Same bytes through the live (XOR) path must not yield the same tehai:
        // it either fails to decode or produces garbage.
        let via_xor = decode_action("ActionNewRound", RESTORE_NEW_ROUND_B64);
        let same = via_xor.ok().and_then(|j| {
            j["tiles"].as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
        }) == Some(expected.iter().map(|s| s.to_string()).collect());
        assert!(!same, "XOR path unexpectedly produced the plain tehai");
    }
}
