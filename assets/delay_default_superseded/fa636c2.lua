-- Akagi autoplay delay model.
--
-- Akagi generated this file next to your config.toml. Edit it freely —
-- it hot-reloads every time you save. Delete it to get a fresh copy of
-- the default on the next start. If the script ever errors, Akagi keeps
-- playing with its built-in model and logs the reason once.
--
-- Adapted from Washizu's pacing policy
-- (https://github.com/nanomena/Washizu, Apache-2.0).
--
-- HOW LONG PEOPLE TAKE comes from people: the log-normal think times below
-- were fitted to ranked game records by kind of decision (30 games, about
-- 21,500 decisions, think times on the server clock).
--
-- WHICH DECISIONS GET THE LONG ONES is the question this file answers. A
-- person thinks where the position is a question for a person: pushing or
-- folding, finding a tile to fold with, choosing between shapes. Two
-- things are read for that, each for what it is good at.
--
-- 1. What people would do (ctx.human_*). The built-in bot imitates people,
--    so its policy is roughly the share of players making each move. The
--    smaller the share that makes the move being made, the less obvious the
--    move, and the longer the think. Those cases are exactly where players
--    split. The bot that plays may be another one; this only reads it.
--
-- 2. The table (ctx.loose, ctx.runner_up_loose, ctx.safe), for what the
--    first reading cannot say. Loose tiles — nothing in the hand connects
--    to them — go out in whatever order: players split over which, and
--    nobody thinks about it. This is about the tile's place in the hand,
--    not its kind: a loose 8 is cleared like a loose North, and a 9 in a run
--    is not loose. And a tile that is not safe against a riichi takes
--    longer than one that is.
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
--   ctx.dealer_opening  dealer's 14-tile opening discard (Mahjong Soul only:
--                       the other platforms wait for the client instead)
--   ctx.can_riichi      riichi is declarable this turn
--   ctx.is_kan          the action is a kan declaration
--   ctx.in_riichi       we are in accepted riichi
--   ctx.opponent_riichi an opponent has declared riichi
--   ctx.junme           our discard number this kyoku (1 = first)
--   ctx.legal_count     number of legal actions
--   ctx.top_prob        the playing bot's top candidate probability (0..1),
--                       or nil
--   ctx.second_prob     second candidate probability, or nil
--   ctx.margin          top_prob - second_prob, or nil
--   ctx.entropy         how spread the candidates were, in nats, or nil
--   ctx.loose, ctx.runner_up_loose, ctx.safe   read off the table; nil when
--                       not read (ctx.safe is nil with no riichi;
--                       runner_up_loose is nil unless the bot's two
--                       likeliest moves are discards of different tiles)
--   ctx.human_top, ctx.human_second, ctx.human_chosen   the built-in bot's
--                       shares for its likeliest moves and for the move
--                       being made; nil with no second opinion
--   ctx.budget          { fixed_ms, add_ms, elapsed_ms } or nil
--   ctx.rng()           uniform random in [0, 1)
--   ctx.lognormal(mu, sigma)  log-normal sample in SECONDS

-- How much of a decision's place in its distribution is read (the rest is
-- noise). 0 ignores the readings; 1 makes the time a function of them.
local FOLLOW = 0.75

-- Where "few people would play this" falls among decisions: { 1 - share of
-- players making the move, share of decisions at or below it }, measured
-- from decision logs.
local PLACE_HUMAN = {
  discard = { { 0.04, 0.10 }, { 0.11, 0.20 }, { 0.20, 0.30 }, { 0.37, 0.40 },
              { 0.56, 0.50 }, { 0.72, 0.60 }, { 0.80, 0.70 }, { 0.92, 0.80 },
              { 0.95, 0.90 }, { 0.97, 0.95 }, { 1.00, 1.00 } },
  other   = { { 0.05, 0.30 }, { 0.31, 0.50 }, { 0.68, 0.70 }, { 0.97, 0.90 },
              { 1.00, 1.00 } },
}

-- The time bank is for the decisions that earn it: from this place up. The
-- rest finish inside the free window.
local BANK_PLACE = 0.75

