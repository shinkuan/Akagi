//! Built-in, in-process bot runner backed by the pure-Rust `native_bot`
//! crate (a small behavior-cloned CNN run via candle — no Python, no
//! libriichi, no subprocess).
//!
//! Unlike [`crate::bot::runner::SubprocessBot`], this runner keeps a live
//! `native_bot::Engine` in-process: each `react()` feeds the batch through the
//! engine's riichienv-core game state and, at our decision points, runs the net
//! to pick a legal action. Bundled model weights are embedded in the binary, so
//! there is no venv, no `uv sync`, and nothing to install.
//!
//! Two reserved bot names select it: [`NATIVE_4P`] (yonma) and [`NATIVE_3P`]
//! (sanma). `BotManager::spawn_runner` recognises them and constructs this
//! runner directly, bypassing the `bot.py`/registry path.
//!
//! ## Local model vs cloud inference
//!
//! One runner serves both paths. It always loads the embedded model, and it
//! re-reads `bot.api` from the shared config at **every decision**, so the user
//! can enable cloud inference, correct a mistyped key, or switch models in the
//! middle of a hanchan and have it apply to the very next move. The local model
//! stays in the loop even when the API is on:
//!
//! - **Gating** — the API's rate limits are low, and calling on every opponent
//!   discard "just in case" roughly triples the request count. So we run the
//!   local model's cheap legal-action check first and only hit the network when
//!   our seat genuinely has a move to make. Forced moves — a legal set of
//!   exactly one action, e.g. every tsumogiri while riichi — are answered
//!   locally too: the server could not pick anything else, so the call would
//!   spend quota to learn nothing.
//! - **Fallback** — if the server is unreachable, rate-limited, or the key is
//!   invalid, we play the local model's action so a live game never stalls.
//! - **Circuit breaker** — after a failed call the API is skipped for a growing
//!   backoff window ([`Breaker`]), so a dead server costs one slow turn rather
//!   than a timeout on every single decision for the rest of the game.
//!
//! The remote service is stateless: every call re-uploads the current kyoku's
//! mjai stream (from our seat's censored perspective). We accumulate that
//! stream in [`NativeBot::stream`] — unconditionally, so switching the API on
//! mid-kyoku has a complete stream to send — and shape it for the API in
//! [`build_api_events`].

use crate::bot::api::{ApiClient, Candidate};
use crate::bot::runner::BotRunner;
use crate::bot::types::BotResponse;
use crate::config::{AppConfig, NativeApiConfig};
use crate::event_bus::NotifyBus;
use crate::game_state::convert;
use crate::schema::{MjaiEvent, Notification};
use anyhow::Result;
use async_trait::async_trait;
use native_bot::engine::{BotAction, Decision, Engine};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::warn;

/// Reserved name for the built-in 4-player bot.
pub const NATIVE_4P: &str = "akagi-native";
/// Reserved name for the built-in 3-player (sanma) bot.
pub const NATIVE_3P: &str = "akagi-native3p";

/// Notification id for the online-API status toasts (enabled / degraded /
/// recovered / disabled). A stable id lets each toast replace the previous one,
/// and the frontend keys its persistent "online API" status indicator off the
/// same channel: `warn`/`error` mean degraded, anything else means healthy.
pub const NATIVE_API_HEALTH_ID: &str = "native-api-health";

/// First backoff after an API failure. Roughly one Majsoul turn, so a blip
/// costs a single local-model move.
const BREAKER_BASE: Duration = Duration::from_secs(5);
/// Ceiling for the exponential backoff. A server that has been down for two
/// minutes is retried every two minutes — one slow turn per window.
const BREAKER_MAX: Duration = Duration::from_secs(120);

/// Whether `name` selects the built-in native bot (either mode).
pub fn is_native(name: &str) -> bool {
    name == NATIVE_4P || name == NATIVE_3P
}

/// Display label for a reserved native-bot name, for the Bots UI.
pub fn display_name(name: &str) -> Option<&'static str> {
    match name {
        NATIVE_4P => Some("Akagi (built-in, 4p)"),
        NATIVE_3P => Some("Akagi (built-in, 3p)"),
        _ => None,
    }
}

/// Construct the built-in bot runner for a game of `num_players` seated at
/// `actor_id`, holding `config` so `bot.api` can be re-read at each decision.
///
/// The cloud-inference session is seeded from the config as it stands now, and
/// silently — a game starting with the API already on is not news. Later
/// changes toast.
pub async fn build(
    actor_id: u8,
    num_players: u8,
    config: Arc<RwLock<AppConfig>>,
    notify_tx: NotifyBus,
) -> Result<Box<dyn BotRunner>> {
    let initial = config.read().await.bot.api.clone();
    let mut bot = NativeBot::new(actor_id, num_players, config, notify_tx)?;
    bot.apply_api_config(&initial, Announce::Silently);
    Ok(Box::new(bot))
}

/// Whether an `bot.api` change should toast the user. Seeding at game start is
/// silent; a change the user just made in Settings is worth confirming.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Announce {
    Silently,
    ToUser,
}

/// Suppresses calls to a failing inference server with exponential backoff, so
/// only the first decision of an outage pays a request timeout. Reset whenever
/// the user changes the API settings — that is an explicit "try again now".
#[derive(Debug)]
struct Breaker {
    /// Whether the last API request succeeded. Toasts fire only on a change of
    /// this flag, so a persistently-down server doesn't spam a toast per turn.
    healthy: bool,
    consecutive_failures: u32,
    /// Skip the API until this instant. `None` ⇒ the breaker is closed.
    open_until: Option<Instant>,
}

impl Breaker {
    fn new() -> Self {
        Self {
            healthy: true,
            consecutive_failures: 0,
            open_until: None,
        }
    }

    /// Whether an API call may be attempted right now.
    fn allows(&self) -> bool {
        self.open_until.is_none_or(|t| Instant::now() >= t)
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.open_until = None;
    }

    /// Open the breaker for `BREAKER_BASE * 2^(failures-1)`, capped at
    /// [`BREAKER_MAX`]. Returns the window, for logging.
    fn record_failure(&mut self) -> Duration {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let shift = (self.consecutive_failures - 1).min(16);
        let backoff = BREAKER_BASE.saturating_mul(1u32 << shift).min(BREAKER_MAX);
        self.open_until = Some(Instant::now() + backoff);
        backoff
    }

    /// Back to a clean slate: retry immediately, and let the next failure toast.
    fn reset(&mut self) {
        self.healthy = true;
        self.consecutive_failures = 0;
        self.open_until = None;
    }
}

/// The remote inference session: an authenticated client plus the settings it
/// was built from, so a change to any of them rebuilds it.
struct ApiSession {
    client: ApiClient,
    base_url: String,
    key: String,
    /// Proxy URL the client was built with; part of the rebuild key.
    proxy: String,
    /// Model id to request; empty ⇒ let the server pick its game default.
    model: String,
}

pub struct NativeBot {
    engine: Engine,
    seat: u8,
    num_players: u8,
    /// Shared, live application config. `bot.api` is re-read at every decision
    /// (see [`NativeBot::react`]) so a mid-game settings change applies to the
    /// next move rather than the next game.
    config: Arc<RwLock<AppConfig>>,
    /// Current-kyoku mjai stream (Akagi schema). Reset on each `start_kyoku`
    /// but keeps the leading `start_game` the API requires as `events[0]`.
    /// Accumulated even while the API is off, so enabling it mid-kyoku can
    /// upload the kyoku so far.
    stream: Vec<MjaiEvent>,
    /// Toast channel — tells the user when cloud inference turns on/off, breaks,
    /// and recovers.
    notify_tx: NotifyBus,
    /// `Some` while cloud inference is configured (`enabled` + URL + key).
    api: Option<ApiSession>,
    /// The (base_url, key, proxy, model) tuple that last failed client build
    /// (e.g. an invalid proxy URL). Re-read every decision, so without this
    /// the same broken config would re-attempt and re-toast on every move;
    /// with it, the warning fires once until the user actually edits something.
    api_failed: Option<(String, String, String, String)>,
    breaker: Breaker,
}

impl NativeBot {
    /// Build the in-process bot for a game of `num_players` with our seat at
    /// `actor_id`, loading the bundled default weights for that mode. The cloud
    /// session starts unset; [`NativeBot::apply_api_config`] establishes it.
    pub fn new(
        actor_id: u8,
        num_players: u8,
        config: Arc<RwLock<AppConfig>>,
        notify_tx: NotifyBus,
    ) -> Result<Self> {
        let engine = native_bot::defaults::engine(num_players, actor_id)?;
        Ok(Self {
            engine,
            seat: actor_id,
            num_players,
            config,
            stream: Vec::new(),
            notify_tx,
            api: None,
            api_failed: None,
            breaker: Breaker::new(),
        })
    }

