use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutoplayConfig {
    /// Master switch. When `false`, no `AutoplayManager` is spawned and bot
    /// responses are not converted into UI clicks.
    pub enabled: bool,
    /// Per-platform autoplay knobs. Only the matching platform's section is
    /// consulted at runtime; the others sit dormant.
    pub majsoul: MajsoulAutoplayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MajsoulAutoplayConfig {
    /// Lower bound of the random pre-click delay (ms). The reference
    /// Akagi autoplay used `random.uniform(1.0, 3.0)` seconds; the same
    /// distribution is replicated here as `[1000, 3000]` ms by default.
    pub pre_click_delay_min_ms: u32,
    /// Upper bound of the random pre-click delay (ms).
    pub pre_click_delay_max_ms: u32,
    /// Inter-click delay between staged clicks within one action (e.g.
    /// reach button → riichi tile, or chi button → candidate select).
    pub inter_click_delay_ms: u32,
    /// How long to hover the mouse over a target before pressing.
    /// Empirically Laya's input system samples hover state before a
    /// mousedown registers a hit on the tile sprite — clicks issued
    /// without a hover delay (or shorter than ~100ms) get dropped on
    /// the floor. Default 150ms; do not lower below 100ms.
    pub hover_delay_ms: u32,
    /// How long to hold the mouse button down between mousePressed and
    /// mouseReleased. Non-zero so the engine doesn't collapse the pair
    /// into a single frame.
    pub click_hold_ms: u32,
    /// Extra delay tacked onto the dealer's first discard. Mahjong Soul
    /// plays a hand-sort animation when the dealer receives all 14 tiles
    /// at once; clicks issued during the animation are dropped. ~2s
    /// covers the animation across normal device speeds. Set to 0 to
    /// opt out (e.g. on a fast box where the animation finishes inside
    /// the regular pre-click delay anyway).
    pub dealer_first_discard_extra_delay_ms: u32,
}

impl Default for MajsoulAutoplayConfig {
    fn default() -> Self {
        Self {
            pre_click_delay_min_ms: 1000,
            pre_click_delay_max_ms: 3000,
            inter_click_delay_ms: 300,
            hover_delay_ms: 150,
            click_hold_ms: 50,
            dealer_first_discard_extra_delay_ms: 2000,
        }
    }
}
