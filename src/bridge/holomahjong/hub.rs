//! MagicOnion StreamingHub envelopes on the battle connection.
//!
//! Each gRPC message on `/IMahjongHub/Connect` is one MessagePack array:
//! `[method, body]` for a server broadcast or a client fire-and-forget call,
//! `[message_id, method, body]` for a client request and its response.
//! `method` is MagicOnion's id for the hub method: FNV-1a 32 over the
//! method name, as a signed 32-bit integer. Bodies are MessagePack-CSharp
//! objects serialized as arrays, so fields are positional.

use serde_json::Value as JsonValue;

/// MagicOnion's method id for a hub method name.
pub const fn method_id(name: &str) -> i64 {
    let bytes = name.as_bytes();
    let mut hash: u32 = 2_166_136_261;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u32).wrapping_mul(16_777_619);
        i += 1;
    }
    hash as i32 as i64
}

macro_rules! methods {
    ($($konst:ident = $name:literal),* $(,)?) => {
        $(pub const $konst: i64 = method_id($name);)*
        /// The method name for an id this bridge knows.
        pub fn method_name(id: i64) -> Option<&'static str> {
            match id {
                $($konst => Some($name),)*
                _ => None,
            }
        }
    };
}

methods! {
    // Server → client.
    ON_KAIJU = "OnKaiju",
    ON_QIPAI = "OnQipai",
    ON_ZIMO = "OnZimo",
    ON_DAPAI = "OnDapai",
    ON_FULOU = "OnFulou",
    ON_LIZHI = "OnLizhi",
    ON_HULE = "OnHule",
    ON_PINGJU = "OnPingju",
    ON_UPDATE_FIELD_INFO = "OnUpDateFieldInfo",
    ON_UPDATE_PLAYER_INFO = "OnUpDatePlayerInfoEvent",
    ON_END_GAME_VOTE_START = "OnEndGameVoteStart",
    ON_UPDATE_END_GAME_VOTE = "OnUpDateEndGameVote",
    ON_RESPONSE_END_GAME_VOTE = "OnResponseEndGameVote",
    ON_AN_GANG_ADD_GANG = "OnAnGangAddGang",
    ON_ADD_BAOPAI = "OnAddBaopai",
    ON_BABEI = "OnBabei",
    ON_HUANG_PAI = "OnHuangPai",
    ON_JIEJU = "OnJieju",
    ON_ZHUNBEI_CHONGLIAN = "OnZhunbeiChonglian",
    ON_CHONGXIN_LIANJIE_CHENGGONG = "OnChongxinLianjieChenggong",
    ON_SYSTEM_CALL_EVENT = "OnSystemCallEvent",
    ON_GAME_START_ERROR = "OnGameStartError",
    ON_KAI_GUAN_ZHAN_EVENT = "OnKaiGuanZhanEvent",
    ON_BROADCAST_DATA_EVENT = "OnBroadCastDataEvent",
    ON_ADVICE_SETTING = "OnAdviceSetting",
    // Client → server.
    JOIN_ROOM_ASYNC = "JoinRoomAsync",
    READY = "Ready",
    KAIJU_EVENT_COMPLETE = "KaijuEventComplete",
    QIPAI_EVENT_COMPLETE = "QipaiEventComplete",
    SELECT_DAPAI = "SelectDapai",
    SELECT_FULOU = "SelectFulou",
    SELECT_ACTION_SKIP = "SelectActionSkip",
    SELECT_HULE = "SelectHule",
    SELECT_AN_GANG_ADD_GANG = "SelectAnGangAddGang",
    SELECT_DAOPAI = "SelectDaopai",
    RESULT_COMPLETE = "ResultComplete",
    RESUME_CONTROL = "ResumeControl",
    END_GAME_VOTE_START = "EndGameVoteStart",
    END_GAME_VOTE_VOTE = "EndGameVote_Vote",
    SELECT_BABEI = "SelectBabei",
    ZHUNBEI_CHONGLIAN_EVENT_COMPLETE = "ZhunbeiChonglianEventComplete",
    CHONGXIN_LIANJIE_CHENGGONG_EVENT_COMPLETE = "ChongxinLianjieChenggongEventComplete",
    JOIN_ROOM_WATCH_ASYNC = "JoinRoomWatchAsync",
    BROADCAST = "BroadCast",
    ADVICE_SETTING = "AdviceSetting",
    REQUEST_PAUSE = "RequestPause",
}

/// One decoded hub message.
#[derive(Debug, Clone, PartialEq)]
pub struct HubMessage {
    /// Set on requests and their responses.
    pub message_id: Option<i64>,
    pub method: i64,
    pub body: JsonValue,
}

impl HubMessage {
    /// Method name when known, else the raw id — for logs and the Inspector.
    pub fn label(&self) -> String {
        method_name(self.method)
            .map(str::to_string)
            .unwrap_or_else(|| format!("method#{}", self.method))
    }
}

/// Decode one hub message. `None` for anything that isn't a 2- or 3-element
/// array with an integer method id (heartbeats, error responses).
pub fn decode(bytes: &[u8]) -> Option<HubMessage> {
    let mut cursor = bytes;
    let value = rmpv::decode::read_value(&mut cursor).ok()?;
    if !cursor.is_empty() {
        return None;
    }
    let rmpv::Value::Array(items) = value else {
        return None;
    };
    let int = |v: &rmpv::Value| v.as_i64();
    match items.as_slice() {
        [method, body] => Some(HubMessage {
            message_id: None,
            method: int(method)?,
            body: to_json(body),
        }),
        [id, method, body] => Some(HubMessage {
            message_id: Some(int(id)?),
            method: int(method)?,
            body: to_json(body),
        }),
        _ => None,
    }
}

