//! Auto-start / auto-continue manager.
//!
//! Queues Ranked matches, plays a target number of games, then stops.
//! It reuses the autoplay plumbing: the shared [`AutoplayContext`] page handle
//! and the CDP click/scroll primitives. Navigation is Ranked lobby → tier →
//! mode (4P East / 4P South / 3P East / 3P South); there is no separate "start
//! matchmaking" button.
//!
//! The hard part is the post-game return to the lobby: the result screens vs
//! the lobby home are invisible to WS and to Majsoul's in-page JS, so the
//! manager uses a visual check ([`majsoul::vision`]) on the Ranked label to
//! know when it's back at the lobby. Success of a re-queue is confirmed by the
//! next [`MjaiEvent::StartGame`] (the game actually beginning) — no bespoke WS
//! plumbing needed.
//!
//! A session may only be STARTED at the lobby with autoplay on: the
//! autostart_start command refuses mid-game (via the published `in_game` flag)
//! and without autoplay (nothing would play the queued games), and the manager
//! stops the session if autoplay is switched off while it runs. Stop works at
//! any time — the current game just finishes without queueing another.
//!
//! Lifecycle (a tick-driven state machine over `MjaiBus` + a ~0.5s timer):
//! - `Seeking` — trying to queue the next game. `clearing` = we may press
//!   Confirm to advance post-game result screens. A cold start (fresh Start
//!   press) queues immediately — the press at the lobby is the ground truth;
//!   only the stale-stream recovery seek waits for a visually confirmed lobby.
//! - `InGame` — a game is running; do nothing.
//! - `Stopped` — reached the target or gave up; idle until relaunch.
//!
//! Failure modes are silent-by-design (mirrors autoplay): no page handle
//! (chromium backend not running) or a failed screenshot just skips the step.
//!
//! Platform seam: the manager itself (state machine, session lifecycle,
//! notifications) is platform-agnostic; everything Majsoul-specific it
//! consumes lives behind `crate::autoplay::majsoul` — the lobby coordinates
//! and room-selection mapping ([`lobby_coords`](lc)) and the visual
//! home-anchor fingerprinting ([`vision`]). Supporting another platform
//! (e.g. Tenhou) means providing that platform's counterparts (nav
//! coordinates + selection mapping, a static home anchor for the
//! return-to-lobby check, an `autostart.<platform>.*` i18n vocabulary and a
//! frontend option group) and dispatching on the configured platform in
//! [`AutoStartManager::run_queue_nav`] / [`AutoStartManager::vision_at_lobby`].

use crate::autoplay::cdp_input::{dispatch_click, dispatch_scroll, evaluate_canvas_rect};
use crate::autoplay::context::AutoplayContext;
use crate::autoplay::majsoul::lobby_coords as lc;
use crate::autoplay::majsoul::vision;
use crate::config::{AppConfig, AutoStartConfig, RoomSelection};
use crate::event_bus::{MjaiBus, NotifyBus};
use crate::schema::{MjaiEvent, Notification};
use anyhow::{anyhow, Result};
use chromiumoxide::page::Page;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast::error::RecvError, RwLock};
use tracing::{debug, info, warn};

/// Tick period for the return-to-lobby driver.
const TICK: Duration = Duration::from_millis(500);
/// No-vision fallback: press Confirm this many times before assuming the lobby.
const NO_VISION_CONFIRM: u32 = 5;
/// Confirm presses after which the result screens are certainly gone. If vision
/// still can't confirm the lobby by then, the session gives up and stops —
/// it never navigates blindly. (Pressing Start at the lobby re-captures the
/// reference, so a stale/poisoned one heals on the next manual start.)
const MAX_CLEAR_CONFIRMS: u32 = 20;
/// If no mjai event arrives for this long while we believe a game is running,
/// the stream is considered dead (disconnect/page reload — EndGame will never
/// arrive) and `in_game` is reset so the Start gate and active sessions don't
/// wait forever on a phantom game.
const STALE_GAME_TIMEOUT: Duration = Duration::from_secs(300);
/// Hover/hold used for lobby clicks (same as the autoplay defaults).
const HOVER_MS: u32 = 150;
const HOLD_MS: u32 = 50;
/// Pacing while a queued game is awaited, and while waiting for a visual
/// lobby confirmation (recovery / post-timeout waits).
const AWAIT_START_POLL: Duration = Duration::from_millis(1000);
const LOBBY_RECHECK_INTERVAL: Duration = Duration::from_millis(1500);
/// Consecutive nav/click infrastructure failures (paced at
/// `confirm_interval_ms`, 2.5s default) before the session gives up — ~150s,
/// comfortably outlasting a routine Majsoul reload/reconnect (30–90s with no
/// page handle) while still ending a session whose browser is simply gone.
const NAV_ERROR_CAP: u32 = 60;

/// Sidecar file (next to config.toml) holding the calibrated lobby-home region
/// fingerprint. Written by the `lobby_calibrate_home` command (and by the
/// manager's auto-calibration); shared with `ipc::commands`.
pub(crate) fn lobby_home_ref_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("autostart_lobby_home.fp")
}

