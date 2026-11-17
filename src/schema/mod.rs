//! Shared schema types used across the project.
//!
//! Anything that needs to travel between modules — protocol events, IPC
//! payloads between backend and frontend, persisted records — lives here so
//! it isn't owned by any single subsystem.

pub mod history;
pub mod inspector;
pub mod ipc;
pub mod mjai;

pub use history::{
    GameRecord, GameStats, HistoryEvent, HistoryEventLog, HistoryFilter, KyokuMode, MatchInfo,
    Platform,
};
pub use inspector::{
    BotReaction, CaptureSource, FrameDirection, FrameRaw, HttpAnnotation, HttpBody, HttpExchange,
    HttpHeader, HttpPhase, InspectorEntry, ParsedFrame, WITHHELD,
};
pub use ipc::{
    BotInfo, BotSettings, BotStatus, CaptureKind, CaptureStatus, HoraScoreInfo, LoadStage,
    LogEntry, LogSessionInfo, Notification, NotifyLevel, PackLogsRequest, ReadInspectorRequest,
    ReadInspectorResponse, ReadLogRequest, ReadLogResponse, Snapshot,
};
pub use mjai::{GameEndReason, GameMeta, MjaiEvent};
