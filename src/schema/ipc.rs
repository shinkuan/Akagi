//! Backend ↔ frontend IPC payload types.
//!
//! These types travel two directions:
//!
//! - **Backend → frontend**: emitted as Tauri events by `crate::ipc`. Names
//!   match the Tauri event names (kebab-case): `notify`, `bot-status`,
//!   `proxy-status`.
//! - **Frontend → backend**: returned from `#[tauri::command]` handlers in
//!   `crate::ipc::commands`.
//!
//! All types are `Serialize + Deserialize` so they round-trip through Tauri's
//! JSON bridge and through the in-process broadcast buses in
//! `crate::event_bus`. Variants are tagged so the wire shape stays stable
//! when new states are added — frontends can match on `state` / `level` /
//! `stage` without positional surprises.

use serde::{Deserialize, Serialize};

// ---------- Notification ----------

/// Severity for a frontend toast / notification box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    Info,
    Success,
    Warn,
    Error,
}

/// One frontend-facing notification. Any subsystem may push these onto
/// `event_bus::NotifyBus`; `crate::ipc` forwards them to all webviews as
/// the `notify` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub level: NotifyLevel,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// `true` → frontend keeps the toast until the user dismisses it.
    /// Used for long-running blocking states (e.g. "installing bot deps").
    #[serde(default)]
    pub sticky: bool,
    /// Stable key. Frontends should replace any prior toast with the same
    /// `id`, so progress updates don't pile up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Optional i18n keys: when set and the frontend has a translation, it
    /// renders `t(key, args)` in the user's language instead of the raw
    /// `title`/`body`, which remain as (English) fallbacks — and as the text
    /// that goes into logs on the backend side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_key: Option<String>,
    /// Interpolation values for `body_key` (i18next-style `{{name}}` slots).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

impl Notification {
    pub fn info(title: impl Into<String>) -> Self {
        Self::new(NotifyLevel::Info, title)
    }
    pub fn success(title: impl Into<String>) -> Self {
        Self::new(NotifyLevel::Success, title)
    }
    pub fn warn(title: impl Into<String>) -> Self {
        Self::new(NotifyLevel::Warn, title)
    }
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(NotifyLevel::Error, title)
    }

    fn new(level: NotifyLevel, title: impl Into<String>) -> Self {
        Self {
            level,
            title: title.into(),
            body: None,
            sticky: false,
            id: None,
            title_key: None,
            body_key: None,
            args: None,
        }
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn title_key(mut self, key: impl Into<String>) -> Self {
        self.title_key = Some(key.into());
        self
    }

    pub fn body_key(mut self, key: impl Into<String>) -> Self {
        self.body_key = Some(key.into());
        self
    }

    pub fn args(mut self, args: serde_json::Value) -> Self {
        self.args = Some(args);
        self
    }

    pub fn sticky(mut self) -> Self {
        self.sticky = true;
        self
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

// ---------- BotStatus ----------

/// Sub-stage during the slow first-spawn path.
///
/// `SyncingDeps` is the long one — `uv sync` downloads every wheel listed
/// in the bot's `pyproject.toml` into `<bot_dir>/.akagi/venv` on first use,
/// which can take tens of seconds. `Spawning` is the short tail (process
/// creation + waiting for the first react round-trip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadStage {
    SyncingDeps,
    Spawning,
}

/// Lifecycle state of the active bot subprocess.
///
/// Tagged with `state` so the frontend can match on a string discriminant
/// (`{"state":"loading","bot":"mortal","stage":"syncing_deps"}`) without
/// having to know Rust's untagged-enum trick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BotStatus {
    /// No active bot — waiting for `start_game` (or bot disabled).
    Idle,
    /// First-time install / spawn in progress. UI should show a spinner.
    Loading { bot: String, stage: LoadStage },
    /// Subprocess up and reacting.
    Ready { bot: String, actor_id: u8 },
    /// Spawn or react path failed; runner torn down.
    Error { bot: String, error: String },
    /// Game ended (`EndGame`) — runner dropped, awaiting next start.
    Stopped { bot: String },
}

// ---------- CaptureStatus ----------

/// Discriminant of the active capture transport. Mirrors
/// [`crate::capture::CaptureKind`] for IPC payloads — re-exported here so
/// `schema::*` is a self-contained surface for frontend type generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    Mitm,
    Chromium,
}