#[derive(Clone, Copy)]
struct Seeking {
    /// Re-queue attempts used so far (a "queued but no StartGame in time"
    /// counts as one).
    attempt: u32,
    /// Don't act before this instant (paces the steps).
    next_step_at: Instant,
    /// `Some` once queue navigation has been issued: wait for StartGame until
    /// this deadline, else retry.
    awaiting_start_until: Option<Instant>,
    /// Confirm presses issued this attempt round (the no-vision heuristic and
    /// the give-up cap key off it; only presses that actually dispatched count).
    confirm_presses: u32,
    /// Whether we may press Confirm to clear result screens (post-game). False
    /// on cold start and stale-stream recovery, which never press Confirm.
    clearing: bool,
    /// Whether the user's Start press vouches for this seek's starting screen.
    /// True for user-initiated seeks (cold start queues immediately — the
    /// press at the lobby IS the ground truth) and post-game returns; false
    /// for stale-stream recovery, where the screen may be a live game — those
    /// act only on a visually confirmed lobby.
    trust_blind: bool,
    /// Consecutive infrastructure failures (nav or Confirm click errors —
    /// typically "no page handle" during a Majsoul reload/reconnect). Counted
    /// separately from `attempt`: a 30–90s page outage must neither burn the
    /// matchmaking budget nor forfeit the Start press's screen vouch. Reset on
    /// any success; at [`NAV_ERROR_CAP`] the session stops (page gone for
    /// good, e.g. browser closed).
    nav_errors: u32,
    /// The Start press's screen vouch only covers the screen AS PRESSED. Once a
    /// nav has dispatched a click, the screen may already be mutated (tier
    /// list open), so an error after that point forfeits reference CAPTURE
    /// for the rest of this seek — queueing itself still proceeds; only a
    /// visually confirmed lobby may capture again.
    capture_forfeited: bool,
}

enum Phase {
    Seeking(Seeking),
    InGame,
    Stopped,
}

/// Runtime control shared between the manual Start/Stop IPC commands (see
/// `ipc::commands::autostart_start` / `autostart_stop`) and the manager.
/// `active` gates the whole loop; bumping `epoch` starts a FRESH session
/// (resets the counter; refused outright if a game is running — sessions may
/// only start at the lobby); `games_done` and `in_game` are published for the
/// UI/commands to poll (`in_game` lets autostart_start refuse and the
/// GameDashboard grey out Start mid-game).
#[derive(Default)]
pub struct AutoStartControl {
    pub active: AtomicBool,
    pub epoch: AtomicU64,
    pub games_done: AtomicU32,
    pub in_game: AtomicBool,
}

pub struct AutoStartManager {
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    mjai_bus: MjaiBus,
    notify_bus: NotifyBus,
    config_path: Arc<PathBuf>,
    control: Arc<AutoStartControl>,
    phase: Phase,
    games_played: u32,
    our_seat: Option<u8>,
    /// Last session epoch seen; a change (manual Start) resets the session.
    last_epoch: u64,
    /// True between StartGame and EndGame, tracked regardless of `active` and
    /// mirrored to `AutoStartControl::in_game` so Start can be refused (and the
    /// button greyed out) while a game is running.
    in_game: bool,
    /// Instant of the last mjai event of any kind — used with `in_game` to
    /// detect a dead game stream (see [`STALE_GAME_TIMEOUT`]).
    last_mjai_at: Option<Instant>,
    /// Cached lobby-home reference fingerprint (loaded lazily from the sidecar).
    home_ref: Option<Vec<u8>>,
    /// Fingerprint captured at queue-nav time. Persisted as the lobby reference
    /// only once StartGame confirms the queue actually worked — proof we really
    /// were at the lobby — so a nav issued on a wrong screen (e.g. Start pressed
    /// elsewhere) can't poison the saved reference.
    pending_home_ref: Option<Vec<u8>>,
    /// Whether THIS session has committed at least one matchmaking queue (a
    /// nav's mode click was dispatched). Monotone within a session; reset on a
    /// fresh epoch. A StartGame reaching an active session that never queued
    /// can only be a manually queued game racing the Start press — refused. A
    /// session that HAS queued adopts late arrivals too: a match landing after
    /// the awaiting timeout, or the bridge re-emitting StartGame when the
    /// client reconnects to the same game (stale-stream recovery).
    queued_this_session: bool,
    /// Whether the CURRENT nav dispatched at least one click before failing —
    /// maintained by `run_queue_nav`, read by drive_seeking's Err handler to
    /// forfeit the seek's capture vouch (after a click the screen is no longer
    /// the one the user pressed Start on).
    nav_clicked: bool,
}

