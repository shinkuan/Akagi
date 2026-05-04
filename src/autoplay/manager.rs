//! Autoplay manager: subscribes to bot decisions + mjai events,
//! translates them into UI clicks dispatched via CDP.
//!
//! Lifecycle:
//! - Spawned by `crate::lib::run` when `cfg.autoplay.enabled = true`.
//! - One long-lived `tokio::select!` loop over `BotResponseBus` and
//!   `MjaiBus`. Bot responses drive clicks; mjai events update local
//!   per-game tracking state (`last_kawa_tile`, `last_self_tsumo`,
//!   `self_riichi_accepted`, `reach_state`).
//!
//! Failure modes are silent-by-design: if the page handle is missing
//! (chromium backend not running) or the canvas-rect query fails, the
//! manager logs a warning and skips the click. The bot pipeline is
//! untouched; the user can still play the round manually.

use crate::autoplay::cdp_input::{dispatch_click, evaluate_canvas_rect};
use crate::autoplay::context::{AutoplayContext, CanvasRect};
use crate::autoplay::majsoul::MajsoulAutoplay;
use crate::autoplay::platform::{ActionContext, PlatformAutoplay, ReachState, Step};
use crate::bot::BotResponse;
use crate::config::AppConfig;
use crate::event_bus::{BotResponseBus, MjaiBus};
use crate::game_state::tracker::GameTracker;
use crate::schema::MjaiEvent;
use riichienv_core::action::Action;
use riichienv_core::state::legal_actions::GameStateLegalActions;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast::error::RecvError, Mutex, RwLock};
use tracing::{debug, info, warn};

/// How long before a cached `CanvasRect` is treated as stale and re-queried.
const CANVAS_RECT_TTL: Duration = Duration::from_secs(30);

pub struct AutoplayManager {
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    tracker: Arc<Mutex<GameTracker>>,
    mjai_bus: MjaiBus,
    platform: Arc<dyn PlatformAutoplay>,
    state: ManagerState,
}

#[derive(Default)]
struct ManagerState {
    last_kawa_tile: Option<String>,
    last_self_tsumo: Option<String>,
    self_riichi_accepted: bool,
    reach_state: ReachState,
    canvas_rect_at: Option<Instant>,
}

impl Default for ReachState {
    fn default() -> Self {
        Self::Idle
    }
}

impl AutoplayManager {
    pub fn new(
        cfg: Arc<RwLock<AppConfig>>,
        ctx: Arc<AutoplayContext>,
        tracker: Arc<Mutex<GameTracker>>,
        mjai_bus: MjaiBus,
    ) -> Self {
        Self {
            cfg,
            ctx,
            tracker,
            mjai_bus,
            // Only Majsoul is wired up today; future Tenhou impl swaps
            // here based on config.platform.kind at run start.
            platform: Arc::new(MajsoulAutoplay::new()),
            state: ManagerState::default(),
        }
    }

    /// Run forever. Returns `Err` only on bus closure (process exit).
    pub async fn run(mut self, response_bus: BotResponseBus) -> anyhow::Result<()> {
        let mut bot_rx = response_bus.subscribe();
        let mut mjai_rx = self.mjai_bus.subscribe();
        info!("autoplay manager started");

        loop {
            tokio::select! {
                msg = bot_rx.recv() => match msg {
                    Ok(resp) => self.handle_bot_response(resp).await,
                    Err(RecvError::Lagged(n)) => warn!("autoplay: bot bus lagged {n}"),
                    Err(RecvError::Closed) => {
                        info!("autoplay: bot bus closed; exiting");
                        return Ok(());
                    }
                },
                msg = mjai_rx.recv() => match msg {
                    Ok(ev) => self.handle_mjai_event(&ev),
                    Err(RecvError::Lagged(n)) => warn!("autoplay: mjai bus lagged {n}"),
                    Err(RecvError::Closed) => {
                        info!("autoplay: mjai bus closed; exiting");
                        return Ok(());
                    }
                },
            }
        }
    }