    /// Reconcile the live [`NativeApiConfig`] with our session.
    ///
    /// Rebuilds the client when the server URL or key changed, updates the model
    /// id, and drops the session entirely when the API is switched off. Any
    /// change resets the circuit breaker: the user editing these settings *is*
    /// the retry signal, so a corrected key is tried on this very decision
    /// rather than after the current backoff window.
    fn apply_api_config(&mut self, cfg: &NativeApiConfig, announce: Announce) {
        if !cfg.is_active() {
            self.api_failed = None;
            if self.api.take().is_some() {
                self.breaker.reset();
                self.notify(
                    Notification::info("Cloud inference off")
                        .body("The built-in bot is using the embedded local model.")
                        .id(NATIVE_API_HEALTH_ID),
                    announce,
                );
            }
            return;
        }

        let base_url = cfg.base_url.trim();
        let key = cfg.key.trim();
        // Collapses the on/off toggle into the effective value, so flipping
        // `proxy_enabled` alone changes this and rebuilds the client.
        let proxy = cfg.effective_proxy();
        let model = cfg.model_for(self.num_players).trim().to_string();
        let unchanged = self.api.as_ref().is_some_and(|s| {
            s.base_url == base_url && s.key == key && s.proxy == proxy && s.model == model
        });
        if unchanged {
            return;
        }
        // This exact config already failed to build a client — stay on the
        // local model quietly instead of re-attempting (and re-toasting) on
        // every decision. Any edit to the config clears the tombstone below.
        if self
            .api_failed
            .as_ref()
            .is_some_and(|f| f.0 == base_url && f.1 == key && f.2 == proxy && f.3 == model)
        {
            return;
        }

        let was_off = self.api.is_none();
        match ApiClient::new(base_url, key, proxy) {
            Ok(client) => {
                self.api_failed = None;
                self.api = Some(ApiSession {
                    client,
                    base_url: base_url.to_string(),
                    key: key.to_string(),
                    proxy: proxy.to_string(),
                    model: model.clone(),
                });
                self.breaker.reset();
                let body = if model.is_empty() {
                    "Using the server's default model.".to_string()
                } else {
                    format!("Using model {model}.")
                };
                let title = if was_off {
                    "Cloud inference on"
                } else {
                    "Cloud inference updated"
                };
                self.notify(
                    Notification::info(title)
                        .body(body)
                        .id(NATIVE_API_HEALTH_ID),
                    announce,
                );
            }
            Err(e) => {
                // Building the HTTP client failed — retrying won't help, so drop
                // to the local model rather than opening a breaker window, and
                // remember the rejected config so this warns once, not per move.
                let msg = format!("{e:#}");
                warn!("native API: client setup failed ({msg}); using the local model");
                self.api = None;
                self.api_failed = Some((
                    base_url.to_string(),
                    key.to_string(),
                    proxy.to_string(),
                    model.clone(),
                ));
                self.notify(
                    Notification::warn("Cloud inference unavailable")
                        .body(msg)
                        .id(NATIVE_API_HEALTH_ID),
                    announce,
                );
            }
        }
    }

    fn notify(&self, note: Notification, announce: Announce) {
        if announce == Announce::ToUser {
            let _ = self.notify_tx.send(note);
        }
    }

    /// Record the outcome of an API request and, **only on a health change**,
    /// toast the user: a warning when the server first becomes unreachable
    /// (the bot silently falls back to the local model), and a success when it
    /// recovers. `ok` = the HTTP request itself succeeded (server reachable).
    fn record_health(&mut self, ok: bool, err: Option<&str>) {
        if ok == self.breaker.healthy {
            return; // no transition
        }
        self.breaker.healthy = ok;
        let note = if ok {
            Notification::success("Online inference restored")
                .body("The built-in bot is using the online API again.")
                .id(NATIVE_API_HEALTH_ID)
        } else {
            let body = match err {
                Some(e) => format!("Falling back to the built-in local model. ({e})"),
                None => "Falling back to the built-in local model.".to_string(),
            };
            Notification::warn("Online inference unavailable")
                .body(body)
                .id(NATIVE_API_HEALTH_ID)
        };
        let _ = self.notify_tx.send(note);
    }

    /// Append one event to the current-kyoku stream, resetting on boundaries so
    /// each request stays small (a kyoku is well under the API's 512-event cap).
    fn accumulate(&mut self, ev: &MjaiEvent) {
        match ev {
            MjaiEvent::StartGame { .. } => {
                self.stream.clear();
                self.stream.push(ev.clone());
            }
            MjaiEvent::StartKyoku { .. } => {
                // Keep the leading start_game (required as events[0]); drop the
                // previous kyoku's tail.
                let start_game = self
                    .stream
                    .first()
                    .filter(|e| matches!(e, MjaiEvent::StartGame { .. }))
                    .cloned();
                self.stream.clear();
                if let Some(sg) = start_game {
                    self.stream.push(sg);
                }
                self.stream.push(ev.clone());
            }
            _ => self.stream.push(ev.clone()),
        }
    }

