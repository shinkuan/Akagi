//! Calibration session: the games the user's own play recorded, and the
//! running counters of the kyoku in progress.
//!
//! The counters run all the time (the cursor's controller needs them while
//! autoplay plays). Games are kept, and raw input written, only while a
//! calibration is active; the raw file is the only per-kyoku record.

use super::recorder::InputBatch;
use super::report::LogReportSample;
use crate::config::ClickMode;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const REQUIRED_GAMES: f64 = 2.0;
pub const RECOMMENDED_GAMES: f64 = 5.0;
/// Shorter kyoku (an early ron) carry too few frames to say anything.
pub const MIN_KYOKU_S: f64 = 15.0;
/// The frame ratio needs this many moving frames in a kyoku to be read.
const MIN_RATIO_FRAMES: u32 = 50;
/// Weight of the newest kyoku in the smoothed frame ratio.
const RATIO_SMOOTHING: f64 = 0.3;
const STATE_FILE: &str = "state.json";
const RAW_DIR: &str = "raw";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRecord {
    pub game_id: u64,
    pub match_mode: Option<u8>,
    /// Whether its first kyoku seen was East 1, 0 honba.
    pub from_start: bool,
    pub kyoku_seen: bool,
    /// Ended with a final result (not a termination).
    pub ended: bool,
    /// Kyoku written to `raw_file`.
    pub kyoku: u32,
    pub raw_file: String,
}

impl GameRecord {
    /// How much this game counts towards the required games.
    pub fn weight(&self) -> f64 {
        if !(self.from_start && self.ended) || self.kyoku == 0 {
            return 0.0;
        }
        match self.match_mode {
            Some(2) | Some(12) => 1.0,
            Some(1) | Some(11) => 0.5,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionFile {
    pub active: bool,
    pub started_at: String,
    pub session_id: String,
    pub games: Vec<GameRecord>,
}

/// One kyoku: its report and its raw page input, one JSON line in the
/// game's raw file. Times are epoch ms; positions canvas-normalised.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawKyoku {
    pub chang: u32,
    pub ju: u32,
    pub ben: u32,
    pub duration_s: f64,
    pub d: u32,
    pub m: u32,
    pub t_ms: u32,
    pub g_ms: u32,
    pub rect: (f64, f64),
    /// Start of every animation frame.
    pub frames: Vec<f64>,
    /// `[t_ms, x, y]` of every frame the pointer had moved in.
    pub moves: Vec<[f64; 3]>,
    /// `[t_ms, down (1/0), button, x, y]`.
    pub buttons: Vec<[f64; 5]>,
    /// `[start_ms, end_ms]` of our own decision windows.
    pub own: Vec<[f64; 2]>,
}

/// What the UI shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStatus {
    pub active: bool,
    pub started_at: String,
    pub games: f64,
    pub kyoku: u32,
    pub required_games: f64,
    pub recommended_games: f64,
    /// The game in progress is being recorded and will count if finished.
    pub recording: bool,
}

/// The kyoku in progress, for the cursor's controller.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveRate {
    pub in_kyoku: bool,
    pub started_ms: f64,
    pub page_frames: u32,
    pub page_moving: u32,
    /// Client frames per page frame, smoothed over recent kyoku.
    pub frame_ratio: Option<f64>,
}

#[derive(Debug, Default)]
struct KyokuAcc {
    started_ms: f64,
    page_frames: u32,
    page_moving: u32,
    raw: RawKyoku,
}

#[derive(Debug, Default)]
pub struct Session {
    /// Empty until loaded: nothing is written then.
    dir: PathBuf,
    file: SessionFile,
    current_game: Option<usize>,
    kyoku: Option<KyokuAcc>,
    /// Own-window transitions `(t_ms, own)` of the kyoku in progress.
    phases: Vec<(f64, bool)>,
    own_now: bool,
    last_m: Option<u32>,
    frame_ratio: Option<f64>,
}