    async fn handle_bot_response(&mut self, resp: BotResponse) {
        // Re-read config every iteration so `cfg.autoplay.enabled` can be
        // toggled at runtime via the Settings UI without restarting.
        let cfg_guard = self.cfg.read().await;
        if !cfg_guard.autoplay.enabled {
            return;
        }
        let cfg = cfg_guard.autoplay.majsoul.clone();
        drop(cfg_guard);

        // Pull our seat + legal actions from the live engine state. This
        // bracket releases the tracker mutex before we sleep/click.
        let (our_seat, legal_actions, snapshot, num_players) = {
            let tracker = self.tracker.lock().await;
            let our_seat = match tracker.our_seat() {
                Some(s) => s,
                None => return, // game hasn't started or no perspective tagged
            };
            let snapshot = match tracker.snapshot() {
                Some(s) => s,
                None => return,
            };
            let num_players = snapshot.num_players;
            let legal_actions: Vec<Action> = if num_players == 3 {
                tracker
                    .state_3p()
                    .map(|s| {
                        // 3p engine has its own GameStateLegalActions impl —
                        // import elsewhere if needed; v1 only handles 4p clicks.
                        let _ = s;
                        Vec::new()
                    })
                    .unwrap_or_default()
            } else {
                tracker
                    .state()
                    .map(|s| s._get_legal_actions_internal(our_seat))
                    .unwrap_or_default()
            };
            (our_seat, legal_actions, snapshot, num_players)
        };

        let action_ctx = ActionContext {
            action: &resp.action,
            snapshot: &snapshot,
            legal_actions: &legal_actions,
            our_seat,
            last_kawa_tile: self.state.last_kawa_tile.as_deref(),
            last_self_tsumo: self.state.last_self_tsumo.as_deref(),
            self_riichi_accepted: self.state.self_riichi_accepted,
            reach_state: self.state.reach_state,
            num_players,
            cfg: &cfg,
        };

        let plan = self.platform.plan(&action_ctx);
        if plan.steps.is_empty() && !plan.inject_reach_for_followup {
            return;
        }

        debug!(
            "autoplay: action={:?} steps={} inject_reach={} await_riichi_dahai={}",
            resp.action,
            plan.steps.len(),
            plan.inject_reach_for_followup,
            plan.awaiting_riichi_dahai
        );

        // Resolve a canvas rect (cache + TTL). If we can't, drop the
        // click — the page handle isn't ready yet (e.g. user still on
        // the lobby), or the chromium backend isn't running at all.
        let rect = match self.canvas_rect_resolve().await {
            Some(r) => r,
            None => {
                warn!("autoplay: no canvas rect — skipping click for {:?}", resp.action);
                return;
            }
        };

        for step in &plan.steps {
            match step {
                Step::Sleep { duration_ms } => {
                    tokio::time::sleep(Duration::from_millis(*duration_ms as u64)).await;
                }
                Step::Click { x_norm, y_norm } => {
                    let (px, py) = rect.pixel(*x_norm, *y_norm);
                    if !rect.contains(px, py) {
                        warn!(
                            "autoplay: click ({px},{py}) outside canvas rect {:?}; skipping",
                            rect
                        );
                        continue;
                    }
                    // Need to re-acquire the page handle on each click;
                    // it may have been replaced (tab reload) between
                    // successive clicks within one action.
                    let page_guard = self.ctx.page.read().await;
                    let Some(page) = page_guard.as_ref() else {
                        warn!("autoplay: no page handle — aborting click sequence");
                        return;
                    };
                    if let Err(e) =
                        dispatch_click(page, px, py, cfg.hover_delay_ms, cfg.click_hold_ms).await
                    {
                        warn!("autoplay: dispatch_click failed: {e:#}");
                        return;
                    }
                    drop(page_guard);
                }
            }
        }

        // Path-B side effect: inject synthetic Reach so the bot will
        // emit the riichi-declaring dahai we need to click next.
        if plan.inject_reach_for_followup {
            self.state.reach_state = ReachState::AwaitingDahai;
            let synthetic = MjaiEvent::Reach { actor: our_seat, pai: None };
            if let Err(e) = self.mjai_bus.send(synthetic) {
                // No subscribers (e.g. bot disabled) — nothing to do, but
                // log for visibility because Path B without a downstream
                // bot would soft-hang autoplay until reach_state resets.
                debug!("autoplay: synthetic Reach send had no subscribers: {e:?}");
            } else {
                info!("autoplay: injected synthetic Reach (Path B) for seat {our_seat}");
            }
        }

        // After a successful Path-B follow-up dahai, return to Idle.
        // The check is conservative: only flip on Dahai by our seat.
        if matches!(self.state.reach_state, ReachState::AwaitingDahai) {
            if let MjaiEvent::Dahai { actor, .. } = &resp.action {
                if *actor == our_seat {
                    self.state.reach_state = ReachState::Idle;
                }
            }
        }
    }

