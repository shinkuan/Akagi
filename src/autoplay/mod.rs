//! Autoplay: perform the bot's decisions in the real client, via CDP.
//!
//! Data flow: bot decision (an `MjaiEvent`) joins the latest
//! `GameStateSnapshot` and `legal_actions` from the `riichienv-core` game
//! state, and the platform impl plans a sequence of `Step`s. Action
//! availability is sourced from the riichi engine, not from the platform
//! protocol parser, so the same logic works across platforms.
//!
//! How a step reaches the game differs by platform, and the difference is
//! large enough to be worth stating up front:
//!
//! - **Mahjong Soul** renders to a canvas and exposes nothing to script, so
//!   the only way in is synthesised mouse input at reconstructed coordinates.
//!   Everything expensive here exists because of that: coordinate tables,
//!   candidate-row index arithmetic, click verification and retry.
//! - **Tenhou** is HTML and script, so its own input path can be driven
//!   directly: its action buttons are DOM elements, and its discard handler
//!   takes a tile index. None of the above applies — nothing is aimed at a
//!   pixel, so nothing can miss one. (Writing frames onto the socket instead
//!   was tried and freezes the board; see [`tenhou`].)
//!
//! Reach is two inputs on both platforms — declare, then discard — and both
//! take the tile from the bot's own `Reach { pai }` and perform them in one
//! plan. Neither client acts on the declaration alone: Mahjong Soul holds its
//! reach popup open, and Tenhou sits on its clock until the tile arrives.
//!
//! Every response arriving here is a decision the riichi engine asked for —
//! `bot::manager` does not flush to the bot otherwise — so an `MjaiEvent::None`
//! means *decline*, and never "the bot had nothing to say".
//!
//! Module layout:
//! - [`context`] — shared state between the chromium capture backend
//!   and this manager (page handle + canvas rect cache).
//! - [`platform`] — `PlatformAutoplay` trait + the `Step` types
//!   (`Click` / `Sleep` / `AwaitReady` / `DomClick` / `Discard`).
//! - [`majsoul`] — the production Majsoul implementation: 16:9
//!   coordinate tables (ported from the Python autoplay in
//!   <https://github.com/shinkuan/Akagi>) + plan dispatch covering all mjai
//!   action types.
//! - [`tenhou`] — the Tenhou implementation: press the client's own action
//!   buttons, or call its discard handler with a tile index.
//!   [`tenhou::inject`] is what makes that handler reachable.
//! - [`tenhou_state`] — the bridge-written slot holding Tenhou's hand at
//!   tile-index resolution plus its current decision window, which is what
//!   lets an mjai tile string name a physical copy at all.
//! - [`cdp_input`] — chromiumoxide wrappers (`dispatch_click`,
//!   `evaluate_canvas_rect`, and the Tenhou DOM helpers: action-button
//!   selectors, the readiness probe, and the call into the injected
//!   discard handler).
//! - [`manager`] — the long-lived `AutoplayManager` task that owns
//!   per-game state and drives the plan.
//! - [`verify`] — did the click land? Counts the client's own uplink
//!   input commands (bumped by the Majsoul bridge) so the manager can
//!   tell a click that registered from one the UI swallowed, and retry.
//!   Canvas clicks only — the Tenhou steps run the client's own handlers,
//!   which report their own failure.
//! - [`delay`] — how long a decision appears to take, shared by every
//!   platform. Its inputs besides the decision itself: [`table`] (facts read
//!   off the table — loose tile, safe against every riichi) and [`advisor`]
//!   (a second opinion from the built-in bot on how obvious the move is to a
//!   person). Neither can change the action.
//!
//! Entry point: [`manager::run_autoplay_manager`].

pub mod advisor;
pub mod budget;
pub mod cdp_input;
pub mod context;
pub mod delay;
pub mod inject;
pub mod majsoul;
pub mod manager;
pub mod platform;
pub mod riichi_city;
pub mod table;
pub mod tenhou;
pub mod tenhou_state;
pub mod verify;

pub use budget::{BudgetSource, SharedTimeBudget, TimeBudget};
pub use context::{AutoplayContext, CanvasRect};
pub use manager::run_autoplay_manager;
pub use platform::{ActionContext, PlanResult, PlatformAutoplay, Step};
pub use verify::{InputKind, InputWatch, SharedInputWatch};
