//! Lua override for the pre-click delay policy.
//!
//! Users drop a `delay.lua` next to their config file defining:
//!
//! ```lua
//! function decide_delay(ctx)
//!   return { delay_ms = 2300 }
//! end
//! ```
//!
//! `delay_ms` is the target **total** thinking time for the decision
//! window (the interval the server observes), not a sleep length — the
//! caller subtracts time already consumed and the click sequence's own
//! duration (`ctx.click_ms`).
//!
//! The script owns the whole computation. Every timing setting — this
//! decision's draw from each Settings range — is in `ctx.settings`, with
//! what the server and the clicks take, and nothing is added on top: the
//! UI-readiness floor, the dealing-animation wait and the server's time
//! limit are the script's to apply (the bundled default does, exactly as
//! the host used to). A script that ignores them can click before the UI
//! exists or run into the client's auto-discard.
//!
//! Safety contract (enforced here, not trusted to the script):
//! - Restricted stdlib: math/string/table plus a scrubbed base library —
//!   no io/os/debug, no `load`/`dofile`/`loadfile` (bytecode and
//!   filesystem escapes), no `pcall`/`xpcall` (they could swallow the
//!   runaway-guard abort), no `string.dump`.
//! - A hard allocation ceiling ([`MEMORY_LIMIT_BYTES`]) — allocation
//!   failure becomes a normal Lua error, not a process abort.
//! - Instruction budget + wall-clock deadline via a VM hook, active
//!   both while the chunk's top level runs at load time and during
//!   every `decide_delay` call.
//! - Any failure (missing file is not a failure; syntax error, runtime
//!   error, timeout, wrong return shape, out-of-range delay) falls back
//!   to the built-in model and is logged **once** per distinct error,
//!   not once per hand.
//! - The script never sees or influences the chosen action.

use super::{DelayDecision, DelayInput};
use mlua::{Function, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};
use tracing::{info, warn};

/// The default delay policy, generated as `delay.lua` next to the
/// config file on first use (see [`ScriptHost::maybe_reload`]).
pub const DEFAULT_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/delay_default.lua"
));

/// Every bundled default shipped by an earlier version, verbatim. An
/// existing `delay.lua` identical (modulo line endings) to one of these
/// is an untouched auto-generated copy of an outdated default, so
/// [`ScriptHost::ensure_default`] replaces it with [`DEFAULT_SCRIPT`];
/// anything else is a user edit and is never touched.
///
/// When changing `assets/delay_default.lua`, copy the version being
/// replaced into `assets/delay_default_superseded/<short-commit>.lua`
/// and add it here — otherwise existing installs keep the old script.
const SUPERSEDED_DEFAULT_SCRIPTS: &[&str] = &[
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/f8c7cfd.lua"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/1893c05.lua"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/cb1f146.lua"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/f1f2842.lua"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/2e5d892.lua"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/b881105.lua"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/delay_default_superseded/fa636c2.lua"
    )),
];

/// Overwrite the script at `path` with [`DEFAULT_SCRIPT`] iff it is an
/// unmodified copy of an earlier bundled default; returns whether it did.
/// A missing, unreadable or user-edited file is left alone.
///
/// Run once at startup, whatever the autoplay switch and delay mode say: the
/// autoplay manager's own check only happens at the first paced decision
/// (and not at all while autoplay is off), so an updated install would keep
/// showing — and, once switched on, briefly running — the old default.
pub fn refresh_outdated_default(path: &Path) -> bool {
    let Ok(existing) = std::fs::read_to_string(path) else {
        return false; // missing or unreadable — `maybe_reload` logs the latter
    };
    if existing == DEFAULT_SCRIPT || !is_superseded_default(&existing) {
        return false;
    }
    match write_atomic(path, DEFAULT_SCRIPT) {
        Ok(()) => {
            info!(
                "delay script was an outdated bundled default — updated: {}",
                path.display()
            );
            true
        }
        Err(e) => {
            warn!(
                "could not update outdated delay script at {} — keeping it: {e}",
                path.display()
            );
            false
        }
    }
}

/// What the `delay.lua` on disk is, next to the bundled default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptState {
    /// No file (it is generated on first use).
    Missing,
    /// The current bundled default.
    Current,
    /// An unmodified older default ([`refresh_outdated_default`] updates it).
    Outdated,
    /// Anything else: the user's own edits, never replaced without asking.
    Modified,
}

/// Classify the script at `path`. An unreadable file counts as modified:
/// it is not ours to judge.
pub fn script_state(path: &Path) -> ScriptState {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ScriptState::Missing,
        Ok(s) if s.replace("\r\n", "\n") == DEFAULT_SCRIPT.replace("\r\n", "\n") => {
            ScriptState::Current
        }
        Ok(s) if is_superseded_default(&s) => ScriptState::Outdated,
        _ => ScriptState::Modified,
    }
}

/// A short, stable name for the bundled default: what the frontend keys
/// "already asked about this one" on, so an edited script is offered each
/// new default once.
pub fn default_version() -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(DEFAULT_SCRIPT.replace("\r\n", "\n").as_bytes());
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// Replace the script at `path` with the bundled default, first copying an
/// existing file to `delay.lua.<YYYYMMDD-HHMMSS>.bak` beside it. Returns
/// the backup's path, if there was a file to back up.
pub fn replace_with_default(path: &Path) -> std::io::Result<Option<PathBuf>> {
    let backup = if path.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{stamp}.bak"));
        let backup = path.with_file_name(name);
        std::fs::copy(path, &backup)?;
        Some(backup)
    } else {
        None
    };
    write_atomic(path, DEFAULT_SCRIPT)?;
    info!(
        "delay script replaced by the bundled default: {}",
        path.display()
    );
    Ok(backup)
}

/// Is `content` an unmodified copy of an earlier bundled default? Line
/// endings are normalized on both sides: the file on disk or the
/// compiled-in constant may carry CRLF depending on the checkout and
/// platform, and a pure EOL difference is not a user edit.
fn is_superseded_default(content: &str) -> bool {
    let normalized = content.replace("\r\n", "\n");
    SUPERSEDED_DEFAULT_SCRIPTS
        .iter()
        .any(|old| old.replace("\r\n", "\n") == normalized)
}