    /// Query the server for the move at the current decision point. Falls back
    /// to `local` (the local model's decision) on any error or a `null`
    /// reaction so a live game never stalls.
    async fn remote_decision(&mut self, local: &Decision) -> (MjaiEvent, Option<Value>) {
        let events = build_api_events(&self.stream, self.seat, self.num_players);
        let result = match self.api.as_ref() {
            Some(s) => {
                let model = s.model.clone();
                s.client.react(model_arg(&model), self.seat, events).await
            }
            None => return local_reply(local, self.seat),
        };
        // A successful HTTP round-trip (even a `null` reaction) means the server
        // is reachable; only a transport/HTTP error counts as "API unavailable".
        match result {
            Ok(resp) => {
                self.breaker.record_success();
                self.record_health(true, None);
                let title = api_show_title(
                    resp.model.as_deref(),
                    self.api.as_ref().map(|s| s.model.as_str()),
                );
                match resp.reaction {
                    Some(reaction) => match self
                        .resolve_reaction(reaction, &resp.candidates, &title)
                        .await
                    {
                        Some(pair) => pair,
                        None => local_reply(local, self.seat),
                    },
                    None => {
                        // Server sees no legal action though the local gate found
                        // one — a stream mismatch. Play the local move rather than
                        // silently pass a real turn.
                        warn!(
                            "native API: null reaction though local has a legal move; using local"
                        );
                        local_reply(local, self.seat)
                    }
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                let backoff = self.breaker.record_failure();
                warn!(
                    "native API react failed ({msg}); using the local model, \
                     skipping the API for {}s",
                    backoff.as_secs()
                );
                self.record_health(false, Some(&msg));
                local_reply(local, self.seat)
            }
        }
    }

    /// Turn a raw mjai reaction from the server into the event to play, filling
    /// the reach two-step (declare → discard) when the reaction is a bare
    /// `reach`. `None` if the reaction can't be parsed (caller falls back).
    async fn resolve_reaction(
        &mut self,
        reaction: Value,
        candidates: &[Candidate],
        title: &str,
    ) -> Option<(MjaiEvent, Option<Value>)> {
        let mut ev: MjaiEvent = serde_json::from_value(reaction).ok()?;
        set_actor(&mut ev, self.seat);
        // Majsoul fuses declaring riichi + discarding into one click, and
        // autoplay stalls unless the reach event names the discard. The server
        // returns a bare reach (the discard is a second decision), so resolve it
        // now — ask the server again with the reach appended, or fall back to
        // the local model's predicted riichi discard.
        if let MjaiEvent::Reach { pai: None, .. } = &ev {
            match self.reach_discard().await {
                Some(discard) => {
                    ev = MjaiEvent::Reach {
                        actor: self.seat,
                        pai: Some(discard),
                    };
                }
                // Couldn't resolve the riichi discard (the follow-up call failed
                // AND the local model found no riichi-legal discard). A reach
                // without the discard tile stalls autoplay, so decline the API
                // reaction entirely and let the caller fall back to the full
                // local decision.
                None => return None,
            }
        }
        let meta = build_show_meta_mjai(&ev, candidates, title);
        Some((ev, meta))
    }

    /// Resolve the post-reach discard: append the reach to the stream and
    /// re-query the server. Falls back to the local model's predicted riichi
    /// discard on any error.
    async fn reach_discard(&mut self) -> Option<String> {
        let mut events = build_api_events(&self.stream, self.seat, self.num_players);
        events.push(serde_json::json!({ "type": "reach", "actor": self.seat }));
        let result = {
            let s = self.api.as_ref()?;
            let model = s.model.clone();
            s.client.react(model_arg(&model), self.seat, events).await
        };
        match result {
            Ok(resp) => {
                self.breaker.record_success();
                self.record_health(true, None);
                if let Some(reaction) = resp.reaction {
                    if let Ok(MjaiEvent::Dahai { pai, .. }) =
                        serde_json::from_value::<MjaiEvent>(reaction)
                    {
                        return Some(pai);
                    }
                }
                self.engine.reach_discard()
            }
            Err(e) => {
                let msg = format!("{e:#}");
                // The declare call succeeded but the follow-up didn't: the server
                // is flaky. Trip the breaker so the *next* decision doesn't pay
                // another timeout, and play the local model's riichi discard.
                self.breaker.record_failure();
                warn!("native API reach-discard follow-up failed ({msg}); using local");
                self.record_health(false, Some(&msg));
                self.engine.reach_discard()
            }
        }
    }
}

/// The `model` argument for a react call: `None` when unset so the server
/// falls back to its game default.
fn model_arg(model: &str) -> Option<&str> {
    (!model.is_empty()).then_some(model)
}

fn attach_legal_ops(meta: &mut Option<Value>, legal_ops: &[String]) {
    let Some(show) = meta
        .as_mut()
        .and_then(Value::as_object_mut)
        .and_then(|root| root.get_mut("show"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    show.insert("legal_ops".to_string(), serde_json::json!(legal_ops));
}

#[async_trait]
impl BotRunner for NativeBot {
    async fn react(&mut self, events: &[MjaiEvent]) -> Result<BotResponse> {
        for ev in events {
            // Keep our seat current if a start_game tags a (possibly new) seat.
            if let MjaiEvent::StartGame { id: Some(seat), .. } = ev {
                self.seat = *seat;
                self.engine.set_seat(*seat);
            }
            self.accumulate(ev);
            if let Some(ri) = convert::to_riichienv(ev)? {
                self.engine.feed(ri);
            }
        }

        // Local gate: nothing to decide ⇒ reply `none`, spend no API call, and
        // leave whatever card is on screen alone.
        let local = match self.engine.decide()? {
            Some(d) if is_decision_point(&d.candidates) => d,
            _ => {
                return Ok(BotResponse {
                    action: MjaiEvent::None,
                    meta: None,
                })
            }
        };

        // Re-read `bot.api` on every decision: the user may have enabled cloud
        // inference, pasted a corrected key, or switched models since the last
        // move, and they expect it to take effect now — not next game.
        let cfg = self.config.read().await.bot.api.clone();
        self.apply_api_config(&cfg, Announce::ToUser);

        // A forced move (the legal set is a singleton — e.g. tsumogiri while
        // riichi) has only one possible answer, so asking the server would
        // spend a metered API call to learn nothing. Answer it locally.
        let use_api = self.api.is_some() && self.breaker.allows() && !local.forced;
        let (action, mut meta) = if use_api {
            self.remote_decision(&local).await
        } else {
            local_reply(&local, self.seat)
        };
        attach_legal_ops(&mut meta, &local.legal_ops);
        Ok(BotResponse { action, meta })
    }

    async fn reset(&mut self) -> Result<()> {
        self.engine.reset();
        self.stream.clear();
        self.breaker.reset();
        Ok(())
    }
}

/// Did the bot actually choose anything here?
///
/// This is the precondition for touching the HUD card: it must change when the
/// bot made a choice, and *only* then. A real decision has something to choose
/// between — our own turn (a discard, riichi, kan, tsumo…), or a call window
/// where a pon / chi / kan / ron sits next to the pass.
///
/// Two things are not decisions. An empty legal set, obviously. And a legal set
/// of exactly `[Pass]`: the engine offers `Pass` unconditionally in its response
/// phase, so a seat standing in someone *else's* call window still gets one
/// handed back. Rendering that as "Pass 100%" would replace real advice with a
/// choice the bot never had.
///
/// The bare-`[Pass]` case is not reachable in live play today — the bridge feeds
/// opponents' hands as `?`, so the engine can only ever see *our* claims, and it
/// only opens a response window when a claim exists, which means the pass always
/// arrives next to the call it was weighed against. But that is a fact about
/// what the engine currently knows, not about what the card promises. Encode the
/// promise.
fn is_decision_point(candidates: &[(BotAction, f32)]) -> bool {
    !matches!(candidates, [] | [(BotAction::Pass, _)])
}

/// Build the reply pair (mjai action + HUD card) from the local model's
/// decision — the fallback when the API path is unavailable.
fn local_reply(local: &Decision, seat: u8) -> (MjaiEvent, Option<Value>) {
    let meta = build_show_meta(&local.candidates);
    (bot_action_to_mjai(local.action.clone(), seat), meta)
}

/// Shape the accumulated Akagi mjai stream into the API's expected JSON:
/// censor other seats' hidden info to `?`, pad 3p `start_game`/`start_kyoku`
/// arrays to length 4, strip player-count / predicted-reach extensions.
fn build_api_events(stream: &[MjaiEvent], seat: u8, num_players: u8) -> Vec<Value> {
    stream
        .iter()
        .map(|ev| to_api_event(ev, seat, num_players))
        .collect()
}

fn to_api_event(ev: &MjaiEvent, seat: u8, num_players: u8) -> Value {
    use serde_json::json;
    let three_p = num_players == 3;
    match ev {
        MjaiEvent::StartGame { names, .. } => {
            let mut names = names.clone();
            if three_p {
                while names.len() < 4 {
                    names.push(String::new());
                }
            }
            json!({ "type": "start_game", "names": names })
        }
        MjaiEvent::StartKyoku {
            bakaze,
            dora_marker,
            kyoku,
            honba,
            kyotaku,
            oya,
            scores,
            tehais,
            ..
        } => {
            let mut scores = scores.clone();
            // Reveal only our own hand; every other seat is 13 "?".
            let mut tehais: Vec<Vec<String>> = tehais
                .iter()
                .enumerate()
                .map(|(i, hand)| {
                    if i as u8 == seat {
                        hand.clone()
                    } else {
                        hidden_hand()
                    }
                })
                .collect();
            if three_p {
                // Pad to the length-4 shape the API requires for 3p, with a
                // phantom 4th seat (score 0, 13 "?").
                while scores.len() < 4 {
                    scores.push(0);
                }
                while tehais.len() < 4 {
                    tehais.push(hidden_hand());
                }
            }
            json!({
                "type": "start_kyoku",
                "bakaze": bakaze,
                "dora_marker": dora_marker,
                "kyoku": kyoku,
                "honba": honba,
                "kyotaku": kyotaku,
                "oya": oya,
                "scores": scores,
                "tehais": tehais,
            })
        }
        MjaiEvent::Tsumo { actor, pai } => {
            // We never see another seat's draw.
            let pai = if *actor == seat {
                pai.clone()
            } else {
                "?".to_string()
            };
            json!({ "type": "tsumo", "actor": actor, "pai": pai })
        }
        MjaiEvent::Reach { actor, .. } => {
            // Strip the non-spec predicted `pai`; the API wants a bare reach.
            json!({ "type": "reach", "actor": actor })
        }
        // Everything else is public and already API-shaped.
        other => serde_json::to_value(other).unwrap_or_else(|_| json!({ "type": "none" })),
    }
}

fn hidden_hand() -> Vec<String> {
    vec!["?".to_string(); 13]
}

/// Force an mjai reaction's `actor` to our seat (the server should already set
/// it, but be defensive).
fn set_actor(ev: &mut MjaiEvent, seat: u8) {
    use MjaiEvent as E;
    match ev {
        E::Tsumo { actor, .. }
        | E::Dahai { actor, .. }
        | E::Reach { actor, .. }
        | E::Pon { actor, .. }
        | E::Chi { actor, .. }
        | E::Daiminkan { actor, .. }
        | E::Ankan { actor, .. }
        | E::Kakan { actor, .. }
        | E::Hora { actor, .. }
        | E::Kita { actor, .. } => *actor = seat,
        _ => {}
    }
}

/// Prepend `lead` to `rest`, forming a meld's tile list (called tile first).
fn with_lead(lead: &str, rest: &[String]) -> Vec<String> {
    std::iter::once(lead.to_string())
        .chain(rest.iter().cloned())
        .collect()
}

/// Card title for a decision the embedded local model served. The two title
/// forms (this and [`api_show_title`]) let the HUD say, per decision, which
/// inference path actually produced the move — the API silently falls back to
/// the local model, so the source can flip mid-game.
const SHOW_TITLE_LOCAL: &str = "Akagi · Local";

/// Row label for declining a call. Both inference paths use it, so the card
/// reads the same whichever one answered. "None" — the mjai wire word — is
/// accurate and useless: on a call window what the player wants to read is
/// "Pass 87%" against "Pon 13%", not "None 87%".
const PASS_LABEL: &str = "Pass";

/// Card title for a decision the online API served: name the model. The
/// server's own report (which model actually answered) wins over the
/// configured id — the config may be empty, meaning "server default".
fn api_show_title(served: Option<&str>, configured: Option<&str>) -> String {
    let model = [served, configured]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|m| !m.is_empty());
    match model {
        Some(m) => format!("Akagi · {m}"),
        None => "Akagi · Online".to_string(),
    }
}

/// HUD "Bot Show" card for the API path: the first row is the exact chosen move
/// (precise tiles), the rest are the server's runner-up **candidate** labels,
/// each with its probability — so the card shows the model's top-N.
fn build_show_meta_mjai(
    ev: &MjaiEvent,
    candidates: &[Candidate],
    title: &str,
) -> Option<serde_json::Value> {
    let mut items: Vec<Value> = Vec::new();
    // Row 0: the exact reaction (precise tiles), prob from the top candidate.
    if let Some((label, pais)) = label_pais_mjai(ev) {
        let prob = candidates.first().map(|c| c.prob);
        items.push(make_show_item(label, &pais, prob));
    }
    // Remaining rows: the coarse candidate labels. `candidates[0]` is the chosen
    // move, already rendered above with exact tiles, so start from index 1.
    for c in candidates.iter().skip(1) {
        if let Some((label, pais)) = label_pais_candidate(&c.action) {
            items.push(make_show_item(label, &pais, Some(c.prob)));
        }
    }
    wrap_show(items, title)
}

/// Label + tiles for a resolved mjai reaction (the exact chosen move).
fn label_pais_mjai(ev: &MjaiEvent) -> Option<(&'static str, Vec<String>)> {
    let out = match ev {
        MjaiEvent::Dahai { pai, .. } => ("Discard", vec![pai.clone()]),
        MjaiEvent::Reach { pai, .. } => ("Riichi", pai.clone().into_iter().collect()),
        MjaiEvent::Pon { pai, consumed, .. } => ("Pon", with_lead(pai, consumed)),
        MjaiEvent::Chi { pai, consumed, .. } => ("Chi", with_lead(pai, consumed)),
        MjaiEvent::Daiminkan { pai, consumed, .. } => ("Kan", with_lead(pai, consumed)),
        MjaiEvent::Ankan { consumed, .. } => ("Ankan", consumed.to_vec()),
        MjaiEvent::Kakan { pai, consumed, .. } => ("Kakan", with_lead(pai, consumed)),
        MjaiEvent::Hora { .. } => ("Hora", vec![]),
        MjaiEvent::Ryukyoku { .. } => ("Ryukyoku", vec![]),
        MjaiEvent::Kita { .. } => ("Kita", vec!["N".into()]),
        // Passing IS the chosen move on a call window (pon/chi/kan/ron
        // offered, model declines) — show it, or the card would render only
        // the runner-ups and read as recommending the call it just declined.
        MjaiEvent::None => (PASS_LABEL, vec![]),
        _ => return None,
    };
    Some(out)
}

/// Label + tiles for a coarse candidate action string (see API §8), e.g.
/// `dahai:5p`, `reach`, `chi_mid`, `pon`, `kan`, `nukidora`. `dahai:<pai>`
/// carries the exact tile; the rest are move-type labels only.
fn label_pais_candidate(action: &str) -> Option<(&'static str, Vec<String>)> {
    if let Some(pai) = action.strip_prefix("dahai:") {
        return Some(("Discard", vec![pai.to_string()]));
    }
    let out = match action {
        "reach" => ("Riichi", vec![]),
        "pon" => ("Pon", vec![]),
        "chi_low" | "chi_mid" | "chi_high" => ("Chi", vec![]),
        "kan" => ("Kan", vec![]),
        "hora" => ("Hora", vec![]),
        "ryukyoku" => ("Ryukyoku", vec![]),
        "nukidora" => ("Kita", vec!["N".into()]),
        // On a call window the pass option is half the decision (e.g. pon 55%
        // vs pass 45%) — a real ranked row, not noise.
        "none" => (PASS_LABEL, vec![]),
        // Unknown future labels are not shown as a row.
        _ => return None,
    };
    Some(out)
}

/// Build one `show.items` entry: a label, optional tiles, optional probability
/// (rendered as a whole-percent string).
fn make_show_item(label: &str, pais: &[String], prob: Option<f64>) -> Value {
    use serde_json::json;
    let mut item = serde_json::Map::new();
    item.insert("label".into(), json!(label));
    if pais.iter().any(|p| !p.is_empty()) {
        item.insert("pais".into(), json!(pais));
    }
    if let Some(p) = prob {
        item.insert("value".into(), json!(format!("{:.0}%", p * 100.0)));
        // Raw probability for machine consumers (`autoplay::delay::probs`);
        // the formatted `value` above only has integer-percent resolution.
        item.insert("prob".into(), json!(p));
    }
    Value::Object(item)
}

/// Wrap `items` in the `{ "show": { title, items } }` envelope, or `None` when
/// there is nothing to show. The title names the inference source (see
/// [`SHOW_TITLE_LOCAL`] / [`api_show_title`]); the frontend renders it as the
/// Bot Show tile's header.
fn wrap_show(items: Vec<Value>, title: &str) -> Option<Value> {
    use serde_json::json;
    if items.is_empty() {
        return None;
    }
    Some(json!({ "show": { "title": title, "items": items } }))
}

fn take_n<const N: usize>(v: Vec<String>) -> [String; N] {
    let mut it = v.into_iter();
    std::array::from_fn(|_| it.next().unwrap_or_default())
}

/// Build the HUD "Bot Show" recommendation card (`meta.show`) from the ranked
/// candidates, so the built-in bot surfaces its **top-N** suggestions (each with
/// its policy probability) like other bots.
///
/// Every candidate becomes a row, the pass included. Declining a call *is* the
/// decision on a call window, and its probability is the most interesting number
/// on screen at that moment — "Pass 87% / Pon 13%" is the answer to "should I
/// have ponned?". Suppressing it left the previous turn's discard advice sitting
/// there, reading as live advice for a decision that was already over.
///
/// Callers must have established a decision point first ([`is_decision_point`]);
/// a lone pass is not one, and must not reach here.
fn build_show_meta(candidates: &[(BotAction, f32)]) -> Option<serde_json::Value> {
    let items: Vec<Value> = candidates
        .iter()
        .map(|(a, p)| {
            let (label, pais) = label_pais_bot_action(a);
            make_show_item(label, &pais, Some(*p as f64))
        })
        .collect();
    wrap_show(items, SHOW_TITLE_LOCAL)
}

/// Label + tiles for one bot action.
fn label_pais_bot_action(a: &BotAction) -> (&'static str, Vec<String>) {
    match a {
        BotAction::Dahai { pai, .. } => ("Discard", vec![pai.clone()]),
        BotAction::Reach { pai } => (
            "Riichi",
            if pai.is_empty() {
                vec![]
            } else {
                vec![pai.clone()]
            },
        ),
        BotAction::Pon { pai, consumed, .. } => ("Pon", with_lead(pai, consumed)),
        BotAction::Chi { pai, consumed, .. } => ("Chi", with_lead(pai, consumed)),
        BotAction::Daiminkan { pai, consumed, .. } => ("Kan", with_lead(pai, consumed)),
        BotAction::Ankan { consumed } => ("Ankan", consumed.clone()),
        BotAction::Kakan { pai, consumed } => ("Kakan", with_lead(pai, consumed)),
        BotAction::Hora { .. } => ("Hora", vec![]),
        BotAction::Kyushu => ("Ryukyoku", vec![]),
        BotAction::Kita => ("Kita", vec!["N".into()]),
        // No tiles: the row is about *declining* the discard on the table, and
        // drawing that tile here would read as a recommendation to play it.
        BotAction::Pass => (PASS_LABEL, vec![]),
    }
}

/// Map a schema-agnostic [`BotAction`] to Akagi's `MjaiEvent` reply.
fn bot_action_to_mjai(a: BotAction, actor: u8) -> MjaiEvent {
    match a {
        BotAction::Dahai { pai, tsumogiri } => MjaiEvent::Dahai {
            actor,
            pai,
            tsumogiri,
        },
        BotAction::Reach { pai } => MjaiEvent::Reach {
            actor,
            pai: Some(pai),
        },
        BotAction::Pon {
            target,
            pai,
            consumed,
        } => MjaiEvent::Pon {
            actor,
            target,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Chi {
            target,
            pai,
            consumed,
        } => MjaiEvent::Chi {
            actor,
            target,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Daiminkan {
            target,
            pai,
            consumed,
        } => MjaiEvent::Daiminkan {
            actor,
            target,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Ankan { consumed } => MjaiEvent::Ankan {
            actor,
            consumed: take_n(consumed),
        },
        BotAction::Kakan { pai, consumed } => MjaiEvent::Kakan {
            actor,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Hora { target } => MjaiEvent::Hora {
            actor,
            target,
            deltas: None,
            ura_markers: None,
        },
        BotAction::Kyushu => MjaiEvent::Ryukyoku { deltas: None },
        BotAction::Kita => MjaiEvent::Kita {
            actor,
            pai: Some("N".into()),
        },
        BotAction::Pass => MjaiEvent::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::test_http::{mock_http, UNREACHABLE_BASE_URL};

    /// A config handle whose `bot.api` is off (fully offline local model).
    fn cfg_off() -> Arc<RwLock<AppConfig>> {
        Arc::new(RwLock::new(AppConfig::default()))
    }

    /// A config handle with cloud inference pointed at `base_url`.
    fn cfg_api(base_url: &str) -> Arc<RwLock<AppConfig>> {
        let mut c = AppConfig::default();
        c.bot.api = api_on(base_url, &"k".repeat(32));
        Arc::new(RwLock::new(c))
    }

    fn api_on(base_url: &str, key: &str) -> NativeApiConfig {
        NativeApiConfig {
            enabled: true,
            base_url: base_url.to_string(),
            key: key.to_string(),
            model_4p: String::new(),
            model_3p: String::new(),
            proxy_enabled: false,
            proxy: String::new(),
        }
    }

    /// Build the runner the way `build()` does (seeding the session silently).
    async fn bot_with(config: Arc<RwLock<AppConfig>>, notify: NotifyBus) -> NativeBot {
        let initial = config.read().await.bot.api.clone();
        let mut bot = NativeBot::new(0, 4, config, notify).unwrap();
        bot.apply_api_config(&initial, Announce::Silently);
        bot
    }

    /// The opening batch up to our first draw, as the manager would deliver it.
    fn opening() -> Vec<MjaiEvent> {
        vec![
            start_game_4p(0),
            start_kyoku_4p(),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "5p".into(),
            },
        ]
    }

    fn start_game_4p(seat: u8) -> MjaiEvent {
        MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(seat),
            num_players: 4,
        }
    }

    /// A deliberately shapeless hand (~4 shanten): on our draw the model has
    /// only plain discards to choose from, never riichi. Tests that assert "the
    /// local model played" would otherwise have to accept a `reach` too, which
    /// hides whichever path actually produced the move.
    fn start_kyoku_4p() -> MjaiEvent {
        let hand: Vec<String> = [
            "1m", "1m", "3m", "5m", "7m", "9m", "2p", "4p", "6p", "8p", "1s", "3s", "5s",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000, 25000, 25000, 25000],
            tehais: vec![hand.clone(), hand.clone(), hand.clone(), hand],
            num_players: 4,
        }
    }

    /// A `start_kyoku` shaped like the ones the bridge actually emits: our hand
    /// is known, the opponents' are `?`. It matters. Handing the engine four
    /// concrete hands (as `start_kyoku_4p` does, for convenience) lets it see
    /// claims it can never see in a real game, so a test built on that is
    /// testing a game we never get to play.
    fn start_kyoku_4p_hidden(our_hand: [&str; 13]) -> MjaiEvent {
        // The bridge's placeholder for a tile we cannot see (mjai's `?`).
        let hidden = vec!["?".to_string(); 13];
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000; 4],
            tehais: vec![
                our_hand.iter().map(|s| s.to_string()).collect(),
                hidden.clone(),
                hidden.clone(),
                hidden,
            ],
            num_players: 4,
        }
    }

    fn seat1_discards(pai: &str) -> [MjaiEvent; 2] {
        [
            MjaiEvent::Tsumo {
                actor: 1,
                pai: pai.into(),
            },
            MjaiEvent::Dahai {
                actor: 1,
                pai: pai.into(),
                tsumogiri: true,
            },
        ]
    }

    /// Regression (#190), end to end. We hold two 9s, seat 1 discards a 9s: a pon
    /// window. Whatever the model picks, the card must show the pass ranked
    /// against the pon — that comparison *is* the decision. Before this fix the
    /// bot emitted no card at all when it declined, so the tile kept displaying
    /// the previous turn's discard advice as if it were live.
    #[tokio::test]
    async fn a_declined_call_still_produces_a_card_ranking_the_pass() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        let mut events = vec![
            start_game_4p(0),
            start_kyoku_4p_hidden([
                "9s", "9s", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p",
            ]),
        ];
        events.extend(seat1_discards("9s"));

        let resp = bot.react(&events).await.unwrap();

        let meta = resp
            .meta
            .as_ref()
            .expect("a pon window is a decision — it must refresh the card");
        let items = meta["show"]["items"].as_array().unwrap();
        let labels: Vec<&str> = items
            .iter()
            .map(|i| i["label"].as_str().unwrap_or_default())
            .collect();
        assert!(
            labels.contains(&"Pass"),
            "the pass must be a ranked row, got {labels:?}"
        );
        assert!(
            labels.contains(&"Pon"),
            "…alongside the call it is weighed against, got {labels:?}"
        );
        assert!(
            items.iter().all(|i| i["value"].is_string()),
            "every row carries the probability the player is here to read"
        );
    }

    /// The other half of the contract: an opponent discard we cannot call is not
    /// a decision, so the card must be left exactly as it was.
    #[tokio::test]
    async fn an_uncallable_opponent_discard_leaves_the_card_alone() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        let mut events = vec![
            start_game_4p(0),
            // No 9s in hand, and seat 1 is our shimocha so a chi is impossible.
            start_kyoku_4p_hidden([
                "1m", "3m", "5m", "7m", "9m", "2p", "4p", "6p", "8p", "1s", "3s", "5s", "7s",
            ]),
        ];
        events.extend(seat1_discards("9s"));

        let resp = bot.react(&events).await.unwrap();

        assert!(matches!(resp.action, MjaiEvent::None));
        assert!(
            resp.meta.is_none(),
            "nothing was decided, so nothing should replace the card on screen"
        );
    }

