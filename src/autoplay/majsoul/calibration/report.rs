//! The client's per-kyoku `Lobby.logReport`, reduced to what calibration and
//! the cursor controller read.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// One `ReqLogReport`. Device info, UA and emoji counts are dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogReportSample {
    /// Same hash the bridge stamps on `GameMeta.game_id`.
    pub game_id: Option<u64>,
    /// When the bridge decoded it, epoch ms.
    pub received_ms: i64,
    /// Frames this kyoku in which the cursor position changed.
    pub d: u32,
    /// Click mode setting (0 single, 1 double).
    pub m: u32,
    /// Shortest / longest double-click gap this kyoku, ms (double mode only).
    pub t_ms: u32,
    pub g_ms: u32,
    /// Kyoku start, epoch seconds.
    pub kyoku_start_s: i64,
    pub chang: u32,
    pub ju: u32,
    pub ben: u32,
}

impl LogReportSample {
    /// Read the decoded request payload. `None` when it is not a logReport
    /// shape (no `d` or no `a`).
    pub fn from_payload(
        payload: &JsonValue,
        game_id: Option<u64>,
        received_ms: i64,
    ) -> Option<Self> {
        let u = |k: &str| {
            payload
                .get(k)
                .and_then(JsonValue::as_u64)
                .and_then(|v| u32::try_from(v).ok())
        };
        let d = u("d")?;
        let kyoku_start_s = payload.get("a").and_then(JsonValue::as_i64)?;
        Some(Self {
            game_id,
            received_ms,
            d,
            m: u("m").unwrap_or(0),
            t_ms: u("t").unwrap_or(0),
            g_ms: u("g").unwrap_or(0),
            kyoku_start_s,
            chang: u("x").unwrap_or(0),
            ju: u("y").unwrap_or(0),
            ben: u("z").unwrap_or(0),
        })
    }

    /// Kyoku length in seconds, from its start to the report.
    pub fn duration_s(&self) -> f64 {
        (self.received_ms as f64 / 1000.0 - self.kyoku_start_s as f64).max(0.0)
    }

    /// The client's moving frames per second over the kyoku.
    pub fn rate(&self) -> Option<f64> {
        let dur = self.duration_s();
        (dur > 0.0).then(|| f64::from(self.d) / dur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_fields_it_needs() {
        let p = json!({
            "a": 1_000, "d": 1500, "m": 1, "t": 180, "g": 420,
            "x": 1, "y": 3, "z": 2, "e": 300, "f": 600,
            "c": {"device_id": "fake"}, "uuid": "fake-uuid", "emoji": [], "v": 1
        });
        let s = LogReportSample::from_payload(&p, Some(7), 1_100_000).unwrap();
        assert_eq!(s.d, 1500);
        assert_eq!((s.m, s.t_ms, s.g_ms), (1, 180, 420));
        assert_eq!((s.chang, s.ju, s.ben), (1, 3, 2));
        assert_eq!(s.game_id, Some(7));
        assert!((s.duration_s() - 100.0).abs() < 1e-9);
        assert!((s.rate().unwrap() - 15.0).abs() < 1e-9);
    }

    #[test]
    fn missing_counter_is_not_a_report() {
        assert!(LogReportSample::from_payload(&json!({"a": 1}), None, 0).is_none());
        assert!(LogReportSample::from_payload(&json!({"d": 1}), None, 0).is_none());
    }

    #[test]
    fn a_report_before_its_start_has_no_rate() {
        let s =
            LogReportSample::from_payload(&json!({"a": 2_000, "d": 3}), None, 1_000_000).unwrap();
        assert_eq!(s.duration_s(), 0.0);
        assert!(s.rate().is_none());
    }
}