/// Lifecycle of the active capture backend.
///
/// `descriptor` carries a human-readable label (proxy listen addr for
/// MITM, executable path for Chromium) — surface in UI, do not parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureStatus {
    Stopped,
    Starting {
        kind: CaptureKind,
        descriptor: String,
    },
    Running {
        kind: CaptureKind,
        descriptor: String,
    },
    Error {
        kind: CaptureKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        descriptor: Option<String>,
        error: String,
    },
}

// ---------- HoraScoreInfo ----------

/// Bot's hora-decision score breakdown, computed on demand by
/// `commands::compute_bot_hora_score`. Returned as `Option<Self>` —
/// `None` when no game is in progress, the actor's hand isn't a valid
/// agari shape, or the winning tile can't be inferred from the live state.
///
/// `points` is the actor's *received* total: for ron, the discarder's pay;
/// for tsumo, the sum across all paying seats. Honba/riichi-stick payouts
/// are NOT folded in (those depend on platform settlement).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoraScoreInfo {
    pub points: u32,
    pub han: u32,
    pub fu: u32,
    pub yakuman: bool,
    /// mjai tile string (e.g. `"5mr"`, `"C"`) — the tile the actor wins on.
    /// For ron this is the most recent discard; for tsumo the most recent draw.
    pub win_tile: String,
}

// ---------- Snapshot / BotInfo ----------
//
// Returned by `commands::get_status` and `commands::list_bots`. These
// types pull in `AppConfig` (one direction only — schema doesn't cycle
// back into config). Keep these stable; the React side keys off field
// names.

use crate::config::AppConfig;
use std::collections::HashMap;
use std::path::PathBuf;

/// One discovered bot, IPC-shaped (separate from `bot::BotEntry` so the
/// wire contract can evolve independently of the registry internals).
///
/// `manifest` carries the bot's settings schema (rendered as a form on
/// the frontend). Bots without a `manifest.toml` have it set to `None`
/// and the UI hides the settings panel for them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotInfo {
    pub name: String,
    pub dir: String,
    pub has_pyproject: bool,
    /// `true` when the bot's Python environment is already installed (no
    /// `pyproject.toml`, or its venv + sync stamp are up to date). The UI
    /// blocks activating a bot whose env isn't ready so a game never picks
    /// a bot that would need a slow first-spawn `uv sync`.
    #[serde(default)]
    pub env_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<crate::bot::manifest::Manifest>,
}

/// Returned by the `get_bot_settings` command. `values` is the result of
/// merging defaults from `manifest` with whatever lives in the bot's
/// `settings.toml`. Frontend should treat `manifest.settings` as the form
/// schema and `values` as the form's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSettings {
    pub manifest: crate::bot::manifest::Manifest,
    pub values: std::collections::BTreeMap<String, serde_json::Value>,
}

/// One-shot dump of everything the UI needs on startup. Cheap to clone
/// (config + two enums + a path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub config: AppConfig,
    pub bot_status: BotStatus,
    pub capture_status: CaptureStatus,
    pub log_dir: PathBuf,
}

// ---------- Logs ----------
//
// One canonical shape for an emitted tracing event: written to disk as a
// line of `all.jsonl` AND broadcast to the frontend log viewer over a
// `tauri::ipc::Channel`. Keeping disk and wire shapes identical means the
// initial-load reader and the live-tail subscriber feed the same UI rows
// without a translation step.

/// One log event. `ts_ms` is millisecond Unix time (local clock at emit).
/// `fields` carries the structured fields recorded via `tracing` (the
/// canonical `message` is hoisted to its own slot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts_ms: i64,
    pub level: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub message: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,
}

/// Metadata for one log session directory under `<log_root>/`. Sorted
/// newest-first by `name` (timestamp). `is_active` marks the session
/// owned by the running process — the UI uses this to enable live tail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSessionInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub is_active: bool,
}

/// Filtered, paginated read against a single session's `all.jsonl`.
/// `limit` is capped server-side at 2000 to bound payload size.
/// `levels`/`targets`/`search` are AND-ed; within each, multiple entries
/// are OR-ed (level in set; any target prefix matches; substring on
/// `message` is case-insensitive).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadLogRequest {
    pub session: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadLogResponse {
    pub entries: Vec<LogEntry>,
    pub has_more: bool,
    pub skipped_malformed: u32,
}

// ---------- Inspector ----------
//
// Mirrors the on-disk `<session>/inspector.jsonl` shape for past-session
// reads. Same `kind` filtering / pagination model as `ReadLogRequest`.

