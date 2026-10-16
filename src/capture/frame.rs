//! Inspector recording shared by the backends that hand WebSocket frames to
//! a bridge themselves (Chromium and ingest).
//!
//! Both receive a frame as text or as base64 rather than as raw bytes, so
//! the original string is recorded verbatim: text stays human-readable in
//! the JSONL, and binary is not decoded only to be re-encoded.

use crate::bridge::ParseResult;
use crate::inspector::InspectorWriter;
use crate::schema::{FrameDirection, FrameRaw, InspectorEntry};
use chrono::Local;

/// Record one `WsFrame` row. `size` is the decoded payload length, which
/// for a binary frame is not the length of its base64 in `raw`.
pub(crate) fn record_ws_frame(
    inspector: &InspectorWriter,
    direction: FrameDirection,
    flow_id: String,
    raw: FrameRaw,
    size: usize,
    result: &ParseResult,
) {
    inspector.record(InspectorEntry::WsFrame {
        ts_ms: Local::now().timestamp_millis(),
        direction,
        flow_id,
        size,
        raw,
        parsed: result.parsed.clone(),
        emitted: result.events.len(),
    });
}
