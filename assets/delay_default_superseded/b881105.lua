-- Akagi autoplay delay model.
--
-- Akagi generated this file next to your config.toml. Edit it freely —
-- it hot-reloads every time you save. Delete it to get a fresh copy of
-- the default on the next start. If the script ever errors, Akagi keeps
-- playing with its built-in model and logs the reason once.
--
-- decide_delay(ctx) returns { delay_ms = <number>, allow_bank = <bool> }.
--
-- delay_ms is the target TOTAL thinking time for the decision window —
-- the time the SERVER observes between offering the decision and
-- receiving the action. It is NOT a sleep length: Akagi subtracts
-- whatever networking and bot inference already consumed.
--
-- Akagi always enforces on top of whatever this script returns:
--   * a minimum delay (config `min_delay_ms`; the higher
--     `min_button_delay_ms` for chi/pon/kan/ron/skip/riichi buttons,
--     which render later than hand tiles) — clicks issued before the
--     UI exists are silently lost
--   * the dealing-animation wait on the dealer's opening discard
--   * the server time budget — autoplay can never run into an
--     auto-discard; the extra time bank is only spent when this script
--     returns allow_bank = true
--
-- Context passed to decide_delay:
--   ctx.action          "dahai" | "reach" | "chi" | "pon" | "daiminkan"
--                       | "ankan" | "kakan" | "hora" | "ryukyoku" | "kita"
--                       | "none"  (declining a call window)
--   ctx.tsumogiri       the discard is the just-drawn tile
--   ctx.post_call       discard following our own chi/pon
--   ctx.tile_class      "honor" | "terminal" | "middle" for a discard
--   ctx.first_action    first action of the kyoku
--   ctx.dealer_opening  dealer's 14-tile opening discard
--   ctx.can_riichi      riichi is declarable this turn
--   ctx.is_kan          the action is a kan declaration
--   ctx.in_riichi       we are in accepted riichi
--   ctx.opponent_riichi an opponent has declared riichi
--   ctx.junme           our discard number this kyoku (1 = first)
--   ctx.legal_count     number of legal actions
--   ctx.top_prob        bot's top candidate probability (0..1), or nil
--   ctx.second_prob     second candidate probability, or nil
--   ctx.margin          top_prob - second_prob, or nil
--                       (the three above are unused by this default
--                       model — see the note before routine_probability)
--   ctx.budget          { fixed_ms, add_ms, elapsed_ms } or nil
--   ctx.rng()           uniform random in [0, 1)
--   ctx.lognormal(mu, sigma)  log-normal sample in SECONDS
--
-- Every number below is calibrated against ranked Throne-room game
-- records (30 games, ~21,500 decisions, think times on the server
-- clock). Real discards are a MIXTURE: a "routine" cluster (~1s — the
-- tile was already decided) and a genuine think whose length depends on
-- what is being discarded, how deep the hand is, and whether someone
-- has declared riichi.

-- Probability that a discard is routine (no real thought). These are
-- MIXTURE WEIGHTS, not the measured fast-fractions directly: the
-- routine cluster only puts ~84% of its own mass under 1.2s and the
-- genuine-think cluster contributes sub-1.2s mass too, so each weight
-- w solves  w*0.844 + (1-w)*P_think(<1.2s) = measured fast-fraction
-- (tedashi h/t/m: 21%/10%/4%; tsumogiri h/t/m: 31%/26%/20%). Tedashi
-- terminal/middle round to zero — their think cluster alone already
-- covers the measured fast mass.
local ROUTINE = {
  tedashi   = { honor = 0.12, terminal = 0.00, middle = 0.00 },
  tsumogiri = { honor = 0.15, terminal = 0.10, middle = 0.04 },
}

-- Log-normal { mu, sigma } (ln-seconds) of the *genuine-think* cluster.
local THINK = {
  tedashi   = { honor = { 0.77, 0.50 }, terminal = { 0.83, 0.50 },
                middle = { 1.01, 0.55 }, default = { 0.87, 0.55 } },
  tsumogiri = { honor = { 0.54, 0.45 }, terminal = { 0.57, 0.45 },
                middle = { 0.63, 0.48 }, default = { 0.57, 0.47 } },
}

-- Non-discard decisions (single log-normal fits them well).
local OTHER = {
  reach           = { 1.10, 0.55 }, -- riichi declaration   (median ~3.0s)
  claim           = { 0.26, 0.57 }, -- call window incl pass (median ~1.3s)
  post_call_dahai = { 0.52, 0.42 }, -- discard after own call (median ~1.7s)
  hora            = { 0.15, 0.50 }, -- ron / tsumo           (median ~1.2s)
  in_riichi       = { 0.35, 0.30 }, -- skip while in riichi  (median ~1.4s)
}