/// Hard sanity range for a script-provided target (10 minutes). Values
/// outside are treated as a script bug, not clamped silently.
const MAX_SCRIPT_DELAY_MS: f64 = 600_000.0;
/// Wall-clock deadline for one script call.
const CALL_DEADLINE: Duration = Duration::from_millis(50);
/// Wall-clock deadline for running the chunk's top level at load time.
/// More generous than [`CALL_DEADLINE`]: precomputing tables at file
/// scope is legitimate, and a load happens once per edit, not per hand.
const COMPILE_DEADLINE: Duration = Duration::from_millis(250);
/// The VM hook fires every N instructions to check the deadline.
const HOOK_EVERY_N_INSTRUCTIONS: u32 = 1_000;
/// Instruction budget for one call (hook fires × N).
const MAX_HOOK_FIRES: u64 = 10_000; // = 10M instructions
/// Lua allocation ceiling. Far above anything a delay policy needs, but
/// it turns a runaway `string.rep`-style allocation into a catchable
/// Lua error — without a limit, mlua's allocator aborts the whole
/// process when the system allocation fails, and the VM hook cannot
/// intervene because C functions never tick it.
const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
/// Base-library globals scrubbed from the VM. `Lua::new_with` always
/// loads the base library regardless of the `StdLib` mask, and these
/// break the sandbox or the runaway guard: `load`/`loadstring` accept
/// precompiled bytecode which Lua 5.4 does not validate (crafted
/// bytecode is native-code execution), `dofile`/`loadfile` reach the
/// filesystem (`dofile()` with no argument blocks reading stdin), and
/// `pcall`/`xpcall` would let a script catch the guard's abort error
/// and keep spinning.
const SCRUBBED_GLOBALS: &[&str] = &[
    "load",
    "loadstring",
    "dofile",
    "loadfile",
    "pcall",
    "xpcall",
];

/// A loaded, compiled delay script.
pub struct DelayScript {
    lua: Lua,
    func: Function,
    /// Last error reported for this script instance; used to log each
    /// distinct failure once instead of spamming every hand. Mutex (not
    /// RefCell) because `&DelayScript` is held inside `ActionContext`
    /// across an await, which requires `Sync`; it is never contended.
    last_error: std::sync::Mutex<Option<String>>,
}

/// Install the runaway guard (instruction budget + wall-clock deadline)
/// on `lua`. The hook only ticks pure Lua execution — C functions never
/// fire it — so allocation is bounded separately by the memory limit.
fn install_guard(lua: &Lua, budget: Duration) -> Result<(), mlua::Error> {
    let deadline = Instant::now() + budget;
    let fires = AtomicU64::new(0);
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(HOOK_EVERY_N_INSTRUCTIONS),
        move |_lua, _debug| {
            if fires.fetch_add(1, Ordering::Relaxed) >= MAX_HOOK_FIRES {
                return Err(mlua::Error::RuntimeError(
                    "delay script exceeded instruction budget".into(),
                ));
            }
            if Instant::now() > deadline {
                return Err(mlua::Error::RuntimeError(
                    "delay script exceeded time budget".into(),
                ));
            }
            Ok(VmState::Continue)
        },
    )
}

impl DelayScript {
    /// Compile `source` and resolve the `decide_delay` global.
    pub fn compile(source: &str, chunk_name: &str) -> Result<Self, String> {
        let lua = Lua::new_with(
            StdLib::MATH | StdLib::STRING | StdLib::TABLE,
            LuaOptions::default(),
        )
        .map_err(|e| format!("lua init: {e}"))?;
        let globals = lua.globals();
        for name in SCRUBBED_GLOBALS {
            globals
                .raw_set(*name, Value::Nil)
                .map_err(|e| format!("lua sandbox: {e}"))?;
        }
        // Bytecode has no legitimate producer once `load` is gone, but
        // strip the producer too so none ever exists in this VM.
        if let Ok(string_lib) = globals.get::<Table>("string") {
            let _ = string_lib.raw_set("dump", Value::Nil);
        }
        lua.set_memory_limit(MEMORY_LIMIT_BYTES)
            .map_err(|e| format!("lua memory limit: {e}"))?;
        // The chunk's top level is user code too: run it under the same
        // guard as decide_delay calls, so `while true do end` at file
        // scope rejects the script instead of hanging the autoplay task.
        install_guard(&lua, COMPILE_DEADLINE).map_err(|e| format!("lua hook: {e}"))?;
        let loaded = lua.load(source).set_name(chunk_name).exec();
        lua.remove_hook();
        loaded.map_err(|e| format!("script load: {e}"))?;
        let func: Function = lua
            .globals()
            .get("decide_delay")
            .map_err(|_| "script defines no `decide_delay` function".to_string())?;
        Ok(Self {
            lua,
            func,
            last_error: std::sync::Mutex::new(None),
        })
    }

    /// Run the script for one decision. `None` means "fall back to the
    /// built-in model" (and the cause has been logged once).
    pub fn try_decide(&self, input: &DelayInput) -> Option<DelayDecision> {
        if install_guard(&self.lua, CALL_DEADLINE).is_err() {
            // No hook means no runaway protection — refuse to run the
            // script rather than risk blocking the autoplay loop.
            warn!("delay script: could not install VM hook — using built-in model");
            return None;
        }
        let result = self.call(input);
        self.lua.remove_hook();

        match result {
            Ok(target_ms) => {
                if let Ok(mut g) = self.last_error.lock() {
                    *g = None;
                }
                Some(DelayDecision {
                    total_target_ms: target_ms,
                    allow_bank: false,
                    from_script: true,
                })
            }
            Err(e) => {
                let msg = e.to_string();
                if let Ok(mut g) = self.last_error.lock() {
                    if g.as_deref() != Some(msg.as_str()) {
                        warn!("delay script failed — using built-in model: {msg}");
                        *g = Some(msg);
                    }
                }
                None
            }
        }
    }