impl Session {
    /// Read `<dir>/state.json` if there is one; a session that was active
    /// when the app closed carries on.
    pub fn load(dir: PathBuf) -> Self {
        let file = fs::read_to_string(dir.join(STATE_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            dir,
            file,
            ..Self::default()
        }
    }

    pub fn active(&self) -> bool {
        self.file.active
    }

    pub fn games(&self) -> f64 {
        self.file.games.iter().map(GameRecord::weight).sum()
    }

    pub fn raw_files(&self) -> Vec<PathBuf> {
        self.file
            .games
            .iter()
            .filter(|g| g.kyoku > 0)
            .map(|g| self.dir.join(RAW_DIR).join(&g.raw_file))
            .collect()
    }

    pub fn status(&self) -> CalibrationStatus {
        let recording = self.file.active
            && self
                .current_game
                .is_some_and(|i| self.file.games[i].from_start);
        CalibrationStatus {
            active: self.file.active,
            started_at: self.file.started_at.clone(),
            games: self.games(),
            kyoku: self.file.games.iter().map(|g| g.kyoku).sum(),
            required_games: REQUIRED_GAMES,
            recommended_games: RECOMMENDED_GAMES,
            recording,
        }
    }

    pub fn live(&self) -> LiveRate {
        LiveRate {
            in_kyoku: self.kyoku.is_some(),
            started_ms: self.kyoku.as_ref().map_or(0.0, |k| k.started_ms),
            page_frames: self.kyoku.as_ref().map_or(0, |k| k.page_frames),
            page_moving: self.kyoku.as_ref().map_or(0, |k| k.page_moving),
            frame_ratio: self.frame_ratio,
        }
    }

    /// The click mode the client last reported.
    pub fn reported_click_mode(&self) -> Option<ClickMode> {
        match self.last_m? {
            0 => Some(ClickMode::Single),
            1 => Some(ClickMode::Double),
            _ => None,
        }
    }

    /// Begin recording. A session already running is left as it is.
    pub fn start(&mut self, now: chrono::DateTime<chrono::Local>) {
        if self.file.active {
            return;
        }
        self.file = SessionFile {
            active: true,
            started_at: now.to_rfc3339(),
            session_id: now.format("%Y%m%d-%H%M%S").to_string(),
            games: Vec::new(),
        };
        self.current_game = None;
    }

    /// Drop the session and its raw input.
    pub fn cancel(&mut self) {
        for path in self.raw_files() {
            let _ = fs::remove_file(path);
        }
        self.finish();
    }

    /// Stop recording. Raw input written so far stays on disk.
    pub fn finish(&mut self) {
        self.file.active = false;
        self.file.games.clear();
        self.current_game = None;
    }

    pub fn on_start_game(&mut self, game_id: Option<u64>, match_mode: Option<u8>) {
        self.kyoku = None;
        self.current_game = None;
        let (true, Some(game_id)) = (self.file.active, game_id) else {
            return;
        };
        if let Some(i) = self.file.games.iter().position(|g| g.game_id == game_id) {
            self.current_game = Some(i);
            return;
        }
        let n = self.file.games.len() + 1;
        self.file.games.push(GameRecord {
            game_id,
            match_mode,
            raw_file: format!("{}_{n}.jsonl", self.file.session_id),
            ..GameRecord::default()
        });
        self.current_game = Some(n - 1);
    }

    pub fn on_start_kyoku(&mut self, bakaze: &str, kyoku: u8, honba: u8, now_ms: f64) {
        self.kyoku = Some(KyokuAcc {
            started_ms: now_ms,
            ..KyokuAcc::default()
        });
        self.phases = vec![(now_ms, self.own_now)];
        if let Some(g) = self.current_game.map(|i| &mut self.file.games[i]) {
            if !g.kyoku_seen {
                g.kyoku_seen = true;
                g.from_start = bakaze == "E" && kyoku == 1 && honba == 0;
            }
        }
    }

    /// Our decision window opened (`true`) or closed.
    pub fn on_phase(&mut self, own: bool, now_ms: f64) {
        if own == self.own_now {
            return;
        }
        self.own_now = own;
        if self.kyoku.is_some() {
            self.phases.push((now_ms, own));
        }
    }

    pub fn on_input(&mut self, batch: &InputBatch) {
        let recording = self.file.active && self.current_game.is_some();
        let Some(k) = self.kyoku.as_mut() else {
            return;
        };
        let start = k.started_ms;
        let frames = batch.frames.iter().filter(|t| **t >= start);
        let moves = batch.moves.iter().filter(|m| m.t_ms >= start);
        k.page_frames += frames.clone().count() as u32;
        k.page_moving += moves.clone().count() as u32;
        if recording {
            k.raw.rect = batch.rect;
            k.raw.frames.extend(frames);
            k.raw.moves.extend(moves.map(|m| [m.t_ms, m.x, m.y]));
            k.raw
                .buttons
                .extend(batch.buttons.iter().filter(|b| b.t_ms >= start).map(|b| {
                    [
                        b.t_ms,
                        f64::from(u8::from(b.down)),
                        f64::from(b.button),
                        b.x,
                        b.y,
                    ]
                }));
        }
    }

    /// The kyoku's report: close it, and while calibrating keep it.
    pub fn on_report(&mut self, r: &LogReportSample) {
        self.last_m = Some(r.m);
        let Some(k) = self.kyoku.take() else {
            return;
        };
        if k.page_moving >= MIN_RATIO_FRAMES {
            let got = f64::from(r.d) / f64::from(k.page_moving);
            self.frame_ratio = Some(
                self.frame_ratio
                    .map_or(got, |p| p + RATIO_SMOOTHING * (got - p)),
            );
        }
        let own = own_intervals(&std::mem::take(&mut self.phases), r.received_ms as f64);
        let Some(game) = self.current_game.map(|i| &mut self.file.games[i]) else {
            return;
        };
        let same_game = r.game_id.is_none_or(|id| id == game.game_id);
        if !self.file.active || !same_game || r.duration_s() < MIN_KYOKU_S || k.page_frames == 0 {
            return;
        }
        let raw = RawKyoku {
            chang: r.chang,
            ju: r.ju,
            ben: r.ben,
            duration_s: r.duration_s(),
            d: r.d,
            m: r.m,
            t_ms: r.t_ms,
            g_ms: r.g_ms,
            own,
            ..k.raw
        };
        game.kyoku += 1;
        let path = self.dir.join(RAW_DIR).join(&game.raw_file);
        if self.dir.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = append_line(&path, &raw) {
            tracing::warn!("mouse calibration: could not write {}: {e}", path.display());
        }
    }

    /// `confirmed`: the game reached its final result.
    pub fn on_end_game(&mut self, confirmed: bool) {
        if let Some(g) = self.current_game.map(|i| &mut self.file.games[i]) {
            g.ended = confirmed;
        }
        self.current_game = None;
        self.kyoku = None;
    }

    pub fn save(&self) -> std::io::Result<()> {
        if self.dir.as_os_str().is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{STATE_FILE}.tmp"));
        fs::write(&tmp, serde_json::to_vec_pretty(&self.file)?)?;
        fs::rename(tmp, self.dir.join(STATE_FILE))
    }
}

fn append_line<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&line)
}