-- NOTE: this model deliberately ignores the bot's reported probabilities
-- (ctx.top_prob / ctx.second_prob / ctx.margin). Measured bot policies
-- can be nearly flat (top ~0.11 on most decisions), which turned every
-- confidence-based rule — "obvious tile" speed-ups and "contested call"
-- slow-downs alike — into a permanent bias instead of a signal. The
-- fields remain available for custom scripts tuned to a specific bot.
local function routine_probability(ctx, giri)
  local p = (ROUTINE[giri] or {})[ctx.tile_class or "middle"] or 0.04
  -- A riichi on the table means every discard gets a safety read.
  if ctx.opponent_riichi then p = p * 0.4 end
  if p > 0.6 then p = 0.6 end
  return p
end

local function discard_think(ctx)
  local giri = ctx.tsumogiri and "tsumogiri" or "tedashi"
  if ctx.rng() < routine_probability(ctx, giri) then
    -- Routine flick: tight cluster around one second.
    return ctx.lognormal(0.0, 0.18)
  end
  local params = THINK[giri][ctx.tile_class or "default"]
                 or THINK[giri].default
  local think = ctx.lognormal(params[1], params[2])
  -- Deeper hands are read longer — but only tedashi shows this
  -- (measured tsumogiri medians are junme-flat).
  if giri == "tedashi" and ctx.junme ~= nil and ctx.junme > 0 then
    think = think * (1.0 + 0.012 * math.min(ctx.junme, 15))
  end
  -- Defence reads against a live riichi: +16% tedashi, +25% tsumogiri,
  -- with a fatter tail.
  if ctx.opponent_riichi then
    think = think * (giri == "tedashi" and 1.16 or 1.25)
    if ctx.rng() < 0.06 then think = think + ctx.lognormal(0.9, 0.5) end
  end
  return think
end

function decide_delay(ctx)
  local think
  local allow_bank = false

  if ctx.in_riichi then
    -- Only claim windows reach this point in riichi (Majsoul auto-
    -- discards our draws). Measured: still ~1.4s — humans do glance.
    think = ctx.lognormal(OTHER.in_riichi[1], OTHER.in_riichi[2])
    return { delay_ms = think * 1000 }
  end

  if ctx.action == "dahai" then
    if ctx.post_call then
      think = ctx.lognormal(OTHER.post_call_dahai[1], OTHER.post_call_dahai[2])
    else
      think = discard_think(ctx)
    end
  elseif ctx.action == "reach" then
    think = ctx.lognormal(OTHER.reach[1], OTHER.reach[2])
  elseif ctx.action == "hora" then
    think = ctx.lognormal(OTHER.hora[1], OTHER.hora[2])
  elseif ctx.action == "ankan" or ctx.action == "kakan" then
    -- Own-turn kan declaration thinks like a middle-tile tedashi.
    think = ctx.lognormal(THINK.tedashi.middle[1], THINK.tedashi.middle[2])
  else
    -- Claim-window responses: chi/pon/daiminkan/skip/kyuushu.
    think = ctx.lognormal(OTHER.claim[1], OTHER.claim[2])
  end

  -- First action of a kyoku: the fresh hand gets surveyed.
  if ctx.first_action then
    think = think + 0.5 + 0.8 * ctx.rng()
  end

  -- A declarable riichi makes this discard a real decision even when
  -- the bot ends up not declaring.
  if ctx.can_riichi and ctx.action == "dahai" then
    think = think + 0.4 + 0.8 * ctx.rng()
  end

  -- Occasional genuine tank — recounting discards, weighing a fold.
  if ctx.rng() < 0.02 then
    think = think + 2.0 + ctx.lognormal(0.8, 0.6)
    allow_bank = true
  end

  -- Budget shaping. The server grants fixed_ms per decision plus a
  -- bank (add_ms) that REFILLS EVERY KYOKU — measured Throne players
  -- dip into it on ~7% of draws, so exceeding the free window is
  -- normal play, not an emergency. But when the bank is nearly dry,
  -- humans wrap up instead of risking the auto-discard timer.
  if ctx.budget ~= nil then
    -- Floored at 0: a sub-second fixed_ms must degrade to "answer at
    -- the enforced minimum", not to a negative delay_ms that Akagi
    -- would reject (which would silently disable this script for the
    -- whole room).
    local free_s = math.max(ctx.budget.fixed_ms / 1000 - 1.0, 0)
    local bank_s = ctx.budget.add_ms / 1000
    if think > free_s then
      if bank_s >= 3.0 then
        allow_bank = true -- natural dip; Akagi bounds how much is spent
      else
        think = free_s    -- nearly broke: finish the thought quickly
      end
    end
  end

  return { delay_ms = think * 1000, allow_bank = allow_bank }
end