    fn call(&self, input: &DelayInput) -> mlua::Result<u32> {
        let ctx = self.build_ctx(input)?;
        let ret: Value = self.func.call(ctx)?;
        let Value::Table(t) = ret else {
            return Err(mlua::Error::RuntimeError(
                "decide_delay must return a table { delay_ms }".into(),
            ));
        };
        let delay_ms: f64 = t.get("delay_ms").map_err(|_| {
            mlua::Error::RuntimeError("decide_delay result is missing numeric `delay_ms`".into())
        })?;
        if !delay_ms.is_finite() || !(0.0..=MAX_SCRIPT_DELAY_MS).contains(&delay_ms) {
            return Err(mlua::Error::RuntimeError(format!(
                "decide_delay returned out-of-range delay_ms {delay_ms}"
            )));
        }
        // Any `allow_bank` a script still returns is ignored: the bank is
        // the script's own business now (it has the budget and the limits).
        Ok(delay_ms as u32)
    }

    /// Build the read-only `ctx` table the script receives.
    fn build_ctx(&self, input: &DelayInput) -> mlua::Result<Table> {
        let lua = &self.lua;
        let ctx = lua.create_table()?;
        ctx.set("action", kind_name(input.kind))?;
        ctx.set("tsumogiri", input.is_tsumogiri)?;
        ctx.set("post_call", input.is_post_call)?;
        ctx.set("first_action", input.first_action_of_kyoku)?;
        ctx.set("dealer_opening", input.opening_animation)?;
        ctx.set("can_riichi", input.can_riichi)?;
        ctx.set("is_kan", input.kind.is_kan())?;
        ctx.set("in_riichi", input.in_riichi)?;
        ctx.set("opponent_riichi", input.opponent_riichi)?;
        if let Some(tc) = input.tile_class {
            ctx.set("tile_class", tc.as_str())?;
        }
        ctx.set("junme", input.junme)?;
        ctx.set("legal_count", input.legal_action_count)?;
        let reading = &input.reading;
        if let Some(loose) = reading.loose {
            ctx.set("loose", loose)?;
        }
        if let Some(loose) = reading.runner_up_loose {
            ctx.set("runner_up_loose", loose)?;
        }
        if let Some(safe) = reading.safe {
            ctx.set("safe", safe)?;
        }
        if let Some(human) = reading.human {
            ctx.set("human_top", human.top)?;
            ctx.set("human_chosen", human.chosen)?;
            if let Some(second) = human.second {
                ctx.set("human_second", second)?;
            }
        }
        if let Some(p) = input.probs {
            ctx.set("top_prob", p.top)?;
            if let Some(second) = p.second {
                ctx.set("second_prob", second)?;
            }
            if let Some(margin) = p.margin() {
                ctx.set("margin", margin)?;
            }
            ctx.set("entropy", p.entropy)?;
        }
        // Whether this decision presses an action button (which renders later
        // than the hand, hence its own floor), and how long the presses
        // themselves take once the delay is over.
        ctx.set("button", input.kind.clicks_action_button())?;
        ctx.set("click_ms", input.click_overhead_ms)?;
        // Every timing setting, as drawn for this decision from its range,
        // and what the time limits are computed from.
        let (c, d) = (input.cfg, input.delay_cfg);
        let settings = lua.create_table()?;
        settings.set("min_delay_ms", d.min_delay_ms.ms())?;
        settings.set("min_button_delay_ms", d.min_button_delay_ms.ms())?;
        settings.set(
            "dealer_opening_ms",
            c.dealer_first_discard_extra_delay_ms.ms(),
        )?;
        settings.set("hover_ms", c.hover_delay_ms.ms())?;
        settings.set("hold_ms", c.click_hold_ms.ms())?;
        settings.set("inter_click_ms", c.inter_click_delay_ms.ms())?;
        settings.set("verify_ms", c.verify_input_ms.ms())?;
        settings.set("pre_click_min_ms", c.pre_click_delay_min_ms)?;
        settings.set("pre_click_max_ms", c.pre_click_delay_max_ms)?;
        settings.set("safety_margin_ms", d.safety_margin_ms)?;
        settings.set("bank_use_fraction", d.bank_use_fraction)?;
        settings.set("bank_max_single_ms", d.bank_max_single_ms)?;
        settings.set("no_budget_cap_ms", d.no_budget_cap_ms)?;
        ctx.set("settings", settings)?;
        if let Some(b) = input.budget {
            let budget = lua.create_table()?;
            budget.set("fixed_ms", b.fixed_ms)?;
            budget.set("add_ms", b.add_ms)?;
            budget.set("elapsed_ms", b.elapsed_ms)?;
            ctx.set("budget", budget)?;
        }
        ctx.set(
            "rng",
            lua.create_function(|_, ()| Ok(rand::random::<f64>()))?,
        )?;
        ctx.set(
            "lognormal",
            lua.create_function(|_, (mu, sigma): (f64, f64)| {
                match rand_distr::LogNormal::new(mu, sigma) {
                    Ok(dist) => {
                        use rand::Rng;
                        Ok(rand::rng().sample::<f64, _>(dist))
                    }
                    Err(_) => Err(mlua::Error::RuntimeError("lognormal: invalid sigma".into())),
                }
            })?,
        )?;
        Ok(ctx)
    }
}

fn kind_name(kind: super::DecisionKind) -> &'static str {
    use super::DecisionKind::*;
    match kind {
        Dahai => "dahai",
        Reach => "reach",
        Chi => "chi",
        Pon => "pon",
        Daiminkan => "daiminkan",
        Ankan => "ankan",
        Kakan => "kakan",
        Hora => "hora",
        Ryukyoku => "ryukyoku",
        Kita => "kita",
        Pass => "none",
    }
}

/// Owns the optional script and its hot-reload state. The autoplay
/// manager calls [`ScriptHost::maybe_reload`] (cheap mtime stat) before
/// planning; the platform planner calls [`ScriptHost::script`].
#[derive(Default)]
pub struct ScriptHost {
    script: Option<DelayScript>,
    /// mtime + size of the last load *attempt* (successful or not) — a
    /// broken file is not recompiled until it changes. Size is part of
    /// the stamp because coarse-timestamp filesystems can give a
    /// truncate-then-write the same mtime as the version we rejected.
    attempted_stamp: Option<(SystemTime, u64)>,
    attempted_path: Option<PathBuf>,
    /// Default-file generation failed (read-only fs) — logged once,
    /// not retried every hand.
    generate_failed: bool,
    /// The existing file was already checked against the superseded
    /// defaults this session — the check reads the whole file, so it
    /// runs once per host, not once per hand.
    superseded_checked: bool,
}