/// Filtered, paginated read against a session's `inspector.jsonl`.
/// `kinds` accepts `"ws_frame"`, `"mjai_event"`, `"bot_reaction"`.
/// `actor` filters records that reference an actor (mjai events with an
/// `actor` field; bot reactions by `actor_id`); ws frames are
/// always included regardless of actor unless `kinds` excludes them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadInspectorRequest {
    pub session: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<u8>,
    /// Case-insensitive substring on the entry's text representation
    /// (frame raw text, mjai event JSON, bot action JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadInspectorResponse {
    pub entries: Vec<crate::schema::InspectorEntry>,
    pub has_more: bool,
    pub skipped_malformed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_minimal_round_trips() {
        let n = Notification::info("hi");
        let s = serde_json::to_string(&n).unwrap();
        // No body / id; sticky=false is emitted (no skip_if).
        assert_eq!(s, r#"{"level":"info","title":"hi","sticky":false}"#);
        let back: Notification = serde_json::from_str(&s).unwrap();
        assert_eq!(back, n);
    }

    #[test]
    fn notification_full_round_trips() {
        let n = Notification::error("boom")
            .body("stack trace here")
            .sticky()
            .id("bot-load-fail");
        let s = serde_json::to_string(&n).unwrap();
        let back: Notification = serde_json::from_str(&s).unwrap();
        assert_eq!(back, n);
        assert!(s.contains(r#""level":"error""#));
        assert!(s.contains(r#""sticky":true"#));
        assert!(s.contains(r#""id":"bot-load-fail""#));
    }

    #[test]
    fn bot_status_loading_tagged_correctly() {
        let s = BotStatus::Loading {
            bot: "mortal".into(),
            stage: LoadStage::SyncingDeps,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(
            j,
            r#"{"state":"loading","bot":"mortal","stage":"syncing_deps"}"#
        );
        let back: BotStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn bot_status_ready_round_trips() {
        let s = BotStatus::Ready {
            bot: "example".into(),
            actor_id: 2,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"state":"ready","bot":"example","actor_id":2}"#);
        let back: BotStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn bot_status_idle_has_no_extra_fields() {
        let s = BotStatus::Idle;
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"state":"idle"}"#);
    }

    #[test]
    fn capture_status_running_round_trips() {
        let s = CaptureStatus::Running {
            kind: CaptureKind::Mitm,
            descriptor: "127.0.0.1:23410".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: CaptureStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        assert!(j.contains(r#""state":"running""#));
        assert!(j.contains(r#""kind":"mitm""#));
        assert!(j.contains(r#""descriptor":"127.0.0.1:23410""#));
    }

    #[test]
    fn capture_status_chromium_running() {
        let s = CaptureStatus::Running {
            kind: CaptureKind::Chromium,
            descriptor: "/usr/bin/google-chrome".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains(r#""kind":"chromium""#));
        let back: CaptureStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn capture_status_error_omits_null_descriptor() {
        let s = CaptureStatus::Error {
            kind: CaptureKind::Chromium,
            descriptor: None,
            error: "spawn failed".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains(r#""state":"error""#));
        assert!(j.contains(r#""kind":"chromium""#));
        // skip_serializing_if = Option::is_none → no descriptor key
        assert!(!j.contains("descriptor"), "got: {j}");
    }

    #[test]
    fn capture_status_stopped_minimal() {
        let s = CaptureStatus::Stopped;
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"state":"stopped"}"#);
    }

    #[test]
    fn log_entry_round_trips_minimal() {
        // Minimal entry with no file/line/fields — empty fields map and
        // missing optional file/line should not appear in the JSON.
        let e = LogEntry {
            ts_ms: 1_700_000_000_000,
            level: "INFO".into(),
            target: "akagi::lib".into(),
            file: None,
            line: None,
            message: "hello".into(),
            fields: HashMap::new(),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(!j.contains("file"), "got: {j}");
        assert!(!j.contains("line"), "got: {j}");
        assert!(!j.contains("fields"), "got: {j}");
        let back: LogEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn log_entry_round_trips_full() {
        let mut fields = HashMap::new();
        fields.insert("seat".into(), serde_json::Value::Number(2.into()));
        fields.insert("kind".into(), serde_json::Value::String("ron".into()));
        let e = LogEntry {
            ts_ms: 1_700_000_000_000,
            level: "WARN".into(),
            target: "akagi::proxy::handler".into(),
            file: Some("src/proxy/handler.rs".into()),
            line: Some(42),
            message: "stalled".into(),
            fields,
        };
        let j = serde_json::to_string(&e).unwrap();
        let back: LogEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, e);
    }
}