    fn handle_mjai_event(&mut self, ev: &MjaiEvent) {
        match ev {
            MjaiEvent::StartGame { .. } | MjaiEvent::EndGame => {
                self.state = ManagerState::default();
            }
            MjaiEvent::StartKyoku { .. } | MjaiEvent::EndKyoku => {
                // Per-kyoku reset: keep last seen rect cache, drop
                // everything else.
                let canvas_at = self.state.canvas_rect_at;
                self.state = ManagerState::default();
                self.state.canvas_rect_at = canvas_at;
            }
            MjaiEvent::Tsumo { actor, pai } => {
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.last_self_tsumo = Some(pai.clone());
                    }
                }
            }
            MjaiEvent::Dahai { actor, pai, .. } => {
                self.state.last_kawa_tile = Some(pai.clone());
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.last_self_tsumo = None;
                    }
                }
            }
            MjaiEvent::ReachAccepted { actor } => {
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.self_riichi_accepted = true;
                    }
                }
            }
            MjaiEvent::Chi { actor, .. }
            | MjaiEvent::Pon { actor, .. }
            | MjaiEvent::Daiminkan { actor, .. }
            | MjaiEvent::Ankan { actor, .. }
            | MjaiEvent::Kakan { actor, .. } => {
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.last_self_tsumo = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Best-effort seat lookup that avoids blocking on the tracker mutex
    /// inside the mjai event handler (which runs synchronously in the
    /// select arm). Falls back to `None` if the lock is contended;
    /// missing the seat once is harmless, the next event will catch up.
    fn our_seat_cached(&self) -> Option<u8> {
        self.tracker.try_lock().ok().and_then(|t| t.our_seat())
    }

    async fn canvas_rect_resolve(&mut self) -> Option<CanvasRect> {
        let now = Instant::now();
        if let Some(at) = self.state.canvas_rect_at {
            if now.duration_since(at) < CANVAS_RECT_TTL {
                if let Some(r) = *self.ctx.canvas_rect.read().await {
                    return Some(r);
                }
            }
        }
        // Re-query.
        let page_guard = self.ctx.page.read().await;
        let page = page_guard.as_ref()?.clone();
        drop(page_guard);
        match evaluate_canvas_rect(&page).await {
            Ok(rect) => {
                *self.ctx.canvas_rect.write().await = Some(rect);
                self.state.canvas_rect_at = Some(now);
                Some(rect)
            }
            Err(e) => {
                debug!("autoplay: evaluate_canvas_rect failed: {e:#}");
                None
            }
        }
    }
}

/// Spawn point for the autoplay loop. Wired by `crate::lib::run` so the
/// `tauri::async_runtime` Tokio runtime is the host.
pub async fn run_autoplay_manager(
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    tracker: Arc<Mutex<GameTracker>>,
    mjai_bus: MjaiBus,
    response_bus: BotResponseBus,
) -> anyhow::Result<()> {
    AutoplayManager::new(cfg, ctx, tracker, mjai_bus).run(response_bus).await
}