-- The dealer's first discard on Mahjong Soul (the only platform that sets
-- ctx.dealer_opening): its window opens with the deal, which takes
-- about three seconds to lay out (the client takes no press before then).
-- The hand is looked at once it is laid out, so the think comes after the
-- deal, not under it; the eye lands on the hand a moment after.
local DEAL_S = 3.2
local DEAL_LAG = 0.3
-- Mahjong Soul gives that window 8 s rather than 5, and starts its clock —
-- the time use the client reports, which the bank is charged by — only
-- later: 1.0 to 3.7 s after the window opens, 1.75 s typically. So the free
-- window, counted from the opening as delay_ms is, runs that much longer. A
-- second of it is used.
local DEAL_UNCOUNTED_S = 1.0
-- Akagi caps the window at fixed_ms less its safety margin and the click's
-- own time; an opening think past this needs allow_bank to keep its length.
local DEAL_CUTOFF_S = 2.0

-- A tile thrown against a riichi: longer when it is not safe.
local LIVE_TILE = 1.35
local SAFE_TILE = 1.10

-- Share of discards that are routine — out in about a second, the tile was
-- already decided — and the log-normal { mu, sigma } (ln-seconds) of the ones
-- that are thought about.
local ROUTINE = {
  tedashi   = { honor = 0.12, terminal = 0.00, middle = 0.00 },
  tsumogiri = { honor = 0.15, terminal = 0.10, middle = 0.04 },
}
local THINK = {
  tedashi   = { honor = { 0.77, 0.50 }, terminal = { 0.83, 0.50 },
                middle = { 1.01, 0.55 }, default = { 0.87, 0.55 } },
  tsumogiri = { honor = { 0.54, 0.45 }, terminal = { 0.57, 0.45 },
                middle = { 0.63, 0.48 }, default = { 0.57, 0.47 } },
}
local OTHER = {
  reach           = { 1.10, 0.55 }, -- riichi declaration   (median ~3.0s)
  claim           = { 0.26, 0.57 }, -- call window incl pass (median ~1.3s)
  post_call_dahai = { 0.52, 0.42 }, -- discard after own call (median ~1.7s)
  hora            = { 0.15, 0.50 }, -- ron / tsumo           (median ~1.2s)
  in_riichi       = { 0.35, 0.30 }, -- skip while in riichi  (median ~1.4s)
}

local function clamp(x, lo, hi)
  if x < lo then return lo end
  if x > hi then return hi end
  return x
end

-- A standard normal draw, and the standard normal's quantile (Abramowitz and
-- Stegun 26.2.23, good to a thousandth), kept inside the middle 98%.
local function normal(ctx)
  return math.log(ctx.lognormal(0, 1))
end

local function quantile(u)
  u = clamp(u, 0.01, 0.99)
  local tail = u < 0.5 and u or 1 - u
  local t = math.sqrt(-2 * math.log(tail))
  local z = t - (2.515517 + 0.802853 * t + 0.010328 * t * t)
                / (1 + 1.432788 * t + 0.189269 * t * t + 0.001308 * t * t * t)
  return u < 0.5 and -z or z
end

-- The share of decisions at or below `x`, from a table of knots.
local function placed(ctx, knots, x)
  local below_x, below_u = 0, 0
  for i, knot in ipairs(knots) do
    if x <= knot[1] then
      if i == 1 and #knots > 1 then return ctx.rng() * knot[2] end
      return below_u + (knot[2] - below_u) * (x - below_x) / (knot[1] - below_x)
    end
    below_x, below_u = knot[1], knot[2]
  end
  return 1
end

-- Clearing the hand: the tile is loose, nobody is in riichi, and the
-- runner-up — when there is one — is loose too. Which goes first is not a
-- question.
local function clearing(ctx)
  return ctx.action == "dahai" and ctx.loose == true and ctx.safe == nil
    and ctx.runner_up_loose ~= false
end

-- The share of decisions no harder than this one, 0..1: where what people
-- would do puts it, or anywhere with no second opinion.
local function place(ctx)
  if clearing(ctx) then return ctx.rng() * 0.3 end
  if ctx.human_chosen == nil then return ctx.rng() end
  local discard = ctx.action == "dahai" or ctx.action == "reach"
  return placed(ctx, discard and PLACE_HUMAN.discard or PLACE_HUMAN.other,
                clamp(1 - ctx.human_chosen, 0, 1))
