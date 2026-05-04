//! Platform-agnostic autoplay surface.
//!
//! `PlatformAutoplay` is the only thing `AutoplayManager` knows about —
//! by holding `Arc<dyn PlatformAutoplay>` it can drop in a Tenhou impl
//! later (DOM-based clicks or direct WS inject) without touching the
//! manager's bus subscription / timing logic.

use crate::config::MajsoulAutoplayConfig;
use crate::game_state::snapshot::GameStateSnapshot;
use crate::schema::MjaiEvent;
use riichienv_core::action::Action;

/// One step in the click sequence the manager will execute.
///
/// The 16:9-normalised coordinates match the convention used by the
/// reference Akagi `LOCATION` table — see
/// `reference/majsoul/autoplay_majsoul.py:13-65`.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Click at a normalised 16:9 point on the game canvas.
    Click { x_norm: f64, y_norm: f64 },
    /// Pause for `duration_ms` before the next step. Used for the
    /// pre-click "thinking" delay and the inter-click gap inside one
    /// action.
    Sleep { duration_ms: u32 },
}

/// State machine for handling Majsoul's fused reach+discard. See
/// `claude_plan_autoplay-majsoul-input-mutable-codd.md` §7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachState {
    Idle,
    /// Path B: we clicked the reach button and injected a synthetic
    /// `Reach` event into MjaiBus. The next `Dahai` from the bot is the
    /// riichi tile and must be clicked even though the bot didn't pre-fill
    /// `Reach.pai`.
    AwaitingDahai,
}

/// Everything the platform impl needs to translate one bot decision
/// into a concrete click sequence.
pub struct ActionContext<'a> {
    /// The bot's chosen action (from `BotResponseBus`).
    pub action: &'a MjaiEvent,
    /// Live game state from the riichi engine.
    pub snapshot: &'a GameStateSnapshot,
    /// Currently legal actions for `our_seat`, sourced from the riichi
    /// engine's `_get_legal_actions_internal`. The platform impl uses
    /// this to:
    /// - decide which action button (chi/pon/kan/...) is in which
    ///   on-screen position, by intersecting with the platform's
    ///   priority table;
    /// - enumerate chi/pon/kan candidate combinations when the bot's
    ///   action is ambiguous (multiple `consume_tiles`).
    pub legal_actions: &'a [Action],
    /// Bot's seat.
    pub our_seat: u8,
    /// The most recent tile any seat discarded — needed to disambiguate
    /// chi/pon target.
    pub last_kawa_tile: Option<&'a str>,
    /// The tile we drew this turn, if any. Used to detect tsumohai
    /// position when emitting `dahai`.
    pub last_self_tsumo: Option<&'a str>,
    /// True from the moment the server confirms our riichi until the
    /// kyoku ends. While set, dahai clicks are suppressed (Majsoul auto-
    /// discards in riichi mode).
    pub self_riichi_accepted: bool,
    /// Reach two-step state (see [`ReachState`]).
    pub reach_state: ReachState,
    /// 3 (sanma) or 4 (yonma).
    pub num_players: u8,
    /// Per-platform config knobs (delays, mouse-move emission, ...).
    pub cfg: &'a MajsoulAutoplayConfig,
}

/// Output of `PlatformAutoplay::plan`. Captures both the click sequence
/// and the side-effect signal the manager needs to fulfil Path B reach
/// (inject a synthetic `Reach` event back to MjaiBus so the bot emits
/// the follow-up dahai).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanResult {
    pub steps: Vec<Step>,
    /// When `true`, the manager should send a synthetic
    /// `MjaiEvent::Reach { actor: our_seat, pai: None }` onto MjaiBus
    /// after the steps run, and transition `ReachState` to
    /// `AwaitingDahai`. Set only when the bot's `Reach` event omits
    /// `pai` and the platform needs a follow-up dahai.
    pub inject_reach_for_followup: bool,
    /// When `true`, the next bot Dahai for our seat is the riichi tile
    /// and should be clicked even if the manager has flipped its
    /// `self_riichi_accepted` gate. Currently always set together with
    /// `inject_reach_for_followup`; kept as a separate flag so future
    /// platforms can express other "treat next dahai specially"
    /// situations.
    pub awaiting_riichi_dahai: bool,
}

pub trait PlatformAutoplay: Send + Sync {
    /// Translate the bot's action into a click sequence + side-effect
    /// hints. Pure: must not perform IO. The manager handles the actual
    /// CDP dispatch and bus injection.
    fn plan(&self, ctx: &ActionContext) -> PlanResult;
}