/// MessagePack → JSON. Binary becomes a hex string and the timestamp
/// extension (type -1) Unix milliseconds; other extensions become
/// `{"ext": type, "hex": …}`.
pub fn to_json(v: &rmpv::Value) -> JsonValue {
    use rmpv::Value as V;
    match v {
        V::Nil => JsonValue::Null,
        V::Boolean(b) => JsonValue::Bool(*b),
        V::Integer(i) => i
            .as_i64()
            .map(JsonValue::from)
            .or_else(|| i.as_u64().map(JsonValue::from))
            .unwrap_or(JsonValue::Null),
        V::F32(f) => {
            serde_json::Number::from_f64(f64::from(*f)).map_or(JsonValue::Null, JsonValue::Number)
        }
        V::F64(f) => serde_json::Number::from_f64(*f).map_or(JsonValue::Null, JsonValue::Number),
        V::String(s) => JsonValue::String(String::from_utf8_lossy(s.as_bytes()).into_owned()),
        V::Binary(b) => JsonValue::String(hex(b)),
        V::Array(a) => JsonValue::Array(a.iter().map(to_json).collect()),
        V::Map(m) => JsonValue::Object(
            m.iter()
                .map(|(k, v)| {
                    let key = match k {
                        V::String(s) => String::from_utf8_lossy(s.as_bytes()).into_owned(),
                        other => to_json(other).to_string(),
                    };
                    (key, to_json(v))
                })
                .collect(),
        ),
        V::Ext(-1, data) => timestamp_ms(data).map_or_else(|| ext_json(-1, data), JsonValue::from),
        V::Ext(t, data) => ext_json(*t, data),
    }
}

fn ext_json(t: i8, data: &[u8]) -> JsonValue {
    serde_json::json!({ "ext": t, "hex": hex(data) })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The MessagePack timestamp extension in its 32/64/96-bit forms.
fn timestamp_ms(data: &[u8]) -> Option<i64> {
    let (secs, nanos) = match data.len() {
        4 => (i64::from(u32::from_be_bytes(data.try_into().ok()?)), 0),
        8 => {
            let v = u64::from_be_bytes(data.try_into().ok()?);
            ((v & 0x3_FFFF_FFFF) as i64, (v >> 34) as i64)
        }
        12 => (
            i64::from_be_bytes(data[4..].try_into().ok()?),
            i64::from(u32::from_be_bytes(data[..4].try_into().ok()?)),
        ),
        _ => return None,
    };
    secs.checked_mul(1000)?.checked_add(nanos / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmpv::Value as V;

    fn encode(v: &V) -> Vec<u8> {
        let mut out = Vec::new();
        rmpv::encode::write_value(&mut out, v).unwrap();
        out
    }

    #[test]
    fn method_ids_match_the_wire() {
        assert_eq!(ON_DAPAI, 664_106_223);
        assert_eq!(ON_ZIMO, 306_042_437);
        assert_eq!(ON_LIZHI, -430_587_364);
        assert_eq!(SELECT_DAPAI, 1_957_121_736);
        assert_eq!(JOIN_ROOM_ASYNC, 464_561_018);
        assert_eq!(ON_AN_GANG_ADD_GANG, -397_989_692);
        assert_eq!(ON_ADD_BAOPAI, 898_925_355);
        assert_eq!(ON_JIEJU, -1_887_149_647);
        assert_eq!(method_name(ON_HULE), Some("OnHule"));
        assert_eq!(method_name(12345), None);
    }

    #[test]
    fn decodes_broadcasts_requests_and_responses() {
        let push = encode(&V::Array(vec![
            V::from(ON_ZIMO),
            V::Array(vec![V::Nil, V::from(2), V::from("m3")]),
        ]));
        let m = decode(&push).unwrap();
        assert_eq!((m.message_id, m.method), (None, ON_ZIMO));
        assert_eq!(m.body, serde_json::json!([null, 2, "m3"]));
        assert_eq!(m.label(), "OnZimo");

        let resp = encode(&V::Array(vec![
            V::from(1),
            V::from(JOIN_ROOM_ASYNC),
            V::Array(vec![V::from(0)]),
        ]));
        let m = decode(&resp).unwrap();
        assert_eq!((m.message_id, m.method), (Some(1), JOIN_ROOM_ASYNC));
    }

    #[test]
    fn rejects_non_envelopes_and_trailing_bytes() {
        assert!(decode(&encode(&V::from(7))).is_none());
        assert!(decode(&encode(&V::Array(vec![V::from(1)]))).is_none());
        assert!(decode(&encode(&V::Array(vec![V::from("x"), V::Nil]))).is_none());
        let mut two = encode(&V::Array(vec![V::from(ON_ZIMO), V::Nil]));
        two.push(0xc0);
        assert!(decode(&two).is_none());
        assert!(decode(b"\xff\xff").is_none());
    }

    #[test]
    fn timestamps_become_unix_millis() {
        let ts32 = V::Ext(-1, 1_700_000_123u32.to_be_bytes().to_vec());
        assert_eq!(to_json(&ts32), serde_json::json!(1_700_000_123_000i64));
        let packed = (500_000_000u64 << 34) | 1_700_000_123u64;
        let ts64 = V::Ext(-1, packed.to_be_bytes().to_vec());
        assert_eq!(to_json(&ts64), serde_json::json!(1_700_000_123_500i64));
        assert_eq!(
            to_json(&V::Ext(5, vec![0xab])),
            serde_json::json!({"ext": 5, "hex": "ab"})
        );
    }
}