impl ScriptHost {
    /// Write [`DEFAULT_SCRIPT`] to `path` if no file exists there yet,
    /// or if the existing file is an unmodified copy of an earlier
    /// bundled default (see [`SUPERSEDED_DEFAULT_SCRIPTS`]) — without
    /// this, installs that generated `delay.lua` on an older version
    /// would keep its behaviour forever after an update. A deleted file
    /// is regenerated on the next start; a failure (e.g. read-only
    /// install) is logged once and the built-in model runs.
    pub fn ensure_default(&mut self, path: &Path) {
        if self.generate_failed {
            return;
        }
        if path.exists() {
            self.refresh_superseded_default(path);
            return;
        }
        match write_atomic(path, DEFAULT_SCRIPT) {
            Ok(()) => {
                info!("generated default delay script: {}", path.display());
                self.superseded_checked = true; // just wrote the current one
            }
            Err(e) => {
                warn!(
                    "could not generate delay script at {} — using built-in model: {e}",
                    path.display()
                );
                self.generate_failed = true;
            }
        }
    }

    /// Overwrite `path` with [`DEFAULT_SCRIPT`] iff its content is an
    /// unmodified copy of an earlier bundled default. User-edited
    /// scripts never match and are left alone.
    fn refresh_superseded_default(&mut self, path: &Path) {
        if self.superseded_checked {
            return;
        }
        self.superseded_checked = true;
        refresh_outdated_default(path);
    }