end

function decide_delay(ctx)
  local u = place(ctx)
  -- This decision's place in its distribution, in standard deviations.
  local z = FOLLOW * quantile(u) + math.sqrt(1 - FOLLOW * FOLLOW) * normal(ctx)
  local function drawn(params)
    return math.exp(params[1] + params[2] * z)
  end

  if ctx.in_riichi then
    -- Only call windows arrive in riichi; a glance and a pass.
    return { delay_ms = drawn(OTHER.in_riichi) * 1000 }
  end

  local think
  local giri = ctx.tsumogiri and "tsumogiri" or "tedashi"
  if ctx.action == "dahai" then
    if ctx.post_call then
      think = drawn(OTHER.post_call_dahai)
    else
      -- The easier the decision, the likelier it was routine: twice the
      -- measured share for the easiest, none for the hardest, the measured
      -- share over all of them.
      local routine = (ROUTINE[giri][ctx.tile_class or "middle"] or 0.04) * 2 * (1 - u)
      if ctx.opponent_riichi then routine = routine * 0.4 end
      if ctx.rng() < routine then
        think = math.exp(0.18 * normal(ctx))
      else
        think = drawn(THINK[giri][ctx.tile_class or "default"] or THINK[giri].default)
        -- Deeper hands are read longer, from the hand only.
        if giri == "tedashi" and ctx.junme ~= nil and ctx.junme > 0 then
          think = think * (1.0 + 0.012 * math.min(ctx.junme, 15))
        end
      end
    end
    -- Against a riichi every tile is a question of safety, and a live one
    -- more than a safe one.
    if ctx.safe == false then
      think = think * LIVE_TILE
    elseif ctx.safe == true then
      think = think * SAFE_TILE
    elseif ctx.opponent_riichi then
      think = think * (giri == "tedashi" and 1.16 or 1.25)
    end
  elseif ctx.action == "reach" then
    think = drawn(OTHER.reach)
  elseif ctx.action == "hora" then
    think = drawn(OTHER.hora)
  elseif ctx.action == "ankan" or ctx.action == "kakan" then
    think = drawn(THINK.tedashi.middle)
  else
    think = drawn(OTHER.claim)
  end

  -- The first look at a new hand.
  if ctx.first_action then think = think + 0.5 + 0.8 * ctx.rng() end
  -- A riichi that could be declared is a decision even when it is not made.
  if ctx.can_riichi and ctx.action == "dahai" then think = think + 0.4 + 0.8 * ctx.rng() end

  -- Now and then, a real tank over a tile: among the hardest tenth, one in five.
  local allow_bank = false
  if (ctx.action == "dahai" or ctx.action == "reach") and u > 0.9 and ctx.rng() < 0.2 then
    think = think + 2.0 + ctx.lognormal(0.8, 0.6)
    allow_bank = true
  end

  local deal = 0
  if ctx.dealer_opening then deal = DEAL_S + DEAL_LAG * ctx.rng() end
  think = deal + think

  -- The server gives fixed_ms per decision and a bank (add_ms) that refills
  -- every hand. The harder decisions may dip into a bank that has something
  -- in it; the rest wrap up inside the free window, a little before its end
  -- rather than all at the same instant, and never inside the deal.
  if ctx.budget ~= nil then
    local free_s = math.max(ctx.budget.fixed_ms / 1000 - 1.0, 0)
    if ctx.dealer_opening then free_s = free_s + DEAL_UNCOUNTED_S end
    if think > free_s then
      if u >= BANK_PLACE and ctx.budget.add_ms / 1000 >= 3.0 then
        allow_bank = true
      else
        allow_bank = false
        think = math.max(free_s - 0.8 * ctx.rng(), deal)
      end
    end
    -- The opening's window is counted from the moment it opens and cut off
    -- before the free window above ends: past that, Akagi has to be told
    -- the time may run on.
    if ctx.dealer_opening and think > ctx.budget.fixed_ms / 1000 - DEAL_CUTOFF_S then
      allow_bank = true
    end
  end

  return { delay_ms = think * 1000, allow_bank = allow_bank }
end
