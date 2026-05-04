//! Autoplay: translate bot decisions into UI clicks via CDP.
//!
//! Architecture and design rationale:
//! `claude_plan_autoplay-majsoul-input-mutable-codd.md` (the approved
//! plan covers data flow, two-step reach handling, and the rationale
//! for sourcing action availability from `riichienv-core` rather than
//! the platform-protocol parser).
//!
//! Module layout:
//! - [`context`] — shared state between the chromium capture backend
//!   and this manager (page handle + canvas rect cache).
//! - [`platform`] — `PlatformAutoplay` trait + the click-step types.
//!   Tenhou-ready: a future `tenhou` impl can use a different `Step`
//!   variant for DOM clicks or WebSocket inject.
//! - [`majsoul`] — the production Majsoul implementation: 16:9
//!   coordinate tables ported from the reference Akagi Python file +
//!   plan dispatch covering all mjai action types.
//! - [`cdp_input`] — chromiumoxide wrappers (`dispatch_click`,
//!   `evaluate_canvas_rect`).
//! - [`manager`] — the long-lived `AutoplayManager` task that owns
//!   per-game state and drives the click sequence.
//!
//! Entry point: [`manager::run_autoplay_manager`].

pub mod cdp_input;
pub mod context;
pub mod majsoul;
pub mod manager;
pub mod platform;

pub use context::{AutoplayContext, CanvasRect};
pub use manager::run_autoplay_manager;
pub use platform::{ActionContext, PlanResult, PlatformAutoplay, ReachState, Step};