    #[tokio::test]
    async fn native_bot_returns_legal_discard_on_own_tsumo() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        // Feed the opening up to our first draw in one batch (as the manager would).
        let resp = bot.react(&opening()).await.unwrap();
        // On our own tsumo we must act — a discard (or riichi/kan/hora), never None.
        assert!(
            !matches!(resp.action, MjaiEvent::None),
            "bot must act on its own tsumo, got None"
        );
        match resp.action {
            MjaiEvent::Dahai { actor, .. } | MjaiEvent::Reach { actor, .. } => {
                assert_eq!(actor, 0)
            }
            MjaiEvent::Ankan { .. } | MjaiEvent::Kakan { .. } | MjaiEvent::Hora { .. } => {}
            other => panic!("unexpected reply on own turn: {other:?}"),
        }
    }

    #[tokio::test]
    async fn native_bot_passes_when_not_its_turn() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        // Opponent (seat 1) draws and discards; we (seat 0) usually can't act.
        let resp = bot
            .react(&[
                start_game_4p(0),
                start_kyoku_4p(),
                MjaiEvent::Tsumo {
                    actor: 1,
                    pai: "9s".into(),
                },
                MjaiEvent::Dahai {
                    actor: 1,
                    pai: "9s".into(),
                    tsumogiri: true,
                },
            ])
            .await
            .unwrap();
        // Either None (nothing to do) or a legal call — must not be one of our
        // own-turn-only actions.
        assert!(
            !matches!(
                resp.action,
                MjaiEvent::Dahai { .. } | MjaiEvent::Reach { .. }
            ),
            "must not discard on someone else's turn: {:?}",
            resp.action
        );
    }

    /// The API path must never stall a live game: an unreachable server falls
    /// back to the embedded model's move and warns the user once.
    #[tokio::test]
    async fn unreachable_server_falls_back_to_the_local_model() {
        use crate::schema::NotifyLevel;
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();
        let mut bot = bot_with(cfg_api(UNREACHABLE_BASE_URL), notify).await;

        let resp = bot.react(&opening()).await.unwrap();
        assert!(
            matches!(resp.action, MjaiEvent::Dahai { actor: 0, .. }),
            "expected the local model's discard, got {:?}",
            resp.action
        );
        let n = rx.try_recv().expect("degrade toast");
        assert_eq!(n.level, NotifyLevel::Warn);
        assert_eq!(n.id.as_deref(), Some(NATIVE_API_HEALTH_ID));

        // And the breaker is now open, so the next decision skips the network.
        assert!(
            !bot.breaker.allows(),
            "a failed call must open the circuit breaker"
        );
    }

    /// A `null` reaction means the server saw no legal action while our local
    /// gate did — a stream mismatch. Play the local move rather than pass a turn.
    #[tokio::test]
    async fn null_reaction_falls_back_to_the_local_model() {
        let (base, served) = mock_http(vec![("200 OK", r#"{"reaction":null}"#.into())]);
        let mut bot = bot_with(cfg_api(&base), crate::event_bus::notify_bus()).await;

        let resp = bot.react(&opening()).await.unwrap();
        assert!(
            matches!(resp.action, MjaiEvent::Dahai { actor: 0, .. }),
            "expected the local model's discard, got {:?}",
            resp.action
        );
        assert_eq!(served.join().unwrap().len(), 1, "the API was consulted");
        // A reachable server, even with a null reaction, is healthy.
        assert!(bot.breaker.allows());
    }

    /// The reach two-step: the server returns a bare `reach`, we re-query with
    /// the reach appended and adopt the discard it names. mjai `reach` must
    /// carry the discard or autoplay stalls.
    #[tokio::test]
    async fn reach_two_step_resolves_the_discard_from_the_follow_up_call() {
        let (base, served) = mock_http(vec![
            (
                "200 OK",
                r#"{"reaction":{"type":"reach","actor":0}}"#.into(),
            ),
            (
                "200 OK",
                r#"{"reaction":{"type":"dahai","actor":0,"pai":"1p","tsumogiri":false}}"#.into(),
            ),
        ]);
        let mut bot = bot_with(cfg_api(&base), crate::event_bus::notify_bus()).await;

        let resp = bot.react(&opening()).await.unwrap();
        assert_eq!(
            resp.action,
            MjaiEvent::Reach {
                actor: 0,
                pai: Some("1p".into()),
            }
        );

        let reqs = served.join().unwrap();
        assert_eq!(reqs.len(), 2, "declare + discard");
        // The follow-up appends the reach we are resolving; the first call can't
        // have carried one (the kyoku had only start_game/start_kyoku/tsumo).
        assert!(
            !reqs[0].contains(r#""reach""#),
            "the declare call must not carry a reach: {}",
            reqs[0]
        );
        assert!(
            reqs[1].contains(r#""reach""#),
            "follow-up must append the reach: {}",
            reqs[1]
        );
    }

    /// If the follow-up call dies, we must still name a discard (from the local
    /// model) rather than emit a bare reach that would stall autoplay.
    ///
    /// The mock is scripted with a single response: it serves the `reach` and
    /// then drops its listener, so the follow-up call finds nothing there.
    #[tokio::test]
    async fn reach_falls_back_to_the_local_discard_when_the_follow_up_fails() {
        let (base, served) = mock_http(vec![(
            "200 OK",
            r#"{"reaction":{"type":"reach","actor":0}}"#.into(),
        )]);
        let mut bot = bot_with(cfg_api(&base), crate::event_bus::notify_bus()).await;

        let resp = bot.react(&opening()).await.unwrap();
        match resp.action {
            MjaiEvent::Reach { actor, pai } => {
                assert_eq!(actor, 0);
                let pai = pai.expect("a reach must name its discard");
                assert!(!pai.is_empty(), "a reach must name its discard");
            }
            // The local model may find no riichi-legal discard at all, in which
            // case the whole reaction is declined and we play the local move.
            MjaiEvent::Dahai { actor: 0, .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(
            served.join().unwrap().len(),
            1,
            "only the declare was served"
        );
        // A dead follow-up trips the breaker, so the next decision stays local.
        assert!(!bot.breaker.allows());
    }

    /// Regression (#227): a forced move — the legal set is a singleton, here
    /// the tsumogiri after our riichi — must be answered by the local model
    /// without spending a metered API call: the server could only return the
    /// same move.
    ///
    /// The mock is scripted with zero responses, so its listener is already
    /// gone when we decide; had the bot tried the network anyway, the refused
    /// connection would have opened the breaker and toasted a degrade warning.
    /// Both staying quiet is the proof that no call was even attempted.
    #[tokio::test]
    async fn a_forced_move_is_answered_locally_without_an_api_call() {
        let (base, served) = mock_http(vec![]);
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();
        let mut bot = bot_with(cfg_api(&base), notify).await;

        let mut events = vec![
            start_game_4p(0),
            // Closed tenpai: 123m 456m 789m 123p + 4p tanki.
            start_kyoku_4p_hidden([
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p",
            ]),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "9s".into(),
            },
            // Our riichi, echoed back through the bus as in a live game.
            MjaiEvent::Reach {
                actor: 0,
                pai: None,
            },
            MjaiEvent::Dahai {
                actor: 0,
                pai: "9s".into(),
                tsumogiri: true,
            },
            MjaiEvent::ReachAccepted { actor: 0 },
        ];
        for seat in 1..4u8 {
            events.push(MjaiEvent::Tsumo {
                actor: seat,
                pai: "1z".into(),
            });
            events.push(MjaiEvent::Dahai {
                actor: seat,
                pai: "1z".into(),
                tsumogiri: true,
            });
        }
        // Our next draw: not the winning 4p, no kan in sight — discarding the
        // drawn tile is the only legal action.
        events.push(MjaiEvent::Tsumo {
            actor: 0,
            pai: "9s".into(),
        });

        let resp = bot.react(&events).await.unwrap();
        assert_eq!(
            resp.action,
            MjaiEvent::Dahai {
                actor: 0,
                pai: "9s".into(),
                tsumogiri: true,
            },
            "riichi forces the tsumogiri"
        );
        // Answered by the local model…
        assert_eq!(
            resp.meta.as_ref().unwrap()["show"]["title"],
            SHOW_TITLE_LOCAL
        );
        // …without touching the network: no request reached the mock, no failed
        // attempt opened the breaker, no degrade toast reached the user.
        assert!(
            bot.breaker.allows(),
            "a skipped call must not touch the breaker"
        );
        assert!(
            rx.try_recv().is_err(),
            "a skipped call must not toast the user"
        );
        assert_eq!(
            served.join().unwrap().len(),
            0,
            "no API call for a forced move"
        );
    }

    // ---------- circuit breaker ----------

    #[test]
    fn breaker_backs_off_exponentially_and_caps() {
        let mut b = Breaker::new();
        assert!(b.allows());
        assert_eq!(b.record_failure(), BREAKER_BASE);
        assert!(!b.allows(), "open right after a failure");
        assert_eq!(b.record_failure(), BREAKER_BASE * 2);
        assert_eq!(b.record_failure(), BREAKER_BASE * 4);
        for _ in 0..10 {
            b.record_failure();
        }
        assert_eq!(b.record_failure(), BREAKER_MAX, "backoff is capped");

        b.record_success();
        assert!(b.allows(), "a success closes the breaker");
        assert_eq!(b.consecutive_failures, 0);
    }

    /// Regression: with no breaker, every decision of an outage paid a full
    /// request timeout (8s at the time, twice for a reach) and the bot missed
    /// every turn of the game. Only the first decision may pay it.
    #[test]
    fn breaker_suppresses_further_calls_until_the_window_expires() {
        let mut b = Breaker::new();
        b.record_failure();
        assert!(!b.allows());
        // The user fixing the settings is an explicit retry signal.
        b.reset();
        assert!(b.allows());
        assert!(b.healthy);
    }

    // ---------- live reconfiguration mid-game ----------

    /// Enabling cloud inference during a hanchan must apply to the very next
    /// decision — the config is re-read per decision, not snapshotted at
    /// `start_game`.
    #[tokio::test]
    async fn enabling_the_api_mid_game_applies_to_the_next_decision() {
        let (base, served) = mock_http(vec![(
            "200 OK",
            r#"{"reaction":{"type":"dahai","actor":0,"pai":"1m","tsumogiri":false},
                "candidates":[{"action":"dahai:1m","prob":0.9}],
                "model":"4p-mock"}"#
                .into(),
        )]);
        let config = cfg_off();
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();
        let mut bot = bot_with(config.clone(), notify).await;

        // Decision 1: API off ⇒ local model, no request.
        let first = bot.react(&opening()).await.unwrap();
        assert!(
            matches!(first.action, MjaiEvent::Dahai { actor: 0, .. }),
            "expected a local discard, got {:?}",
            first.action
        );
        assert!(bot.api.is_none());
        // ...and the HUD card says the local model played it.
        assert_eq!(
            first.meta.as_ref().unwrap()["show"]["title"],
            SHOW_TITLE_LOCAL
        );

        // The user enables cloud inference mid-kyoku.
        config.write().await.bot.api = api_on(&base, &"k".repeat(32));

        // Decision 2 (our next draw): the API is consulted and its move played.
        // Our own discard echoes back through the bus, as it does in a real game.
        let second = bot
            .react(&[
                first.action.clone(),
                MjaiEvent::Tsumo {
                    actor: 0,
                    pai: "3s".into(),
                },
            ])
            .await
            .unwrap();
        assert_eq!(
            second.action,
            MjaiEvent::Dahai {
                actor: 0,
                pai: "1m".into(),
                tsumogiri: false,
            },
            "the server's move must be played"
        );
        assert_eq!(served.join().unwrap().len(), 1, "exactly one API call");
        // The card title now names the model the server says answered.
        assert_eq!(
            second.meta.as_ref().unwrap()["show"]["title"],
            "Akagi · 4p-mock"
        );

        // ...and the user is told, on the same channel the status LED listens to.
        let n = rx.try_recv().expect("a toast confirming the switch");
        assert_eq!(n.id.as_deref(), Some(NATIVE_API_HEALTH_ID));
        assert_eq!(n.level, crate::schema::NotifyLevel::Info);
        assert!(n.title.contains("Cloud inference on"), "got {}", n.title);
    }

    /// Turning it back off mid-game returns to the local model immediately and
    /// clears the "online API" indicator.
    #[tokio::test]
    async fn disabling_the_api_mid_game_returns_to_the_local_model() {
        let config = cfg_api(UNREACHABLE_BASE_URL);
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();
        let mut bot = bot_with(config.clone(), notify).await;
        assert!(bot.api.is_some(), "seeded from the config");

        config.write().await.bot.api.enabled = false;
        let resp = bot.react(&opening()).await.unwrap();

        assert!(bot.api.is_none(), "session dropped");
        assert!(
            matches!(resp.action, MjaiEvent::Dahai { actor: 0, .. }),
            "local model plays"
        );
        let n = rx.try_recv().expect("an off toast");
        assert_eq!(n.id.as_deref(), Some(NATIVE_API_HEALTH_ID));
        // `info` (not warn) so the frontend clears the degraded indicator.
        assert_eq!(n.level, crate::schema::NotifyLevel::Info);
    }

    /// Correcting a mistyped key must rebuild the client *and* clear the open
    /// breaker, so the corrected key is tried on the next decision rather than
    /// after the backoff window.
    #[tokio::test]
    async fn changing_the_key_rebuilds_the_client_and_clears_the_breaker() {
        let notify = crate::event_bus::notify_bus();
        let mut bot = bot_with(cfg_api(UNREACHABLE_BASE_URL), notify).await;

        bot.breaker.record_failure();
        assert!(!bot.breaker.allows());

        let fixed = api_on(UNREACHABLE_BASE_URL, &"z".repeat(32));
        bot.apply_api_config(&fixed, Announce::ToUser);

        assert!(
            bot.breaker.allows(),
            "a settings change is an explicit retry signal"
        );
        assert_eq!(bot.api.as_ref().unwrap().key, "z".repeat(32));
    }

    #[tokio::test]
    async fn toggling_the_proxy_flag_rebuilds_with_the_effective_proxy() {
        let notify = crate::event_bus::notify_bus();
        // A proxy is typed but the toggle is off: the session must stay direct.
        let mut cfg = AppConfig::default();
        cfg.bot.api = api_on(UNREACHABLE_BASE_URL, &"k".repeat(32));
        cfg.bot.api.proxy = "socks5://127.0.0.1:1080".into();
        let mut bot = bot_with(Arc::new(RwLock::new(cfg)), notify).await;
        assert_eq!(
            bot.api.as_ref().unwrap().proxy,
            "",
            "toggle off ⇒ client built direct even with a proxy set"
        );

        // Flip the toggle on (same URL/key/proxy string): the client rebuilds
        // and now carries the effective proxy.
        let mut on = api_on(UNREACHABLE_BASE_URL, &"k".repeat(32));
        on.proxy = "socks5://127.0.0.1:1080".into();
        on.proxy_enabled = true;
        bot.apply_api_config(&on, Announce::ToUser);
        assert_eq!(
            bot.api.as_ref().unwrap().proxy,
            "socks5://127.0.0.1:1080",
            "toggle on ⇒ rebuilt with the effective proxy"
        );
    }

    /// Switching the model id keeps the same server but re-requests under the new
    /// model; an unchanged config must not churn the client or toast.
    #[tokio::test]
    async fn model_switch_updates_the_session_and_no_op_config_is_quiet() {
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();
        let mut bot = bot_with(cfg_api(UNREACHABLE_BASE_URL), notify).await;

        let same = api_on(UNREACHABLE_BASE_URL, &"k".repeat(32));
        bot.apply_api_config(&same, Announce::ToUser);
        assert!(rx.try_recv().is_err(), "an unchanged config must be silent");

        let mut switched = same.clone();
        switched.model_4p = "4p-strong".into();
        bot.apply_api_config(&switched, Announce::ToUser);
        assert_eq!(bot.api.as_ref().unwrap().model, "4p-strong");
        let n = rx.try_recv().expect("a toast naming the new model");
        assert!(n.title.contains("updated"), "got {}", n.title);
        assert!(n.body.unwrap_or_default().contains("4p-strong"));
    }

    // ---------- API-backed native bot: request shaping ----------

    fn hand13() -> Vec<String> {
        (0..13).map(|i| format!("{}p", (i % 9) + 1)).collect()
    }

    #[test]
    fn api_event_censors_other_seats_and_strips_num_players() {
        let sk = MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000, 25000, 25000, 25000],
            tehais: vec![hand13(), hand13(), hand13(), hand13()],
            num_players: 4,
        };
        let v = to_api_event(&sk, 2, 4);
        assert_eq!(v["type"], "start_kyoku");
        assert!(
            v.get("num_players").is_none(),
            "num_players must be stripped for the API"
        );
        let tehais = v["tehais"].as_array().unwrap();
        assert_eq!(tehais.len(), 4);
        // Our own seat (2) is revealed; all others are 13 "?".
        assert_ne!(tehais[2][0], "?");
        for i in [0usize, 1, 3] {
            let hand = tehais[i].as_array().unwrap();
            assert_eq!(hand.len(), 13);
            assert!(hand.iter().all(|t| t == "?"), "seat {i} must be hidden");
        }

        // Draws: ours revealed, others censored.
        let mine = to_api_event(
            &MjaiEvent::Tsumo {
                actor: 2,
                pai: "5p".into(),
            },
            2,
            4,
        );
        assert_eq!(mine["pai"], "5p");
        let theirs = to_api_event(
            &MjaiEvent::Tsumo {
                actor: 1,
                pai: "5p".into(),
            },
            2,
            4,
        );
        assert_eq!(theirs["pai"], "?");
    }

    #[test]
    fn api_event_pads_3p_arrays_to_length_four() {
        let sg = MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 3,
        };
        let v = to_api_event(&sg, 0, 3);
        assert_eq!(v["names"].as_array().unwrap().len(), 4);
        assert_eq!(v["names"][3], "");
        assert!(v.get("num_players").is_none());
        assert!(v.get("id").is_none(), "start_game reduced to type + names");

        let sk = MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "1s".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![35000, 35000, 35000],
            tehais: vec![hand13(), hand13(), hand13()],
            num_players: 3,
        };
        let v = to_api_event(&sk, 0, 3);
        assert_eq!(v["scores"].as_array().unwrap().len(), 4);
        assert_eq!(v["scores"][3], 0);
        let tehais = v["tehais"].as_array().unwrap();
        assert_eq!(tehais.len(), 4);
        // Phantom 4th seat is a hidden hand.
        assert_eq!(tehais[3].as_array().unwrap().len(), 13);
        assert_eq!(tehais[3][0], "?");
        // Real other seat (1) still censored.
        assert!(tehais[1].as_array().unwrap().iter().all(|t| t == "?"));
    }

    #[test]
    fn api_event_strips_predicted_reach_pai() {
        let r = MjaiEvent::Reach {
            actor: 0,
            pai: Some("5p".into()),
        };
        let v = to_api_event(&r, 0, 4);
        assert_eq!(v["type"], "reach");
        assert_eq!(v["actor"], 0);
        assert!(
            v.get("pai").is_none(),
            "predicted reach pai must be stripped"
        );
    }

    #[test]
    fn show_meta_from_reaction_carries_label_pai_and_prob() {
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: "W".into(),
            tsumogiri: false,
        };
        let cands = vec![Candidate {
            action: "dahai:W".into(),
            prob: 0.83,
        }];
        let meta = build_show_meta_mjai(&ev, &cands, "Akagi · 4p-x").unwrap();
        assert_eq!(meta["show"]["title"], "Akagi · 4p-x");
        let item = &meta["show"]["items"][0];
        assert_eq!(item["label"], "Discard");
        assert_eq!(item["pais"][0], "W");
        assert_eq!(item["value"], "83%");
    }

    #[test]
    fn show_meta_carries_legal_ops_omitted_by_policy_top_n() {
        let mut meta = Some(serde_json::json!({
            "show": {
                "items": [
                    {"label": "Pon", "value": "27%"},
                    {"label": "Pass", "value": "27%"},
                    {"label": "Chi", "value": "26%"}
                ]
            }
        }));
        attach_legal_ops(
            &mut meta,
            &["pass", "chi", "pon", "kan"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );

        assert_eq!(
            meta.unwrap()["show"]["legal_ops"],
            serde_json::json!(["pass", "chi", "pon", "kan"]),
        );
    }

    /// The card title names the API model that served the decision: the
    /// server's report wins, then the configured id, then a generic "Online".
    /// Blanks don't count as a model id.
    #[test]
    fn api_show_title_prefers_served_then_configured_then_online() {
        assert_eq!(api_show_title(Some("4p-x"), Some("4p-cfg")), "Akagi · 4p-x");
        assert_eq!(api_show_title(None, Some("4p-cfg")), "Akagi · 4p-cfg");
        assert_eq!(api_show_title(Some(""), Some(" ")), "Akagi · Online");
        assert_eq!(api_show_title(None, None), "Akagi · Online");
        assert_eq!(api_show_title(Some(" 4p-x "), None), "Akagi · 4p-x");
    }

    #[test]
    fn local_show_meta_lists_top_candidates_with_probs() {
        let cands = vec![
            (
                BotAction::Dahai {
                    pai: "1m".into(),
                    tsumogiri: false,
                },
                0.6f32,
            ),
            (BotAction::Reach { pai: "2p".into() }, 0.3f32),
            (
                BotAction::Dahai {
                    pai: "9s".into(),
                    tsumogiri: true,
                },
                0.1f32,
            ),
        ];
        let meta = build_show_meta(&cands).unwrap();
        assert_eq!(
            meta["show"]["title"], SHOW_TITLE_LOCAL,
            "a local decision must be titled as such"
        );
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3, "should surface all three candidates");
        assert_eq!(items[0]["label"], "Discard");
        assert_eq!(items[0]["pais"][0], "1m");
        assert_eq!(items[0]["value"], "60%");
        assert_eq!(items[1]["label"], "Riichi");
        assert_eq!(items[1]["pais"][0], "2p");
        assert_eq!(items[1]["value"], "30%");
        assert_eq!(items[2]["pais"][0], "9s");
        assert_eq!(items[2]["value"], "10%");
    }

    /// Regression (#190): declining a call used to produce no card at all, so
    /// the tile kept showing the previous turn's discard advice — live-looking
    /// advice for a decision that was already over. The pass is the decision
    /// here, and its probability against the call it turned down is the whole
    /// point of the card.
    #[test]
    fn local_show_meta_ranks_the_pass_against_the_call_it_declined() {
        let cands = vec![
            (BotAction::Pass, 0.9f32),
            (
                BotAction::Pon {
                    target: 0,
                    pai: "1m".into(),
                    consumed: vec!["1m".into(), "1m".into()],
                },
                0.1f32,
            ),
        ];

        let meta = build_show_meta(&cands).expect("a declined call still has a card to show");
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "both the pass and the pon it beat");
        assert_eq!(items[0]["label"], "Pass");
        assert_eq!(items[0]["value"], "90%");
        assert!(
            items[0].get("pais").is_none(),
            "the pass row must draw no tile — it would read as a discard suggestion"
        );
        assert_eq!(items[1]["label"], "Pon");
        assert_eq!(items[1]["value"], "10%");
    }

    /// The other half of #190: a lone pass is *not* a decision. `WaitResponse`
    /// opens for every seat as soon as any seat can claim, and the engine always
    /// offers `Pass` there — so a seat with nothing to claim gets `[Pass]` back.
    /// Showing "Pass 100%" for that would replace the card on every opponent
    /// discard someone *else* could call.
    #[test]
    fn a_lone_pass_is_not_a_decision_point() {
        assert!(
            !is_decision_point(&[]),
            "no legal action at all is not a decision"
        );
        assert!(
            !is_decision_point(&[(BotAction::Pass, 1.0)]),
            "someone else's call window is not our decision"
        );
    }

    #[test]
    fn a_choice_is_a_decision_point() {
        let call_window = [
            (BotAction::Pass, 0.9f32),
            (
                BotAction::Pon {
                    target: 0,
                    pai: "1m".into(),
                    consumed: vec!["1m".into(), "1m".into()],
                },
                0.1f32,
            ),
        ];
        assert!(
            is_decision_point(&call_window),
            "pass vs pon is exactly the decision the card exists to explain"
        );

        // Our own turn: no pass on offer, and even a single forced discard is a
        // decision the player wants to see.
        let forced_discard = [(
            BotAction::Dahai {
                pai: "1m".into(),
                tsumogiri: true,
            },
            1.0f32,
        )];
        assert!(is_decision_point(&forced_discard));
    }

    #[test]
    fn api_show_meta_lists_candidates_with_probs() {
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cands = vec![
            Candidate {
                action: "dahai:5p".into(),
                prob: 0.7,
            },
            Candidate {
                action: "reach".into(),
                prob: 0.2,
            },
            Candidate {
                action: "dahai:9m".into(),
                prob: 0.1,
            },
        ];
        let meta = build_show_meta_mjai(&ev, &cands, "Akagi · Online").unwrap();
        assert_eq!(meta["show"]["title"], "Akagi · Online");
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        // Row 0 = exact chosen move; rows 1..= coarse candidate labels.
        assert_eq!(items[0]["label"], "Discard");
        assert_eq!(items[0]["pais"][0], "5p");
        assert_eq!(items[0]["value"], "70%");
        assert_eq!(items[1]["label"], "Riichi");
        assert_eq!(items[1]["value"], "20%");
        assert_eq!(items[2]["label"], "Discard");
        assert_eq!(items[2]["pais"][0], "9m");
        assert_eq!(items[2]["value"], "10%");
    }

    /// A call window where the model declines (pon 35% vs pass 65%): the card
    /// must show the chosen pass as row 0 AND keep the pass runner-up rows —
    /// dropping them made the card read as recommending the declined call.
    #[test]
    fn api_show_meta_ranks_the_pass_alongside_the_call() {
        // Chosen move is the pass: row 0 = "Pass" with candidates[0]'s prob.
        let cands = vec![
            Candidate {
                action: "none".into(),
                prob: 0.65,
            },
            Candidate {
                action: "pon".into(),
                prob: 0.35,
            },
        ];
        let meta = build_show_meta_mjai(&MjaiEvent::None, &cands, "Akagi · Online").unwrap();
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        // Same word as the local path (#190): one card, one vocabulary,
        // whichever inference path answered.
        assert_eq!(items[0]["label"], "Pass");
        assert_eq!(items[0]["value"], "65%");
        assert_eq!(items[1]["label"], "Pon");
        assert_eq!(items[1]["value"], "35%");

        // Chosen move is the call: the pass runner-up still gets a row.
        let ev = MjaiEvent::Pon {
            actor: 0,
            target: 3,
            pai: "4m".into(),
            consumed: ["4m".into(), "4m".into()],
        };
        let cands = vec![
            Candidate {
                action: "pon".into(),
                prob: 0.55,
            },
            Candidate {
                action: "none".into(),
                prob: 0.45,
            },
        ];
        let meta = build_show_meta_mjai(&ev, &cands, "Akagi · Online").unwrap();
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["label"], "Pon");
        assert_eq!(items[0]["value"], "55%");
        assert_eq!(items[1]["label"], "Pass");
        assert_eq!(items[1]["value"], "45%");
    }

    /// `build` constructs without any network I/O, in either mode and either
    /// player count — a bogus URL is fine, nothing connects at build time. The
    /// seeded session must not toast (a game starting with the API on is not news).
    #[tokio::test]
    async fn build_constructs_without_network_io_and_seeds_quietly() {
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();

        assert!(build(0, 4, cfg_off(), notify.clone()).await.is_ok());

        let on = cfg_api(UNREACHABLE_BASE_URL);
        assert!(on.read().await.bot.api.is_active());
        assert!(build(0, 4, on.clone(), notify.clone()).await.is_ok());
        assert!(build(0, 3, on, notify).await.is_ok());

        assert!(rx.try_recv().is_err(), "seeding the session must be silent");
    }

    #[tokio::test]
    async fn api_health_toasts_only_on_transition() {
        use crate::schema::NotifyLevel;
        let notify = crate::event_bus::notify_bus();
        let mut rx = notify.subscribe();
        let mut bot = bot_with(cfg_api(UNREACHABLE_BASE_URL), notify).await;

        // healthy → degraded: one warn toast naming the error.
        bot.record_health(false, Some("boom"));
        let n = rx.try_recv().expect("degrade toast");
        assert_eq!(n.level, NotifyLevel::Warn);
        assert_eq!(n.id.as_deref(), Some(NATIVE_API_HEALTH_ID));
        assert!(n.body.unwrap_or_default().contains("boom"));

        // still degraded: no repeat toast (would spam once per turn).
        bot.record_health(false, Some("boom again"));
        assert!(
            rx.try_recv().is_err(),
            "must not toast without a transition"
        );

        // recovered: one success toast, same id (replaces the warning).
        bot.record_health(true, None);
        let n = rx.try_recv().expect("recover toast");
        assert_eq!(n.level, NotifyLevel::Success);
        assert_eq!(n.id.as_deref(), Some(NATIVE_API_HEALTH_ID));

        // still healthy: silent.
        bot.record_health(true, None);
        assert!(rx.try_recv().is_err());
    }
}