impl AutoStartManager {
    pub fn new(
        cfg: Arc<RwLock<AppConfig>>,
        ctx: Arc<AutoplayContext>,
        mjai_bus: MjaiBus,
        notify_bus: NotifyBus,
        config_path: Arc<PathBuf>,
        control: Arc<AutoStartControl>,
    ) -> Self {
        // Idle until a session is started (epoch bumped) via Start or a cold-start
        // enable. `last_epoch = 0` matches the initial epoch so no spurious reset.
        Self {
            cfg,
            ctx,
            mjai_bus,
            notify_bus,
            config_path,
            control,
            phase: Phase::Stopped,
            games_played: 0,
            our_seat: None,
            last_epoch: 0,
            in_game: false,
            last_mjai_at: None,
            home_ref: None,
            pending_home_ref: None,
            queued_this_session: false,
            nav_clicked: false,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut mjai_rx = self.mjai_bus.subscribe();
        info!("autostart manager started");
        loop {
            tokio::select! {
                msg = mjai_rx.recv() => match msg {
                    Ok(ev) => self.handle_event(&ev),
                    Err(RecvError::Lagged(n)) => warn!("autostart: mjai bus lagged {n}"),
                    Err(RecvError::Closed) => {
                        info!("autostart: mjai bus closed; exiting");
                        return Ok(());
                    }
                },
                _ = tokio::time::sleep(TICK) => self.on_tick().await,
            }
        }
    }

    /// Set the in-game flag and publish it to the shared control so the
    /// autostart_start command / GameDashboard can refuse Start mid-game.
    fn set_in_game(&mut self, v: bool) {
        self.in_game = v;
        self.control.in_game.store(v, Ordering::SeqCst);
    }

    fn handle_event(&mut self, ev: &MjaiEvent) {
        self.last_mjai_at = Some(Instant::now());
        // Load `active` BEFORE syncing the epoch: autostart_start publishes
        // the new epoch before flipping active on, so under the SeqCst total
        // order an active==true observed here guarantees sync_session_epoch
        // sees the new epoch — an event racing a just-pressed Start can never
        // run new-session logic on the previous session's stale counters.
        let active = self.control.active.load(Ordering::SeqCst);
        self.sync_session_epoch();
        match ev {
            MjaiEvent::StartGame { id, .. } => {
                // A game began — this doubles as re-queue success confirmation.
                self.set_in_game(true);
                self.our_seat = *id;
                if active
                    && matches!(&self.phase, Phase::Seeking(s) if s.awaiting_start_until.is_some())
                {
                    // Our queue nav led to this game while we were expecting
                    // it, so the fingerprint the nav captured is a trustworthy
                    // lobby reference — persist it.
                    self.persist_pending_reference();
                } else if !self.queued_this_session
                    && matches!(self.phase, Phase::Seeking(_))
                    && self.control.active.load(Ordering::SeqCst)
                {
                    // A session that never queued anything cannot own a game:
                    // this is a manually queued match racing the Start press
                    // past the command's in_game gate (re-load `active` — the
                    // press's store may have landed after this handler's
                    // snapshot). Sessions may only start at the lobby, so
                    // refuse rather than adopt. A session that HAS queued
                    // adopts instead — late matches (after the awaiting
                    // timeout) and reconnect re-emits of its own game are
                    // still its games.
                    self.stop_session(
                        "foreign_game",
                        None,
                        "Detected a game this session didn't queue; stopped. \
                         Press Start at the lobby after this game ends."
                            .to_string(),
                    );
                }
                // Unconditionally: any Seeking/Stopped phase is wrong while a
                // game is running. Inert for inactive sessions, and closes the
                // race where a session start lands between this handler's two
                // atomic loads — an active Seeking left standing here would
                // blind-navigate into the live game after its idle fallback.
                self.phase = Phase::InGame;
            }
            MjaiEvent::EndGame => {
                self.set_in_game(false);
                if active {
                    self.on_end_game();
                }
            }
            _ => {}
        }
    }

    /// A bumped epoch (manual Start) starts a FRESH session: reset the counter
    /// and queue now if idle at the lobby. A game already running refuses the
    /// session outright — Start is only allowed at the lobby. Called from both
    /// the tick and the event path so whichever observes the new epoch first
    /// applies the reset.
    fn sync_session_epoch(&mut self) {
        let epoch = self.control.epoch.load(Ordering::SeqCst);
        if epoch == self.last_epoch {
            return;
        }
        self.last_epoch = epoch;
        self.games_played = 0;
        self.control.games_done.store(0, Ordering::SeqCst);
        // A fresh session must not inherit a fingerprint captured by an older
        // session's nav — there is no evidence about where that nav ran — nor
        // an older session's queued-a-game credit.
        self.pending_home_ref = None;
        self.queued_this_session = false;
        // A Start-then-quick-Stop can land here with active already false again
        // — still reset the (idle) phase, but don't announce a session that the
        // user just cancelled.
        let active = self.control.active.load(Ordering::SeqCst);
        self.phase = if self.in_game {
            if active {
                // Starting is not allowed mid-game: autostart_start already
                // gates on the published in_game flag, so reaching here means a
                // game began inside the tiny window between that check and the
                // epoch/active stores. Refuse rather than arm.
                self.stop_session(
                    "refused_in_game",
                    None,
                    "A game is in progress — can't start a session now. \
                     Press Start at the lobby after this game ends."
                        .to_string(),
                );
            }
            Phase::InGame
        } else {
            if active {
                self.notify("queueing", None, "Queueing for a match.".to_string());
            }
            Phase::Seeking(Seeking {
                attempt: 0,
                next_step_at: Instant::now(),
                awaiting_start_until: None,
                confirm_presses: 0,
                clearing: false,
                trust_blind: true,
                nav_errors: 0,
                capture_forfeited: false,
            })
        };
    }

    fn on_end_game(&mut self) {
        // Count the finished game. `our_seat` is captured from StartGame.id and
        // is None for spectator/replay streams.
        let (count_only_seat, target, settle_ms) = self
            .cfg
            .try_read()
            .map(|c| {
                (
                    c.autostart.count_only_our_seat,
                    c.autostart.target_game_count,
                    c.autostart.settle_delay_ms,
                )
            })
            .unwrap_or_else(|_| {
                // Contended config lock: fall back to the real defaults rather
                // than a duplicated literal that could drift from them.
                let d = AutoStartConfig::default();
                (
                    d.count_only_our_seat,
                    d.target_game_count,
                    d.settle_delay_ms,
                )
            });
        if !count_only_seat || self.our_seat.is_some() {
            self.games_played += 1;
        }
        self.control
            .games_done
            .store(self.games_played, Ordering::SeqCst);
        if target > 0 && self.games_played >= target {
            self.phase = self.stop_session(
                "target_reached",
                Some(serde_json::json!({ "count": self.games_played })),
                format!("Finished {} game(s); stopping.", self.games_played),
            );
            return;
        }
        // Otherwise return to the lobby and re-queue. The settle delay gives
        // the first result screen time to appear before the first Confirm press.
        self.phase = Phase::Seeking(Seeking {
            attempt: 0,
            next_step_at: Instant::now() + Duration::from_millis(settle_ms as u64),
            awaiting_start_until: None,
            confirm_presses: 0,
            clearing: true,
            trust_blind: true,
            nav_errors: 0,
            capture_forfeited: false,
        });
    }

    async fn on_tick(&mut self) {
        let (cfg, autoplay_on) = {
            let c = self.cfg.read().await;
            (c.autostart.clone(), c.autoplay.enabled)
        };
        // A dead event stream while "in a game" USUALLY means the client
        // disconnected or reloaded — EndGame will never arrive, so reset
        // before it wedges the Start gate (refused forever) or an active session.
        // But the silence can also be a live game whose events merely stopped
        // flowing (event tap died, extreme thinking timers), so recovery must
        // never click blindly: it waits for VISION to confirm the lobby
        // (trust_blind=false), and stops outright when vision can't be used.
        if self.in_game
            && self
                .last_mjai_at
                .is_some_and(|t| t.elapsed() >= STALE_GAME_TIMEOUT)
        {
            warn!(
                "autostart: no mjai events for {STALE_GAME_TIMEOUT:?} while in-game; assuming the game is gone"
            );
            self.set_in_game(false);
            self.our_seat = None;
            if self.control.active.load(Ordering::SeqCst) && matches!(self.phase, Phase::InGame) {
                if cfg.use_vision && self.home_reference().is_some() {
                    self.notify(
                        "stream_lost_waiting",
                        None,
                        "Game event stream lost; waiting to visually confirm \
                         the lobby before re-queueing."
                            .to_string(),
                    );
                    self.phase = Phase::Seeking(Seeking {
                        attempt: 0,
                        next_step_at: Instant::now(),
                        awaiting_start_until: None,
                        confirm_presses: 0,
                        clearing: false,
                        trust_blind: false,
                        nav_errors: 0,
                        capture_forfeited: false,
                    });
                } else {
                    self.phase = self.stop_session(
                        "stream_lost_stopped",
                        None,
                        "Game event stream lost and the current screen can't \
                         be safely confirmed; stopped."
                            .to_string(),
                    );
                }
            }
        }
        // Session control: a bumped epoch (manual Start) resets the session —
        // queue now if idle; a game already running refuses it (sessions may
        // only start at the lobby).
        self.sync_session_epoch();
        if !self.control.active.load(Ordering::SeqCst) {
            return;
        }
        // Autostart rides on autoplay to actually play the games it queues;
        // with autoplay turned off mid-session, every further game would sit
        // unplayed — stop instead. (autostart_start refuses to begin without
        // autoplay; this catches it being switched off while running.)
        if !autoplay_on {
            self.phase = self.stop_session(
                "autoplay_off",
                None,
                "Autoplay was turned off; stopped.".to_string(),
            );
            return;
        }
        // Invariant repair: an active session can't be "InGame" without a
        // running game — that combination only arises when an EndGame raced
        // the session start and its handling was consumed under active=false.
        // Treat it as the game end it was.
        if matches!(self.phase, Phase::InGame) && !self.in_game {
            warn!("autostart: InGame phase without a running game; treating as game end");
            self.on_end_game();
        }
        // The mirror invariant: an active session that never queued anything
        // cannot legitimately be riding a game — that combination means a
        // manually queued game raced the Start press through both refusal
        // checks. Refuse it here, the backstop for interleavings the event
        // path can't observe.
        if matches!(self.phase, Phase::InGame) && self.in_game && !self.queued_this_session {
            self.stop_session(
                "foreign_game",
                None,
                "Detected a game this session didn't queue; stopped. \
                 Press Start at the lobby after this game ends."
                    .to_string(),
            );
            return;
        }
        // Terminal target check (also re-checked here so it fires even mid-seek).
        if cfg.target_game_count > 0
            && self.games_played >= cfg.target_game_count
            && !matches!(self.phase, Phase::Stopped)
        {
            if !matches!(self.phase, Phase::InGame) {
                self.phase = self.stop_session(
                    "target_reached_idle",
                    Some(serde_json::json!({ "count": self.games_played })),
                    format!(
                        "Finished {} game(s); no further queueing.",
                        self.games_played
                    ),
                );
            }
            return;
        }
        let seeking = match self.phase {
            Phase::Seeking(s) => s,
            _ => return,
        };
        self.phase = self.drive_seeking(&cfg, seeking).await;
    }

    /// One step of the return-to-lobby / re-queue loop. Returns the next phase.
    async fn drive_seeking(&mut self, cfg: &AutoStartConfig, mut s: Seeking) -> Phase {
        let now = Instant::now();
        if now < s.next_step_at {
            return Phase::Seeking(s);
        }

        // Waiting for the queued game to actually begin (StartGame flips us to
        // InGame in handle_event). Here we only handle the timeout → retry.
        if let Some(deadline) = s.awaiting_start_until {
            if now >= deadline {
                s.attempt += 1;
                if s.attempt > cfg.max_attempts {
                    return self.stop_session(
                        "matchmaking_failed",
                        None,
                        "Matchmaking kept failing; stopped.".to_string(),
                    );
                }
                warn!("autostart: matchmaking timed out, retry {}", s.attempt);
                // The nav that opened this window didn't lead to a game, so
                // its captured fingerprint is untrustworthy — drop it rather
                // than risk pairing it with some later StartGame.
                self.pending_home_ref = None;
                s.awaiting_start_until = None;
                s.confirm_presses = 0;
                // The nav that timed out already mutated the screen (its mode
                // click dispatched), so ANY further blind action — pressing
                // Confirm on a clearing seek, or re-running the nav on a
                // cold-start seek whose Start-press vouch is long spent — is
                // blind clicking on an unknown screen (likely the matchmaking
                // overlay). With vision usable, convert to a pure vision
                // wait: a late match is still adopted if it lands
                // (queued_this_session), otherwise a visually confirmed lobby
                // restarts the queue. The blind retry survives only where
                // vision genuinely can't confirm: explicit no-vision mode
                // (the user opted into blind presses; stopping here would
                // kill the session on the very first slow queue and
                // dead-letter max_attempts) and a fresh install whose first
                // ever queue timed out before persisting a reference (the
                // bounded retry is the only self-heal available there).
                if cfg.use_vision && (s.clearing || self.home_reference().is_some()) {
                    s.clearing = false;
                    s.trust_blind = false;
                    self.notify(
                        "timeout_waiting",
                        None,
                        "Matchmaking timed out; watching for the lobby (or a \
                         late match) to continue."
                            .to_string(),
                    );
                }
                s.next_step_at = now + Duration::from_millis(cfg.confirm_interval_ms as u64);
            } else {
                s.next_step_at = now + AWAIT_START_POLL;
            }
            return Phase::Seeking(s);
        }

        // Recovery seeks (trust_blind=false) may only act on a visual lobby
        // confirmation; if vision has become unusable (toggled off, reference
        // gone), idling forever would wedge the session — stop it the same way
        // the recovery entry point does.
        if !s.trust_blind && (!cfg.use_vision || self.home_reference().is_none()) {
            return self.stop_session(
                "vision_unusable",
                None,
                "Can't safely confirm the current screen (vision unavailable); stopped."
                    .to_string(),
            );
        }

        // "Are we at the lobby?" A cold start straight from a Start press
        // trusts the press itself — the user pressed it at the lobby, and the
        // queue nav re-captures the reference there (which also heals a stale
        // one). Everything else needs a confirmation: vision when available,
        // or the blind press-Confirm-N-times heuristic in explicit no-vision
        // mode. There is NO blind navigation — when the lobby can't be
        // confirmed, the give-up paths below stop the session instead.
        // `vision_ok` remembers whether THIS step actually confirmed the
        // lobby visually — it gates fingerprint capture below (a trust_blind
        // retry runs on an unverified screen).
        let mut vision_ok = false;
        let is_lobby = if !s.clearing {
            if s.trust_blind {
                true
            } else {
                vision_ok = self.vision_at_lobby(cfg).await;
                vision_ok
            }
        } else if !cfg.use_vision {
            // Explicit blind mode: press Confirm a few times, then assume the lobby.
            s.confirm_presses >= NO_VISION_CONFIRM
        } else if self.home_reference().is_none() {
            // Vision on but no reference yet (the first queue nav never got
            // to persist one): the lobby can never be confirmed — fall
            // through to the small no-reference give-up below; the user
            // re-presses Start at the lobby, which captures the reference and
            // unblocks all later sessions.
            false
        } else {
            vision_ok = self.vision_at_lobby(cfg).await;
            vision_ok
        };

        if is_lobby {
            let room = cfg.resolve_room(None, None); // rank hook: filled once rank-read lands

            // Capture a reference fingerprint only when this screen is
            // vouched for: the user's Start press (first attempt, and no
            // earlier nav of this seek clicked anything) or a visual
            // confirmation. A timeout-retry nav runs on an unverified screen
            // (likely still matchmaking), and a retry after a mid-nav error
            // runs on a screen the failed nav itself mutated — in both cases
            // a wrong screen must not get persisted as the lobby reference.
            let allow_capture = vision_ok || (s.attempt == 0 && !s.capture_forfeited);
            match self.run_queue_nav(cfg, room, allow_capture).await {
                Ok(true) => {
                    info!("autostart: queued {:?}", room);
                    // Matchmaking is committed (mode click dispatched): from
                    // here on any game that starts is plausibly ours, however
                    // late it lands.
                    self.queued_this_session = true;
                    s.nav_errors = 0;
                    s.awaiting_start_until = Some(
                        Instant::now() + Duration::from_millis(cfg.matchmaking_timeout_ms as u64),
                    );
                    s.next_step_at = Instant::now() + AWAIT_START_POLL;
                }
                Ok(false) => {
                    // Stop pressed (or autoplay switched off) mid-navigation;
                    // the nav aborted before the mode click, so no game was
                    // queued — and the fingerprint it captured dies with the
                    // abort.
                    info!("autostart: queue nav aborted (Stop or autoplay off)");
                    self.pending_home_ref = None;
                    if self.control.active.load(Ordering::SeqCst) {
                        // Still active ⇒ the abort came from autoplay being
                        // switched off, not Stop. Stop formally RIGHT HERE —
                        // deferring to the next tick's guard would leave a
                        // zombie active-but-Stopped session if autoplay is
                        // toggled back on within the window.
                        return self.stop_session(
                            "autoplay_off",
                            None,
                            "Autoplay was turned off; stopped.".to_string(),
                        );
                    }
                    return Phase::Stopped;
                }
                Err(e) => {
                    // Infrastructure failure (typically "no page handle"
                    // during a Majsoul reload). Counted on its own budget —
                    // NOT `attempt` — so a 30–90s outage neither burns the
                    // matchmaking budget nor (via `allow_capture`) forfeits
                    // the Start press's screen vouch; but a page that's gone
                    // for good must not leave the session running forever.
                    s.nav_errors += 1;
                    if self.nav_clicked {
                        // The failed nav already clicked something — the
                        // screen is no longer the one the user vouched for,
                        // so this seek may not capture a reference again
                        // (except on a visual confirmation).
                        s.capture_forfeited = true;
                    }
                    if s.nav_errors > NAV_ERROR_CAP {
                        return self.stop_session(
                            "page_unavailable",
                            None,
                            "Game page stayed unavailable; stopped.".to_string(),
                        );
                    }
                    warn!(
                        "autostart: queue nav failed ({} in a row): {e:#}",
                        s.nav_errors
                    );
                    s.next_step_at = now + Duration::from_millis(cfg.confirm_interval_ms as u64);
                }
            }
        } else if s.clearing {
            // Vision on but no reference (fresh install whose first queue
            // nav never got to persist one): the lobby can never be
            // confirmed, so press only enough to clear the typical result
            // screens, then stop with precise guidance — not the full cap's
            // worth of stray clicks on what is probably the real lobby.
            if cfg.use_vision && self.home_reference().is_none() {
                if s.confirm_presses >= NO_VISION_CONFIRM {
                    // Guidance must match the config: with auto-calibrate on,
                    // pressing Start at the lobby heals this; with it off, only
                    // manual calibration (or re-enabling auto-calibrate) can.
                    return if cfg.auto_calibrate_home {
                        self.stop_session(
                            "no_reference",
                            None,
                            "No lobby reference fingerprint yet; stopped. Press \
                             Start at the lobby to calibrate automatically."
                                .to_string(),
                        )
                    } else {
                        self.stop_session(
                            "no_reference_manual",
                            None,
                            "No lobby reference fingerprint; stopped. \
                             Auto-calibration is disabled — enable \
                             auto_calibrate_home or calibrate manually."
                                .to_string(),
                        )
                    };
                }
            } else if s.confirm_presses >= MAX_CLEAR_CONFIRMS {
                // Cap reached — the result screens are long gone; whatever
                // screen this is, the lobby can't be confirmed, so exit rather
                // than act.
                return self.stop_session(
                    "lobby_not_found",
                    None,
                    "Still can't recognise the lobby after repeated confirms; \
                     stopped. Press Start at the lobby again."
                        .to_string(),
                );
            }
            match self.press_confirm().await {
                // Only presses that actually dispatched count towards the
                // cap — a dead page must not burn through it while nothing
                // was ever clicked. Dispatch failures get their own budget so
                // a dead page can't spin a no-vision clearing seek forever.
                Ok(()) => {
                    s.confirm_presses += 1;
                    s.nav_errors = 0;
                }
                Err(e) => {
                    s.nav_errors += 1;
                    if s.nav_errors > NAV_ERROR_CAP {
                        return self.stop_session(
                            "page_unavailable",
                            None,
                            "Game page stayed unavailable; stopped.".to_string(),
                        );
                    }
                    debug!("autostart: confirm click skipped: {e:#}");
                }
            }
            s.next_step_at = now + Duration::from_millis(cfg.confirm_interval_ms as u64);
        } else {
            // Not at the lobby yet — wait and re-check. Unbounded by design:
            // this is the stale-stream recovery wait (a live game can run
            // arbitrarily long) and it presses nothing.
            s.next_step_at = now + LOBBY_RECHECK_INTERVAL;
        }
        Phase::Seeking(s)
    }

    /// Screenshot the Ranked-label region and NCC-compare to the saved lobby
    /// reference. `false` if anything is unavailable (no page, no reference,
    /// screenshot failed) — the caller then just waits/retries.
    async fn vision_at_lobby(&mut self, cfg: &AutoStartConfig) -> bool {
        let Some(page) = self.page().await else {
            return false;
        };
        let Ok(rect) = evaluate_canvas_rect(&page).await else {
            return false;
        };
        let (x, y, w, h) = lc::HOME_ANCHOR;
        let fp = match vision::region_fingerprint(&page, &rect, x, y, w, h).await {
            Ok(fp) => fp,
            Err(e) => {
                debug!("autostart: fingerprint failed: {e:#}");
                return false;
            }
        };
        let Some(reference) = self.home_reference() else {
            debug!("autostart: no lobby reference — run lobby calibration first");
            return false;
        };
        vision::ncc_similarity(&fp, &reference) >= cfg.home_ncc_threshold
    }

    /// Navigate Ranked lobby → tier (scroll for Jade/Throne) → mode.
    /// Matchmaking starts on the mode click (no separate Start button). The
    /// sequence spans several seconds, so every wait re-checks the shared
    /// active flag AND that autoplay is still on — a Stop press or autoplay
    /// being switched off aborts before the next click and the nav returns
    /// `Ok(false)` (nothing queued; the client may be left on an intermediate
    /// lobby screen, which is harmless). `Ok(true)` = the mode click was issued.
    async fn run_queue_nav(
        &mut self,
        cfg: &AutoStartConfig,
        room: RoomSelection,
        allow_capture: bool,
    ) -> Result<bool> {
        // Reset the per-nav state FIRST — before the fallible page/rect
        // acquisitions. A leftover `nav_clicked` from an earlier nav would
        // otherwise leak through an early `?` return into this seek's Err
        // handling and wrongly forfeit its capture vouch; same for a stale
        // pending fingerprint.
        self.pending_home_ref = None;
        self.nav_clicked = false;
        let page = self.page().await.ok_or_else(|| anyhow!("no page handle"))?;
        let inter = cfg.inter_click_delay_ms as u64;

        // Auto-calibrate: we are believed to be at the lobby right now (about
        // to click the Ranked entry), so capture the lobby-home reference — but
        // only as PENDING; it is persisted when StartGame proves the queue
        // worked (see `persist_pending_reference`), and only when the caller
        // vouches for this screen (`allow_capture`: user-pressed first attempt
        // or a visual confirmation — never a timeout-retry on an unknown
        // screen). This keeps the visual return-to-lobby check working without
        // manual calibration and self-heals across skin/version changes.
        if allow_capture && cfg.auto_calibrate_home {
            if let Err(e) = self.capture_pending_reference(&page).await {
                debug!("autostart: auto-calibrate skipped: {e:#}");
            }
        }

        // Human-like pause at the lobby before committing to the next game.
        if !self
            .pause_while_active(cfg.inter_game_delay_ms as u64)
            .await
        {
            return Ok(false);
        }
        // From the first click on, the screen may be mutated by our own nav —
        // an error after this point forfeits the seek's capture vouch.
        self.nav_clicked = true;
        self.click_norm(&page, lc::RANKED_ENTRY).await?;
        if !self.pause_while_active(inter).await {
            return Ok(false);
        }

        let (tier_xy, needs_scroll) = lc::tier_target(room.tier);
        if needs_scroll {
            // Fresh rect for the scroll anchor too — the pauses above are
            // exactly when a user might resize the window.
            let rect = evaluate_canvas_rect(&page).await?;
            let (sx, sy) = rect.pixel(lc::TIER_LIST_SCROLL_AT.0, lc::TIER_LIST_SCROLL_AT.1);
            for _ in 0..lc::TIER_SCROLL_TICKS {
                dispatch_scroll(&page, sx, sy, lc::TIER_SCROLL_DELTA).await?;
                tokio::time::sleep(lc::TIER_SCROLL_INTERVAL).await;
            }
            if !self.pause_while_active(inter).await {
                return Ok(false);
            }
        }
        self.click_norm(&page, tier_xy).await?;
        if !self.pause_while_active(inter).await {
            return Ok(false);
        }

        let mode_xy = lc::mode_target(room.player_count, room.round_length);
        self.click_norm(&page, mode_xy).await?;
        Ok(true)
    }

    /// Sleep `ms` in short slices, re-checking the shared active flag AND that
    /// autoplay is still on, so a Stop press — or autoplay being switched off,
    /// which would leave the queued game unplayed — aborts multi-second
    /// navigation promptly. Returns `false` the moment either check fails,
    /// `true` after sleeping the full span.
    async fn pause_while_active(&self, ms: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            if !self.control.active.load(Ordering::SeqCst) {
                return false;
            }
            // try_read: a briefly contended config lock must not abort the
            // nav — the next slice re-checks.
            if let Ok(c) = self.cfg.try_read() {
                if !c.autoplay.enabled {
                    return false;
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            tokio::time::sleep((deadline - now).min(Duration::from_millis(200))).await;
        }
    }

    async fn press_confirm(&self) -> Result<()> {
        let page = self.page().await.ok_or_else(|| anyhow!("no page handle"))?;
        self.click_norm(&page, lc::RESULT_CONFIRM).await
    }

    /// Click a 16:9-normalised point. The canvas rect is re-read per click —
    /// it is one cheap CDP call, and a cached rect goes stale the moment the
    /// user resizes the window mid-sequence (the multi-second queue nav is
    /// exactly such a window).
    async fn click_norm(&self, page: &Page, (xn, yn): (f64, f64)) -> Result<()> {
        let rect = evaluate_canvas_rect(page).await?;
        let (px, py) = rect.pixel(xn, yn);
        if !rect.contains(px, py) {
            return Err(anyhow!("click ({px},{py}) outside canvas {rect:?}"));
        }
        dispatch_click(page, px, py, HOVER_MS, HOLD_MS).await
    }

    async fn page(&self) -> Option<Page> {
        self.ctx.page.read().await.clone()
    }

    /// Capture the Ranked-region fingerprint now as the PENDING lobby
    /// reference. Persisted by [`Self::persist_pending_reference`] once the
    /// queued game's StartGame arrives.
    async fn capture_pending_reference(&mut self, page: &Page) -> Result<()> {
        let rect = evaluate_canvas_rect(page).await?;
        let (x, y, w, h) = lc::HOME_ANCHOR;
        self.pending_home_ref = Some(vision::region_fingerprint(page, &rect, x, y, w, h).await?);
        Ok(())
    }

    /// Promote the pending fingerprint to the saved lobby reference. Called on
    /// StartGame while our queue nav was awaiting it — the game beginning is
    /// strong (not perfect: a manual start inside the awaiting window also
    /// matches) evidence the nav really ran at the lobby. A wrongly persisted
    /// reference is not fatal: the next Start pressed at the lobby re-captures
    /// and overwrites it (cold starts trust the press, not the reference).
    fn persist_pending_reference(&mut self) {
        let Some(fp) = self.pending_home_ref.take() else {
            return;
        };
        let path = lobby_home_ref_path(&self.config_path);
        if let Err(e) = std::fs::write(&path, &fp) {
            // Keep the in-memory copy — the visual check still works this run.
            warn!("autostart: persisting lobby reference failed: {e:#}");
        }
        self.home_ref = Some(fp);
    }

    /// Deactivate the session and notify. Every unattended stop MUST go through
    /// here so the UI's polled active flag can never go stale (a stale `true`
    /// would silently resurrect the session on the user's next manual game).
    fn stop_session(
        &mut self,
        key: &str,
        args: Option<serde_json::Value>,
        body_en: String,
    ) -> Phase {
        self.control.active.store(false, Ordering::SeqCst);
        // A pending fingerprint must not outlive its session — a later manual
        // game's StartGame is no evidence about where THIS session's nav ran.
        self.pending_home_ref = None;
        self.notify(key, args, body_en);
        Phase::Stopped
    }

    fn home_reference(&mut self) -> Option<Vec<u8>> {
        if self.home_ref.is_none() {
            let path = lobby_home_ref_path(&self.config_path);
            self.home_ref = std::fs::read(&path).ok();
        }
        self.home_ref.clone()
    }

    /// Notify the user. `key` selects the localized text on the frontend
    /// (under `autostart.notify.*`, interpolated with `args`); `body_en` is
    /// the English fallback and the text that goes into the log.
    fn notify(&self, key: &str, args: Option<serde_json::Value>, body_en: String) {
        info!("autostart: {body_en}");
        let mut n = Notification::info("Auto-start")
            .title_key("autostart.notify.title")
            .body_key(format!("autostart.notify.{key}"))
            .body(body_en);
        if let Some(args) = args {
            n = n.args(args);
        }
        let _ = self.notify_bus.send(n);
    }
}

/// Spawn point for the autostart loop. Wired by `crate::lib::run` (spawned
/// unconditionally at startup; sessions are driven purely by the shared
/// [`AutoStartControl`] via the Start/Stop IPC commands).
#[allow(clippy::too_many_arguments)]
pub async fn run_autostart_manager(
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    mjai_bus: MjaiBus,
    notify_bus: NotifyBus,
    config_path: Arc<PathBuf>,
    control: Arc<AutoStartControl>,
) -> Result<()> {
    AutoStartManager::new(cfg, ctx, mjai_bus, notify_bus, config_path, control)
        .run()
        .await
}
