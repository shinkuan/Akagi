//! Calibration session: what the user's own games recorded, and the running
//! per-kyoku counters autoplay steers by.
//!
//! The counters run all the time (the cursor controller needs them while
//! autoplay plays); kyoku and games are kept, and raw input written, only
//! while a calibration is active.

use super::recorder::InputBatch;
use super::report::LogReportSample;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const REQUIRED_GAMES: f64 = 2.0;
pub const RECOMMENDED_GAMES: f64 = 5.0;
/// Shorter kyoku (an early ron) carry too few frames to say anything.
pub const MIN_KYOKU_S: f64 = 15.0;
const RECENT_CAP: usize = 50;
const STATE_FILE: &str = "state.json";
const RAW_DIR: &str = "raw";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KyokuRecord {
    pub chang: u32,
    pub ju: u32,
    pub ben: u32,
    pub duration_s: f64,
    pub d: u32,
    pub m: u32,
    pub t_ms: u32,
    pub g_ms: u32,
    pub page_frames: u32,
    pub page_moving: u32,
    pub own_frames: u32,
    pub own_moving: u32,
    pub other_frames: u32,
    pub other_moving: u32,
}

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
    pub kyoku: Vec<KyokuRecord>,
    pub raw_file: String,
}

impl GameRecord {
    /// How much this game counts towards the required games.
    pub fn weight(&self) -> f64 {
        if !(self.from_start && self.ended) || self.kyoku.is_empty() {
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
    pub version: u32,
    pub active: bool,
    pub started_at: String,
    pub session_id: String,
    pub games: Vec<GameRecord>,
    /// Moving-frame rate of recent autoplay kyoku, newest last.
    pub recent_autoplay: Vec<f64>,
}

/// One kyoku's raw page input, one JSON line in the game's raw file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawKyoku {
    pub chang: u32,
    pub ju: u32,
    pub ben: u32,
    pub d: u32,
    pub m: u32,
    pub rect: (f64, f64),
    pub frames: Vec<f64>,
    /// `[t_ms, x, y]`, canvas-normalised.
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
    pub recent_autoplay_rate: Option<f64>,
    pub recent_autoplay_kyoku: u32,
}

/// Counters for the kyoku in progress.
#[derive(Debug, Default)]
struct KyokuAcc {
    started_ms: f64,
    page_frames: u32,
    page_moving: u32,
    own_frames: u32,
    own_moving: u32,
    other_frames: u32,
    other_moving: u32,
    raw: RawKyoku,
}

/// Running totals of the kyoku in progress, for the cursor controller.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveCounts {
    pub started_ms: f64,
    pub page_frames: u32,
    pub page_moving: u32,
}