    /// (Re)load the script when the file at `path` appeared, changed or
    /// vanished. A missing file is the normal no-script state.
    pub fn maybe_reload(&mut self, path: &Path, enabled: bool) {
        if !enabled {
            self.script = None;
            self.attempted_stamp = None;
            self.attempted_path = None;
            return;
        }
        let stamp = std::fs::metadata(path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        let path_changed = self.attempted_path.as_deref() != Some(path);
        if !path_changed && stamp == self.attempted_stamp {
            return; // nothing new — keep current state (script or None)
        }
        self.attempted_path = Some(path.to_path_buf());
        self.attempted_stamp = stamp;

        let Some(_) = stamp else {
            if self.script.is_some() {
                info!("delay script removed — using built-in model");
            }
            self.script = None;
            return;
        };
        match std::fs::read_to_string(path) {
            Ok(source) => match DelayScript::compile(&source, &path.display().to_string()) {
                Ok(script) => {
                    info!("delay script loaded: {}", path.display());
                    self.script = Some(script);
                }
                Err(e) => {
                    warn!("delay script rejected ({}): {e}", path.display());
                    self.script = None;
                }
            },
            Err(e) => {
                warn!("delay script unreadable ({}): {e}", path.display());
                self.script = None;
            }
        }
    }

    pub fn script(&self) -> Option<&DelayScript> {
        self.script.as_ref()
    }
}

/// Write-then-rename: a crash or full disk mid-write must not leave a
/// truncated delay.lua that the exists/superseded gates in
/// [`ScriptHost::ensure_default`] would then treat as the user's file
/// forever.
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension("lua.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::super::{BudgetSnapshot, DecisionKind, DelayInput};
    use super::*;
    use crate::config::{DelayModelConfig, MajsoulAutoplayConfig};

    fn base_input<'a>(
        cfg: &'a MajsoulAutoplayConfig,
        delay_cfg: &'a DelayModelConfig,
    ) -> DelayInput<'a> {
        DelayInput {
            kind: DecisionKind::Dahai,
            is_tsumogiri: false,
            is_post_call: false,
            first_action_of_kyoku: false,
            opening_animation: false,
            can_riichi: false,
            in_riichi: false,
            opponent_riichi: false,
            tile_class: None,
            junme: 0,
            legal_action_count: 1,
            probs: None,
            reading: Default::default(),
            budget: None,
            click_overhead_ms: 0,
            cfg,
            delay_cfg,
        }
    }

    #[test]
    fn script_result_is_used() {
        let s = DelayScript::compile(
            "function decide_delay(ctx) return { delay_ms = 2300 } end",
            "test",
        )
        .unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let dec = s.try_decide(&base_input(&cfg, &d)).unwrap();
        assert_eq!(dec.total_target_ms, 2300);
        assert!(!dec.allow_bank);
    }

    #[test]
    fn script_sees_ctx_and_helpers() {
        let s = DelayScript::compile(
            r#"
            function decide_delay(ctx)
              assert(ctx.action == "dahai")
              assert(ctx.first_action == false)
              assert(ctx.legal_count == 1)
              assert(ctx.budget.fixed_ms == 5000)
              assert(ctx.budget.add_ms == 20000)
              local r = ctx.rng()
              assert(r >= 0 and r < 1)
              local ln = ctx.lognormal(0.6, 0.5)
              assert(ln > 0)
              assert(ctx.button == false)
              assert(ctx.click_ms == 300)
              local s = ctx.settings
              assert(s.min_delay_ms == 1234 and s.min_button_delay_ms == 2345)
              assert(s.dealer_opening_ms == 2500 and s.hover_ms == 210)
              assert(s.hold_ms == 90 and s.inter_click_ms == 330 and s.verify_ms == 410)
              assert(s.pre_click_min_ms == 1000 and s.pre_click_max_ms == 3000)
              assert(s.safety_margin_ms == 1000 and s.bank_use_fraction == 0.25)
              assert(s.bank_max_single_ms == 5000 and s.no_budget_cap_ms == 15000)
              return { delay_ms = 1000 + ctx.budget.elapsed_ms }
            end
            "#,
            "test",
        )
        .unwrap();
        use crate::config::MsRange;
        // Settings as one decision draws them.
        let cfg = MajsoulAutoplayConfig {
            dealer_first_discard_extra_delay_ms: MsRange::fixed(2500),
            hover_delay_ms: MsRange::fixed(210),
            click_hold_ms: MsRange::fixed(90),
            inter_click_delay_ms: MsRange::fixed(330),
            verify_input_ms: MsRange::fixed(410),
            ..MajsoulAutoplayConfig::default()
        };
        let d = DelayModelConfig {
            min_delay_ms: MsRange::fixed(1234),
            min_button_delay_ms: MsRange::fixed(2345),
            ..DelayModelConfig::default()
        };
        let mut i = base_input(&cfg, &d);
        i.click_overhead_ms = 300;
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 5000,
            add_ms: 20_000,
            elapsed_ms: 250,
        });
        let dec = s.try_decide(&i).unwrap();
        assert_eq!(dec.total_target_ms, 1250);
        assert!(dec.from_script);
    }

    /// The table facts, the second opinion and the entropy reach the script
    /// when read, and are nil when not — "not read" is never "no".
    #[test]
    fn script_sees_the_reading_and_nil_when_unread() {
        let s = DelayScript::compile(
            r#"
            function decide_delay(ctx)
              if ctx.human_chosen == nil then
                assert(ctx.loose == nil and ctx.safe == nil and ctx.runner_up_loose == nil)
                assert(ctx.human_top == nil and ctx.human_second == nil)
                assert(ctx.entropy == nil)
                return { delay_ms = 1111 }
              end
              assert(ctx.loose == true)
              assert(ctx.runner_up_loose == false)
              assert(ctx.safe == false)
              assert(math.abs(ctx.human_top - 0.6) < 1e-9)
              assert(math.abs(ctx.human_second - 0.3) < 1e-9)
              assert(math.abs(ctx.human_chosen - 0.3) < 1e-9)
              assert(math.abs(ctx.entropy - 0.5) < 1e-9)
              return { delay_ms = 2222 }
            end
            "#,
            "test",
        )
        .unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let mut i = base_input(&cfg, &d);
        assert_eq!(s.try_decide(&i).unwrap().total_target_ms, 1111);
        i.reading = crate::autoplay::delay::Reading {
            loose: Some(true),
            runner_up_loose: Some(false),
            safe: Some(false),
            human: Some(crate::autoplay::delay::HumanView {
                top: 0.6,
                second: Some(0.3),
                chosen: 0.3,
            }),
        };
        i.probs = Some(crate::autoplay::delay::DecisionProbs {
            top: 0.7,
            second: Some(0.2),
            entropy: 0.5,
        });
        assert_eq!(s.try_decide(&i).unwrap().total_target_ms, 2222);
    }

    #[test]
    fn syntax_error_is_rejected_at_compile() {
        assert!(DelayScript::compile("function decide_delay(", "test").is_err());
        assert!(
            DelayScript::compile("x = 1", "test").is_err(),
            "no function"
        );
    }

    #[test]
    fn runtime_error_falls_back() {
        let s =
            DelayScript::compile("function decide_delay(ctx) error('boom') end", "test").unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        assert!(s.try_decide(&base_input(&cfg, &d)).is_none());
    }

    #[test]
    fn wrong_return_shape_falls_back() {
        for src in [
            "function decide_delay(ctx) return 42 end",
            "function decide_delay(ctx) return { } end",
            "function decide_delay(ctx) return { delay_ms = 'soon' } end",
            "function decide_delay(ctx) return { delay_ms = -5 } end",
            "function decide_delay(ctx) return { delay_ms = 1e12 } end",
            "function decide_delay(ctx) return { delay_ms = 0/0 } end",
        ] {
            let s = DelayScript::compile(src, "test").unwrap();
            let cfg = MajsoulAutoplayConfig::default();
            let d = DelayModelConfig::default();
            assert!(
                s.try_decide(&base_input(&cfg, &d)).is_none(),
                "must fall back for: {src}"
            );
        }
    }

    #[test]
    fn infinite_loop_is_aborted() {
        let s = DelayScript::compile("function decide_delay(ctx) while true do end end", "test")
            .unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let start = Instant::now();
        assert!(s.try_decide(&base_input(&cfg, &d)).is_none());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "hook must abort the loop quickly"
        );
    }

    /// Regression: a `while true do end` at file scope used to hang
    /// `compile` forever (the guard hook was only installed for
    /// `decide_delay` calls, not for the chunk's top level).
    #[test]
    fn top_level_infinite_loop_is_rejected_at_compile() {
        let start = Instant::now();
        assert!(DelayScript::compile("while true do end", "test").is_err());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "compile must abort a top-level loop quickly"
        );
    }

    /// Regression: `pcall` used to be reachable and could catch the
    /// guard's abort error, making the instruction budget escapable.
    /// With `pcall` scrubbed, the shielded loop dies on the nil call.
    #[test]
    fn pcall_cannot_shield_a_runaway_loop() {
        let s = DelayScript::compile(
            r#"
            function decide_delay(ctx)
              while true do pcall(function() while true do end end) end
            end
            "#,
            "test",
        )
        .unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let start = Instant::now();
        assert!(s.try_decide(&base_input(&cfg, &d)).is_none());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "a pcall-shielded loop must still abort quickly"
        );
    }

    /// Regression: `load` accepted unvalidated precompiled bytecode
    /// (native-code execution) and `dofile()` blocked reading stdin.
    /// All loaders and `string.dump` must be gone; io/os/debug stay out.
    #[test]
    fn sandbox_scrubs_loaders_and_dump() {
        let s = DelayScript::compile(
            r#"
            assert(load == nil, "load must be nil")
            assert(loadstring == nil, "loadstring must be nil")
            assert(dofile == nil, "dofile must be nil")
            assert(loadfile == nil, "loadfile must be nil")
            assert(pcall == nil, "pcall must be nil")
            assert(xpcall == nil, "xpcall must be nil")
            assert(string.dump == nil, "string.dump must be nil")
            assert(os == nil and io == nil and debug == nil, "no io/os/debug")
            function decide_delay(ctx) return { delay_ms = 2000 } end
            "#,
            "test",
        )
        .unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        assert!(s.try_decide(&base_input(&cfg, &d)).is_some());
    }

    /// Regression: with no memory limit, a huge `string.rep` ballooned
    /// RSS unchecked (C functions never tick the guard hook) and an
    /// allocation failure aborted the process. With the limit set it is
    /// a normal Lua error → fallback.
    #[test]
    fn allocation_bomb_falls_back() {
        let s = DelayScript::compile(
            r#"
            function decide_delay(ctx)
              local x = string.rep("x", 1e9)
              return { delay_ms = #x }
            end
            "#,
            "test",
        )
        .unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let start = Instant::now();
        assert!(s.try_decide(&base_input(&cfg, &d)).is_none());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "allocation failure must be an immediate Lua error"
        );
    }

    /// Regression: `fixed_ms < 1000` used to drive the default script's
    /// `free_s` negative, producing a negative `delay_ms` that the host
    /// rejects — silently disabling the script for every decision in
    /// such a room.
    #[test]
    fn bundled_default_survives_tiny_fixed_budget() {
        let s = DelayScript::compile(DEFAULT_SCRIPT, "default").unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let mut i = base_input(&cfg, &d);
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 500,
            add_ms: 1000,
            elapsed_ms: 0,
        });
        for _ in 0..200 {
            assert!(
                s.try_decide(&i).is_some(),
                "a sub-second fixed_ms must not reject the default script"
            );
        }
    }

    /// The script owns the whole computation: the host adds no floor and
    /// no cap to its answer (an `allow_bank` it still returns is ignored).
    #[test]
    fn the_host_adds_nothing_to_a_scripts_answer() {
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let run = |src: &str, i: &DelayInput| {
            DelayScript::compile(src, "test")
                .unwrap()
                .try_decide(i)
                .unwrap()
                .total_target_ms
        };
        let zero = "function decide_delay(ctx) return { delay_ms = 0, allow_bank = 0 } end";
        let mut i = base_input(&cfg, &d);
        i.opening_animation = true;
        assert_eq!(run(zero, &i), 0, "no dealing-animation floor added");
        let mut i = base_input(&cfg, &d);
        i.kind = DecisionKind::Pon;
        assert_eq!(run(zero, &i), 0, "no button floor added");

        let huge = "function decide_delay(ctx) return { delay_ms = 500000 } end";
        let mut i = base_input(&cfg, &d);
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 5000,
            add_ms: 0,
            elapsed_ms: 0,
        });
        assert_eq!(run(huge, &i), 500_000, "no budget cap added");
    }

    /// What the host no longer adds, the bundled default does: never
    /// before the drawn floor (plus the presses' own time), never past the
    /// server's window (less the safety margin and the presses), the floor
    /// winning where the two cross.
    #[test]
    fn the_bundled_default_holds_the_floor_and_the_limit() {
        use crate::config::MsRange;
        let s = DelayScript::compile(DEFAULT_SCRIPT, "delay_default.lua").unwrap();
        let cfg = MajsoulAutoplayConfig {
            dealer_first_discard_extra_delay_ms: MsRange::fixed(2600),
            ..MajsoulAutoplayConfig::default()
        };
        let d = DelayModelConfig {
            min_delay_ms: MsRange::fixed(1300),
            min_button_delay_ms: MsRange::fixed(2100),
            ..DelayModelConfig::default()
        };
        let click = 300;
        for (kind, opening, floor) in [
            (DecisionKind::Dahai, false, 1300),
            (DecisionKind::Pon, false, 2100),
            (DecisionKind::Dahai, true, 2600),
        ] {
            for budget in [
                None,
                Some(BudgetSnapshot {
                    fixed_ms: 5000,
                    add_ms: 0,
                    elapsed_ms: 0,
                }),
                Some(BudgetSnapshot {
                    fixed_ms: 3000,
                    add_ms: 0,
                    elapsed_ms: 0,
                }),
            ] {
                let mut i = base_input(&cfg, &d);
                i.kind = kind;
                i.opening_animation = opening;
                i.click_overhead_ms = click;
                i.budget = budget;
                let limit = match budget {
                    Some(b) => b.fixed_ms - d.safety_margin_ms - click,
                    None => d.no_budget_cap_ms,
                };
                for _ in 0..200 {
                    let t = s.try_decide(&i).unwrap().total_target_ms;
                    assert!(t >= floor + click, "{kind:?} {budget:?}: {t} < floor");
                    assert!(
                        t <= limit.max(floor + click),
                        "{kind:?} {budget:?}: {t} > limit"
                    );
                }
            }
        }
    }

    /// The bundled default (`DEFAULT_SCRIPT`, generated as `delay.lua`)
    /// must compile and produce human-plausible values.
    #[test]
    fn bundled_default_script_works() {
        let s = DelayScript::compile(DEFAULT_SCRIPT, "delay_default.lua").unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();

        // Plain tedashi: calibrated median ~2.4s. The script rng is not
        // seedable from here, so assert on a batch median with slack.
        let mut samples: Vec<u32> = (0..300)
            .map(|_| s.try_decide(&base_input(&cfg, &d)).unwrap().total_target_ms)
            .collect();
        samples.sort_unstable();
        let med = samples[samples.len() / 2];
        assert!(
            (1500..=4000).contains(&med),
            "default-script tedashi median {med} implausible"
        );
        // Floors/caps still bind: nothing below min_delay_ms, nothing
        // above the no-budget static cap.
        assert!(*samples.first().unwrap() >= d.min_delay_ms.ms());
        assert!(*samples.last().unwrap() <= d.no_budget_cap_ms);

        // In riichi: measured Throne players still glance (~1.4s median),
        // never below the UI-readiness floor.
        let mut riichi_samples: Vec<u32> = (0..300)
            .map(|_| {
                let mut i = base_input(&cfg, &d);
                i.in_riichi = true;
                i.kind = DecisionKind::Pass;
                s.try_decide(&i).unwrap().total_target_ms
            })
            .collect();
        riichi_samples.sort_unstable();
        assert!(*riichi_samples.first().unwrap() >= d.min_delay_ms.ms());
        let riichi_med = riichi_samples[riichi_samples.len() / 2];
        assert!(
            (1100..=1800).contains(&riichi_med),
            "in-riichi median {riichi_med} off calibration (~1.4s)"
        );

        // The default model ignores the bot's probabilities: a near-tie
        // must not shift the batch median (each batch re-rolls the rng,
        // so allow generous sampling slack).
        let mut tie_samples: Vec<u32> = (0..300)
            .map(|_| {
                let mut i = base_input(&cfg, &d);
                i.probs = Some(crate::autoplay::delay::DecisionProbs {
                    top: 0.40,
                    second: Some(0.399),
                    entropy: 0.0,
                });
                s.try_decide(&i).unwrap().total_target_ms
            })
            .collect();
        tie_samples.sort_unstable();
        let tie_med = tie_samples[tie_samples.len() / 2] as f64;
        let ratio = tie_med / med as f64;
        assert!(
            (0.8..=1.25).contains(&ratio),
            "near-tie median {tie_med} vs base {med} — probs must not shift the model"
        );

        // Claim windows are the fast reaction bucket: batch median well
        // under the tedashi one.
        let mut claims: Vec<u32> = (0..300)
            .map(|_| {
                let mut i = base_input(&cfg, &d);
                i.kind = DecisionKind::Pass;
                s.try_decide(&i).unwrap().total_target_ms
            })
            .collect();
        claims.sort_unstable();
        assert!(claims[claims.len() / 2] < med);
    }

    /// Regression: bots that report flat policy probabilities (top well
    /// under 0.60 on nearly every decision — e.g. the native bot's HUD
    /// probs) used to trip a `top_prob < 0.60 → routine × 0.3` rule in
    /// the bundled script, all but eliminating the fast "routine flick"
    /// cluster and inflating every discard. Flat probs must keep the
    /// same routine odds as no probs at all.
    #[test]
    fn bundled_default_flat_probs_keep_routine_fraction() {
        let s = DelayScript::compile(DEFAULT_SCRIPT, "delay_default.lua").unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();

        // Tsumogiri honor: the bucket with the largest routine weight
        // (0.15). With the collapse rule that dropped to 0.045, pushing
        // the fast (≤1.2s) fraction from ~30% down to ~24%.
        // 10k draws: the bundled script lands near 0.29 here, and at 3k the
        // sampling spread alone came within two standard errors of 0.27.
        let fast_fraction = |probs: Option<crate::autoplay::delay::DecisionProbs>| {
            let n = 10_000;
            let fast = (0..n)
                .filter(|_| {
                    let mut i = base_input(&cfg, &d);
                    i.is_tsumogiri = true;
                    i.tile_class = Some(crate::autoplay::delay::TileClass::Honor);
                    i.probs = probs;
                    s.try_decide(&i).unwrap().total_target_ms <= 1200
                })
                .count();
            fast as f64 / n as f64
        };

        // Flat probs with a tiny margin — the shape that used to trip
        // both removed rules (routine collapse and near-tie extra time).
        let flat = fast_fraction(Some(crate::autoplay::delay::DecisionProbs {
            top: 0.11,
            second: Some(0.105),
            entropy: 0.0,
        }));
        assert!(
            flat > 0.27,
            "flat-prob fast fraction {flat:.3} — routine collapse is back"
        );
    }

    /// Median total target of the bundled script over `n` draws of `tweak`ed
    /// input, with the UI floors off so only the script's own shape shows.
    fn bundled_median(tweak: impl Fn(&mut DelayInput)) -> u32 {
        let s = DelayScript::compile(DEFAULT_SCRIPT, "delay_default.lua").unwrap();
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig {
            min_delay_ms: crate::config::MsRange::fixed(0),
            min_button_delay_ms: crate::config::MsRange::fixed(0),
            ..DelayModelConfig::default()
        };
        let mut samples: Vec<u32> = (0..2000)
            .map(|_| {
                let mut i = base_input(&cfg, &d);
                tweak(&mut i);
                s.try_decide(&i).unwrap().total_target_ms
            })
            .collect();
        samples.sort_unstable();
        samples[samples.len() / 2]
    }

    fn human(chosen: f64) -> crate::autoplay::delay::Reading {
        crate::autoplay::delay::Reading {
            human: Some(crate::autoplay::delay::HumanView {
                top: chosen.max(0.5),
                second: None,
                chosen,
            }),
            ..Default::default()
        }
    }

    /// The bundled policy thinks longer over a move few people would make
    /// than over one nearly everyone makes.
    #[test]
    fn bundled_default_thinks_longer_where_people_split() {
        let obvious = bundled_median(|i| i.reading = human(0.97));
        let rare = bundled_median(|i| i.reading = human(0.10));
        assert!(
            rare as f64 > obvious as f64 * 1.4,
            "rare move median {rare} vs obvious {obvious}"
        );
    }

    /// Clearing loose tiles with nobody in riichi is not a question.
    #[test]
    fn bundled_default_clears_loose_tiles_quickly() {
        let contested = bundled_median(|i| {
            i.reading = crate::autoplay::delay::Reading {
                loose: Some(false),
                ..Default::default()
            }
        });
        let clearing = bundled_median(|i| {
            i.reading = crate::autoplay::delay::Reading {
                loose: Some(true),
                runner_up_loose: Some(true),
                ..Default::default()
            }
        });
        assert!(
            (clearing as f64) < contested as f64 * 0.85,
            "clearing median {clearing} vs contested {contested}"
        );
    }

    /// Against a riichi a live tile takes longer than a safe one.
    #[test]
    fn bundled_default_takes_longer_over_a_live_tile() {
        let reading = |safe| crate::autoplay::delay::Reading {
            loose: Some(false),
            safe: Some(safe),
            ..Default::default()
        };
        let safe = bundled_median(|i| {
            i.opponent_riichi = true;
            i.reading = reading(true);
        });
        let live = bundled_median(|i| {
            i.opponent_riichi = true;
            i.reading = reading(false);
        });
        assert!(
            live as f64 > safe as f64 * 1.1,
            "live median {live} vs safe {safe}"
        );
    }

    /// The dealer's opening thinks after the deal is laid out, not under it.
    #[test]
    fn bundled_default_thinks_after_the_deal() {
        let opening = bundled_median(|i| {
            i.opening_animation = true;
            i.first_action_of_kyoku = true;
        });
        assert!(opening >= 4000, "opening median {opening}");
    }

    /// `ensure_default` generates the bundled script once and never
    /// overwrites user edits.
    #[test]
    fn ensure_default_generates_once_and_preserves_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        let mut host = ScriptHost::default();

        host.ensure_default(&path);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            DEFAULT_SCRIPT,
            "missing file must be created from the bundled default"
        );

        std::fs::write(
            &path,
            "-- user edit\nfunction decide_delay(c) return { delay_ms = 1 } end",
        )
        .unwrap();
        host.ensure_default(&path);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("-- user edit"),
            "an existing file must never be overwritten"
        );

        // Same for a fresh host (no superseded_checked short-circuit):
        // a user script simply doesn't match any old default.
        let mut fresh = ScriptHost::default();
        fresh.ensure_default(&path);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("-- user edit"),
            "a user-edited file must survive the superseded-default check"
        );
    }

    /// Regression (issue #231): a delay.lua auto-generated by an older
    /// version must be replaced by the current bundled default on the
    /// next run — updates used to leave the stale script in place until
    /// the user deleted it by hand.
    #[test]
    fn outdated_bundled_default_is_refreshed() {
        for old in SUPERSEDED_DEFAULT_SCRIPTS {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("delay.lua");
            std::fs::write(&path, old).unwrap();

            let mut host = ScriptHost::default();
            host.ensure_default(&path);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                DEFAULT_SCRIPT,
                "an unmodified older default must be updated in place"
            );
        }
    }

    /// A pure CRLF re-encoding of an old default (Windows editors,
    /// autocrlf checkouts) is still not a user edit.
    #[test]
    fn outdated_default_with_crlf_is_refreshed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        let old = SUPERSEDED_DEFAULT_SCRIPTS.last().unwrap();
        // Normalize first: on an autocrlf checkout the compiled-in old
        // default already carries CRLF, and a blind `\n → \r\n` pass would
        // double the carriage returns into something no editor produces.
        let crlf = old.replace("\r\n", "\n").replace('\n', "\r\n");
        std::fs::write(&path, crlf).unwrap();

        let mut host = ScriptHost::default();
        host.ensure_default(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_SCRIPT);
    }

    #[test]
    fn a_script_is_told_apart_from_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        assert_eq!(script_state(&path), ScriptState::Missing);
        std::fs::write(&path, DEFAULT_SCRIPT).unwrap();
        assert_eq!(script_state(&path), ScriptState::Current);
        std::fs::write(&path, SUPERSEDED_DEFAULT_SCRIPTS.last().unwrap()).unwrap();
        assert_eq!(script_state(&path), ScriptState::Outdated);
        std::fs::write(
            &path,
            "-- mine\nfunction decide_delay(c) return { delay_ms = 1 } end",
        )
        .unwrap();
        assert_eq!(script_state(&path), ScriptState::Modified);
        assert_eq!(default_version().len(), 12);
    }

    /// Replacing keeps the user's script beside the new default.
    #[test]
    fn replacing_backs_the_old_script_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        let mine = "-- mine\nfunction decide_delay(c) return { delay_ms = 1 } end";
        std::fs::write(&path, mine).unwrap();
        let backup = replace_with_default(&path).unwrap().expect("a backup");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), mine);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_SCRIPT);
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("delay.lua."));

        let fresh = dir.path().join("none.lua");
        assert!(
            replace_with_default(&fresh).unwrap().is_none(),
            "nothing to back up"
        );
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), DEFAULT_SCRIPT);
    }

    /// The startup check updates an untouched old default without a host,
    /// and leaves a user edit and a missing file alone.
    #[test]
    fn startup_refresh_updates_only_an_untouched_old_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        assert!(!refresh_outdated_default(&path), "nothing to update");
        assert!(!path.exists(), "a missing file is not generated here");

        std::fs::write(&path, SUPERSEDED_DEFAULT_SCRIPTS.last().unwrap()).unwrap();
        assert!(refresh_outdated_default(&path));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_SCRIPT);
        assert!(!refresh_outdated_default(&path), "already current");

        let edited = format!("{}\n-- tweak\n", SUPERSEDED_DEFAULT_SCRIPTS.last().unwrap());
        std::fs::write(&path, &edited).unwrap();
        assert!(!refresh_outdated_default(&path));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    }

    /// The check runs once per host: rewriting the old script back mid-
    /// session is treated as a (bizarre) user edit until the next start.
    #[test]
    fn superseded_check_runs_once_per_host() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        let old = SUPERSEDED_DEFAULT_SCRIPTS.last().unwrap();
        std::fs::write(&path, old).unwrap();

        let mut host = ScriptHost::default();
        host.ensure_default(&path);
        std::fs::write(&path, old).unwrap();
        host.ensure_default(&path);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().as_str(),
            *old,
            "the whole-file comparison must not repeat every hand"
        );
    }

    /// Maintenance guard: the superseded list must contain real *old*
    /// versions — each one detected, none of them the current default
    /// (copying the wrong file into the superseded directory would
    /// otherwise go unnoticed).
    #[test]
    fn superseded_list_is_distinct_from_current_default() {
        for old in SUPERSEDED_DEFAULT_SCRIPTS {
            assert!(is_superseded_default(old));
            assert_ne!(
                old.replace("\r\n", "\n"),
                DEFAULT_SCRIPT.replace("\r\n", "\n"),
                "the current default must never be listed as superseded"
            );
        }
        assert!(!is_superseded_default(DEFAULT_SCRIPT));
    }

    #[test]
    fn host_hot_reloads_and_tolerates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delay.lua");
        let mut host = ScriptHost::default();

        // Missing file: normal no-script state.
        host.maybe_reload(&path, true);
        assert!(host.script().is_none());

        std::fs::write(
            &path,
            "function decide_delay(ctx) return { delay_ms = 1111 } end",
        )
        .unwrap();
        host.maybe_reload(&path, true);
        assert!(host.script().is_some());

        // Rewrite with a different value (bump mtime explicitly — some
        // filesystems have coarse timestamps).
        std::fs::write(
            &path,
            "function decide_delay(ctx) return { delay_ms = 2222 } end",
        )
        .unwrap();
        let bumped = SystemTime::now() + Duration::from_secs(2);
        // Windows needs a handle with write access for SetFileTime; a
        // read-only `File::open` yields ERROR_ACCESS_DENIED.
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(bumped).unwrap();
        host.maybe_reload(&path, true);
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        let dec = host
            .script()
            .unwrap()
            .try_decide(&base_input(&cfg, &d))
            .unwrap();
        assert_eq!(dec.total_target_ms, 2222, "reload must pick up the edit");

        // Disabled: script dropped.
        host.maybe_reload(&path, false);
        assert!(host.script().is_none());
    }
}