/// Own-window intervals from the transition log, the last closed at `end`.
fn own_intervals(phases: &[(f64, bool)], end: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut open: Option<f64> = None;
    for &(t, own) in phases {
        match (own, open) {
            (true, None) => open = Some(t),
            (false, Some(s)) => {
                out.push([s, t]);
                open = None;
            }
            _ => {}
        }
    }
    if let Some(s) = open {
        out.push([s, end]);
    }
    out
}

/// Raw kyoku lines of one game's file; unreadable lines are skipped.
pub fn read_raw(path: &Path) -> Vec<RawKyoku> {
    fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::majsoul::calibration::recorder::{Button, Move};
    use tempfile::TempDir;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05+00:00")
            .unwrap()
            .with_timezone(&chrono::Local)
    }

    fn report(game_id: u64, start_s: i64, end_ms: i64, d: u32) -> LogReportSample {
        LogReportSample {
            game_id: Some(game_id),
            received_ms: end_ms,
            d,
            m: 0,
            t_ms: 0,
            g_ms: 0,
            kyoku_start_s: start_s,
            chang: 0,
            ju: 0,
            ben: 0,
        }
    }

    /// `frames` frames 16 ms apart from `t0`, the first `moving` of them moving.
    fn batch(t0: f64, frames: usize, moving: usize) -> InputBatch {
        let ft: Vec<f64> = (0..frames).map(|i| t0 + i as f64 * 16.0).collect();
        InputBatch {
            moves: ft[..moving]
                .iter()
                .map(|&t| Move {
                    t_ms: t,
                    x: 0.5,
                    y: 0.5,
                })
                .collect(),
            buttons: vec![Button {
                t_ms: t0,
                down: true,
                button: 0,
                x: 0.5,
                y: 0.9,
            }],
            frames: ft,
            rect: (1280.0, 720.0),
        }
    }

    /// One whole game of `kyoku` kyoku, each 60 s long.
    fn play_game(s: &mut Session, id: u64, mode: u8, kyoku: u8, first: (&str, u8, u8), end: bool) {
        s.on_start_game(Some(id), Some(mode));
        for k in 0..kyoku {
            let start_ms = 1_000_000.0 + f64::from(k) * 100_000.0;
            let (bakaze, kn, honba) = if k == 0 { first } else { ("E", k + 1, 0) };
            s.on_start_kyoku(bakaze, kn, honba, start_ms);
            s.on_input(&batch(start_ms + 10.0, 100, 30));
            s.on_report(&report(
                id,
                (start_ms / 1000.0) as i64,
                start_ms as i64 + 60_000,
                900,
            ));
        }
        s.on_end_game(end);
    }

    fn session(tmp: &TempDir) -> Session {
        Session::load(tmp.path().to_path_buf())
    }

    #[test]
    fn games_count_by_length_and_completeness() {
        let tmp = TempDir::new().unwrap();
        let mut s = session(&tmp);
        s.start(now());
        play_game(&mut s, 1, 2, 8, ("E", 1, 0), true); // hanchan
        assert_eq!(s.games(), 1.0);
        play_game(&mut s, 2, 1, 4, ("E", 1, 0), true); // tonpuu
        assert_eq!(s.games(), 1.5);
        play_game(&mut s, 3, 2, 4, ("E", 3, 0), true); // joined mid-game
        play_game(&mut s, 4, 2, 4, ("E", 1, 0), false); // terminated
        assert_eq!(s.games(), 1.5);
        // Kyoku from every game are still samples.
        assert_eq!(s.status().kyoku, 20);
        let lines: usize = s.raw_files().iter().map(|p| read_raw(p).len()).sum();
        assert_eq!(lines, 20);
    }

    #[test]
    fn nothing_is_kept_outside_a_session() {
        let tmp = TempDir::new().unwrap();
        let mut s = session(&tmp);
        play_game(&mut s, 1, 2, 8, ("E", 1, 0), true);
        assert_eq!(s.status().kyoku, 0);
        assert!(s.raw_files().is_empty());
    }

    #[test]
    fn short_kyoku_and_other_games_reports_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let mut s = session(&tmp);
        s.start(now());
        s.on_start_game(Some(9), Some(2));
        s.on_start_kyoku("E", 1, 0, 1_000_000.0);
        s.on_input(&batch(1_000_010.0, 50, 10));
        s.on_report(&report(9, 1000, 1_010_000, 100)); // 10 s
        s.on_start_kyoku("E", 2, 0, 2_000_000.0);
        s.on_input(&batch(2_000_010.0, 50, 10));
        s.on_report(&report(8, 2000, 2_060_000, 100)); // another game's
        assert_eq!(s.status().kyoku, 0);
    }

    #[test]
    fn the_raw_line_carries_the_kyoku_and_our_windows() {
        let tmp = TempDir::new().unwrap();
        let mut s = session(&tmp);
        s.start(now());
        s.on_start_game(Some(1), Some(2));
        s.on_start_kyoku("E", 1, 0, 0.0);
        s.on_phase(true, 800.0);
        s.on_phase(false, 1200.0);
        s.on_phase(true, 1500.0);
        s.on_input(&batch(0.0, 100, 40));
        s.on_report(&LogReportSample {
            m: 1,
            t_ms: 150,
            g_ms: 300,
            ..report(1, 0, 60_000, 50)
        });
        let raw = &read_raw(&s.raw_files()[0])[0];
        assert_eq!(raw.own, vec![[800.0, 1200.0], [1500.0, 60_000.0]]);
        assert_eq!(
            (raw.frames.len(), raw.moves.len(), raw.buttons.len()),
            (100, 40, 1)
        );
        assert_eq!((raw.d, raw.m, raw.t_ms, raw.g_ms), (50, 1, 150, 300));
        assert!((raw.duration_s - 60.0).abs() < 1e-9);
        assert_eq!(s.reported_click_mode(), Some(ClickMode::Double));
    }

    #[test]
    fn live_counts_and_the_frame_ratio() {
        let mut s = Session::default();
        assert!(!s.live().in_kyoku);
        s.on_start_kyoku("E", 1, 0, 0.0);
        s.on_input(&batch(0.0, 100, 40));
        let live = s.live();
        assert!(live.in_kyoku);
        assert_eq!((live.page_frames, live.page_moving), (100, 40));
        s.on_report(&report(1, 0, 60_000, 20));
        assert!(
            s.live().frame_ratio.is_none(),
            "too few moving frames to read"
        );
        s.on_start_kyoku("E", 2, 0, 0.0);
        s.on_input(&batch(0.0, 100, 80));
        s.on_report(&report(1, 0, 60_000, 40));
        assert_eq!(s.live().frame_ratio, Some(0.5));
        assert!(!s.live().in_kyoku);
    }

    #[test]
    fn state_survives_a_restart_and_cancel_removes_raw_input() {
        let tmp = TempDir::new().unwrap();
        let mut s = session(&tmp);
        s.start(now());
        play_game(&mut s, 1, 2, 3, ("E", 1, 0), true);
        s.save().unwrap();
        let mut back = session(&tmp);
        assert!(back.active());
        assert_eq!(back.status(), s.status());
        let raw = back.raw_files();
        assert!(raw[0].exists());
        back.cancel();
        assert!(!raw[0].exists());
        assert!(!back.active());
        assert_eq!(back.games(), 0.0);
    }

    #[test]
    fn a_reconnect_continues_the_same_game() {
        let tmp = TempDir::new().unwrap();
        let mut s = session(&tmp);
        s.start(now());
        s.on_start_game(Some(5), Some(2));
        s.on_start_kyoku("E", 1, 0, 0.0);
        s.on_input(&batch(0.0, 100, 10));
        s.on_report(&report(5, 0, 60_000, 10));
        s.on_start_game(Some(5), Some(2));
        s.on_start_kyoku("E", 3, 0, 100_000.0);
        s.on_end_game(true);
        assert_eq!(s.games(), 1.0);
    }

    #[test]
    fn an_unloaded_session_writes_nothing() {
        let mut s = Session::default();
        s.start(now());
        s.on_start_game(Some(1), Some(2));
        s.on_start_kyoku("E", 1, 0, 0.0);
        s.on_input(&batch(0.0, 100, 10));
        s.on_report(&report(1, 0, 60_000, 10));
        s.save().unwrap();
        assert_eq!(s.status().kyoku, 1);
    }
}