#[derive(Debug)]
pub struct Session {
    dir: PathBuf,
    file: SessionFile,
    current_game: Option<usize>,
    kyoku: Option<KyokuAcc>,
    /// Own-window transitions `(t_ms, own)` of the kyoku in progress.
    phases: Vec<(f64, bool)>,
    own_now: bool,
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
            current_game: None,
            kyoku: None,
            phases: Vec::new(),
            own_now: false,
        }
    }

    pub fn active(&self) -> bool {
        self.file.active
    }

    pub fn games(&self) -> f64 {
        self.file.games.iter().map(GameRecord::weight).sum()
    }

    /// Kyoku a fit may use.
    pub fn usable_kyoku(&self) -> Vec<KyokuRecord> {
        self.file
            .games
            .iter()
            .flat_map(|g| g.kyoku.iter().cloned())
            .collect()
    }

    pub fn raw_files(&self) -> Vec<PathBuf> {
        self.file
            .games
            .iter()
            .filter(|g| !g.raw_file.is_empty())
            .map(|g| self.raw_dir().join(&g.raw_file))
            .collect()
    }

    pub fn status(&self) -> CalibrationStatus {
        let recent = &self.file.recent_autoplay;
        let recording = self
            .current_game
            .and_then(|i| self.file.games.get(i))
            .is_some_and(|g| self.file.active && g.from_start);
        CalibrationStatus {
            active: self.file.active,
            started_at: self.file.started_at.clone(),
            games: self.games(),
            kyoku: self.file.games.iter().map(|g| g.kyoku.len() as u32).sum(),
            required_games: REQUIRED_GAMES,
            recommended_games: RECOMMENDED_GAMES,
            recording,
            recent_autoplay_rate: median(recent),
            recent_autoplay_kyoku: recent.len() as u32,
        }
    }

    pub fn live(&self) -> Option<LiveCounts> {
        self.kyoku.as_ref().map(|k| LiveCounts {
            started_ms: k.started_ms,
            page_frames: k.page_frames,
            page_moving: k.page_moving,
        })
    }

    /// Begin recording. A session already running is left as it is.
    pub fn start(&mut self, now: chrono::DateTime<chrono::Local>) {
        if self.file.active {
            return;
        }
        self.file.active = true;
        self.file.started_at = now.to_rfc3339();
        self.file.session_id = now.format("%Y%m%d-%H%M%S").to_string();
        self.file.games.clear();
        self.current_game = None;
    }

    /// Drop the session and its raw input.
    pub fn cancel(&mut self) {
        for path in self.raw_files() {
            let _ = fs::remove_file(path);
        }
        self.file.active = false;
        self.file.games.clear();
        self.current_game = None;
    }

    /// The fit is written; stop recording. Raw input stays on disk.
    pub fn finish(&mut self) {
        self.file.active = false;
        self.file.games.clear();
        self.current_game = None;
    }

    pub fn on_start_game(&mut self, game_id: Option<u64>, match_mode: Option<u8>) {
        self.kyoku = None;
        self.current_game = None;
        if !self.file.active {
            return;
        }
        let Some(game_id) = game_id else { return };
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
        self.current_game = Some(self.file.games.len() - 1);
    }

    pub fn on_start_kyoku(&mut self, bakaze: &str, kyoku: u8, honba: u8, now_ms: f64) {
        self.kyoku = Some(KyokuAcc {
            started_ms: now_ms,
            ..KyokuAcc::default()
        });
        self.phases.clear();
        self.phases.push((now_ms, self.own_now));
        if let Some(g) = self.current_game.and_then(|i| self.file.games.get_mut(i)) {
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

    fn own_at(&self, t_ms: f64) -> bool {
        match self.phases.partition_point(|(t, _)| *t <= t_ms) {
            0 => self.phases.first().is_some_and(|p| p.1),
            i => self.phases[i - 1].1,
        }
    }

    pub fn on_input(&mut self, batch: &InputBatch) {
        let Some(start) = self.kyoku.as_ref().map(|k| k.started_ms) else {
            return;
        };
        let record_raw = self.recording_current();
        let mut moving = batch.moves.iter().map(|m| m.t_ms).peekable();
        let mut counts = Vec::with_capacity(batch.frames.len());
        for &t in batch.frames.iter().filter(|t| **t >= start) {
            // A move belongs to the frame that starts at its time.
            let mut moved = false;
            while moving.peek().is_some_and(|mt| *mt <= t + 0.05) {
                moved |= (moving.next().unwrap_or(f64::NAN) - t).abs() <= 0.05;
            }
            counts.push((self.own_at(t), moved));
        }
        let k = self.kyoku.as_mut().expect("checked above");
        for (own, moved) in counts {
            k.page_frames += 1;
            k.page_moving += u32::from(moved);
            if own {
                k.own_frames += 1;
                k.own_moving += u32::from(moved);
            } else {
                k.other_frames += 1;
                k.other_moving += u32::from(moved);
            }
        }
        if record_raw {
            k.raw.rect = batch.rect;
            k.raw
                .frames
                .extend(batch.frames.iter().filter(|t| **t >= start));
            k.raw.moves.extend(
                batch
                    .moves
                    .iter()
                    .filter(|m| m.t_ms >= start)
                    .map(|m| [m.t_ms, m.x, m.y]),
            );
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

    fn recording_current(&self) -> bool {
        self.file.active
            && self
                .current_game
                .and_then(|i| self.file.games.get(i))
                .is_some()
    }

    /// The kyoku's report: close it. Returns the page-to-client frame ratio it
    /// showed, when there were enough moving frames to tell. `autoplay`
    /// says whether our cursor played it (kept for the UI's comparison).
    pub fn on_report(&mut self, r: &LogReportSample, autoplay: bool) -> Option<f64> {
        let k = self.kyoku.take()?;
        let own = own_intervals(&self.phases, k.started_ms, r.received_ms as f64);
        self.phases.clear();
        let duration_s = r.duration_s();
        let ratio = (k.page_moving >= 50).then(|| f64::from(r.d) / f64::from(k.page_moving));
        if autoplay && !self.file.active {
            if let Some(rate) = r.rate() {
                self.file.recent_autoplay.push(rate);
                let excess = self.file.recent_autoplay.len().saturating_sub(RECENT_CAP);
                self.file.recent_autoplay.drain(..excess);
            }
        }
        let Some(gi) = self.current_game else {
            return ratio;
        };
        let same_game = self
            .file
            .games
            .get(gi)
            .is_some_and(|g| r.game_id.is_none_or(|id| id == g.game_id));
        if !self.file.active || !same_game || duration_s < MIN_KYOKU_S || k.page_frames == 0 {
            return ratio;
        }
        let record = KyokuRecord {
            chang: r.chang,
            ju: r.ju,
            ben: r.ben,
            duration_s,
            d: r.d,
            m: r.m,
            t_ms: r.t_ms,
            g_ms: r.g_ms,
            page_frames: k.page_frames,
            page_moving: k.page_moving,
            own_frames: k.own_frames,
            own_moving: k.own_moving,
            other_frames: k.other_frames,
            other_moving: k.other_moving,
        };
        let raw = RawKyoku {
            chang: r.chang,
            ju: r.ju,
            ben: r.ben,
            d: r.d,
            m: r.m,
            own,
            ..k.raw
        };
        let game = &mut self.file.games[gi];
        game.kyoku.push(record);
        let path = self.dir.join(RAW_DIR).join(&game.raw_file);
        if let Err(e) = append_line(&path, &raw) {
            tracing::warn!("mouse calibration: could not write {}: {e}", path.display());
        }
        ratio
    }

    /// `confirmed`: the game reached its final result.
    pub fn on_end_game(&mut self, confirmed: bool) {
        if let Some(g) = self.current_game.and_then(|i| self.file.games.get_mut(i)) {
            g.ended = confirmed;
        }
        self.current_game = None;
        self.kyoku = None;
    }

    pub fn save(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{STATE_FILE}.tmp"));
        fs::write(&tmp, serde_json::to_vec_pretty(&self.file)?)?;
        fs::rename(tmp, self.dir.join(STATE_FILE))
    }

    fn raw_dir(&self) -> PathBuf {
        self.dir.join(RAW_DIR)
    }
}

fn append_line<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    f.write_all(&line)
}

/// Own-window intervals between `start` and `end` from the transition log.
fn own_intervals(phases: &[(f64, bool)], start: f64, end: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut open: Option<f64> = None;
    for &(t, own) in phases {
        match (own, open) {
            (true, None) => open = Some(t.max(start)),
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

pub fn median(values: &[f64]) -> Option<f64> {
    quantile(values, 0.5)
}

/// Linear-interpolated quantile; `None` for no values.
pub fn quantile(values: &[f64], q: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let pos = q.clamp(0.0, 1.0) * (v.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    Some(v[lo] + (v[hi] - v[lo]) * (pos - lo as f64))
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
    use crate::autoplay::mouse::recorder::{Button, Move};
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

    /// `frames` evenly spaced frames from `t0`, the first `moving` of them moving.
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
            s.on_report(
                &report(
                    id,
                    (start_ms / 1000.0) as i64,
                    start_ms as i64 + 60_000,
                    900,
                ),
                false,
            );
        }
        s.on_end_game(end);
    }

    #[test]
    fn games_count_by_length_and_completeness() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
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
    }

    #[test]
    fn nothing_is_kept_outside_a_session() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
        play_game(&mut s, 1, 2, 8, ("E", 1, 0), true);
        assert_eq!(s.status().kyoku, 0);
        assert!(s.raw_files().is_empty());
    }

    #[test]
    fn short_kyoku_and_other_games_reports_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
        s.start(now());
        s.on_start_game(Some(9), Some(2));
        s.on_start_kyoku("E", 1, 0, 1_000_000.0);
        s.on_input(&batch(1_000_010.0, 50, 10));
        s.on_report(&report(9, 1000, 1_010_000, 100), false); // 10 s
        s.on_start_kyoku("E", 2, 0, 2_000_000.0);
        s.on_input(&batch(2_000_010.0, 50, 10));
        s.on_report(&report(8, 2000, 2_060_000, 100), false); // another game's
        assert_eq!(s.status().kyoku, 0);
    }

    #[test]
    fn frames_split_by_our_decision_window() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
        s.start(now());
        s.on_start_game(Some(1), Some(2));
        s.on_start_kyoku("E", 1, 0, 0.0);
        // Frames at 0,16,..,1584; our window covers [800, 1200).
        s.on_phase(true, 800.0);
        s.on_phase(false, 1200.0);
        s.on_input(&batch(0.0, 100, 100));
        s.on_report(&report(1, 0, 60_000, 50), false);
        let k = &s.usable_kyoku()[0];
        assert_eq!(k.page_frames, 100);
        assert_eq!(k.own_frames, 25);
        assert_eq!(k.own_moving, 25);
        assert_eq!(k.other_frames, 75);
        let raw = read_raw(&s.raw_files()[0]);
        assert_eq!(raw[0].own, vec![[800.0, 1200.0]]);
        assert_eq!(raw[0].frames.len(), 100);
        assert_eq!(raw[0].buttons.len(), 1);
    }

    #[test]
    fn the_frame_ratio_needs_enough_movement() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
        s.on_start_kyoku("E", 1, 0, 0.0);
        s.on_input(&batch(0.0, 100, 40));
        assert!(s.on_report(&report(1, 0, 60_000, 20), false).is_none());
        s.on_start_kyoku("E", 2, 0, 0.0);
        s.on_input(&batch(0.0, 100, 80));
        let r = s.on_report(&report(1, 0, 60_000, 40), false).unwrap();
        assert!((r - 0.5).abs() < 1e-9);
    }

    #[test]
    fn autoplay_kyoku_feed_the_recent_rates() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
        for _ in 0..(RECENT_CAP + 5) {
            s.on_start_kyoku("E", 1, 0, 0.0);
            s.on_report(&report(1, 0, 100_000, 1000), true);
        }
        let st = s.status();
        assert_eq!(st.recent_autoplay_kyoku, RECENT_CAP as u32);
        assert!((st.recent_autoplay_rate.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn state_survives_a_restart_and_cancel_removes_raw_input() {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::load(tmp.path().to_path_buf());
        s.start(now());
        play_game(&mut s, 1, 2, 3, ("E", 1, 0), true);
        s.save().unwrap();
        let mut back = Session::load(tmp.path().to_path_buf());
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
        let mut s = Session::load(tmp.path().to_path_buf());
        s.start(now());
        s.on_start_game(Some(5), Some(2));
        s.on_start_kyoku("E", 1, 0, 0.0);
        s.on_input(&batch(0.0, 100, 10));
        s.on_report(&report(5, 0, 60_000, 10), false);
        // Reconnect: same game, joined at East 3.
        s.on_start_game(Some(5), Some(2));
        s.on_start_kyoku("E", 3, 0, 100_000.0);
        s.on_end_game(true);
        assert_eq!(s.games(), 1.0);
    }

    #[test]
    fn quantiles_interpolate() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(quantile(&[0.0, 10.0], 0.25), Some(2.5));
        assert_eq!(median(&[]), None);
    }
}
