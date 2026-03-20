"""
Majsoul AutoPlay coordinate constants (relative to window, 0.0~1.0).

All positions assume a 16:9 window aspect ratio.
Adjust these values if clicks are off-target.
"""

# ---------------------------------------------------------------------------
# In-game: hand tile positions
#
# Calibrated from calibrate.py output on the actual Steam window (1792×1051):
#
#   GetWindowRect returns the physical pixel rect of the entire window, which
#   is the same coordinate space pyautogui.click() uses — no DPI correction needed.
#
#   Steam window: left=513 top=391 width=1792 height=1051
#
#   Tile geometry (measured from 1024px logical screenshot, same ratios apply at any scale):
#     Tile 0  (二萬) center: ~147px  →  147/1024 = 0.144
#     Tile 12 (西)   center: ~880px  →  880/1024 = 0.859
#     Step: (0.859 - 0.144) / 12  =  0.060
#     Tsumo  (中)   center: ~975px  →  975/1024 = 0.952
#     Tsumo gap: 0.952 - (0.144 + 13×0.060) = 0.028
#
#   Tile Y center ≈ 0.875 of window height (includes title bar)
# ---------------------------------------------------------------------------

# Y position of hand tiles (center of tile)
HAND_TILE_Y = 0.875

# X position of the first (leftmost) tile center in a 13-tile hand
HAND_TILE_START_X = 0.144

# Center-to-center distance between adjacent tiles
HAND_TILE_WIDTH = 0.048

# Extra horizontal gap between the 13th hand tile and the drawn tile (tsumo)
HAND_TSUMO_GAP = 0.020

# ---------------------------------------------------------------------------
# In-game: action buttons (appear in lower area during decisions)
#
# Button layout in Majsoul (left→right when all options are available):
#   吃(Chi) · 碰(Pon) · 杠(Kan) · 和(Hora) · 立直(Riichi) · 跳过(Skip)
#
# Y position: buttons float above the tile row at roughly 72-73% of window height.
# Run  python calibrate.py  to verify positions with a screenshot overlay.
# ---------------------------------------------------------------------------

# Y position shared by all action buttons
ACTION_BTN_Y = 0.740

SKIP_BTN_X   = 0.650   # 跳过 / Pass  (rightmost)
CHI_BTN_X    = 0.500   # 吃            (leftmost when chi is available)
PON_BTN_X    = 0.500   # 碰
KAN_BTN_X    = 0.500   # 杠
RIICHI_BTN_X = 0.560   # 立直
HORA_BTN_X   = 0.560   # 和牌 (tsumo / ron)

# Chi sub-option buttons appear after clicking Chi; up to 3 combo tiles appear
# horizontally, centered in the lower-middle area (above the action button row).
CHI_SUB_BTN_Y       = 0.660
CHI_SUB_BTN_START_X = 0.380   # leftmost sub-option (aligns with CHI_BTN_X)
CHI_SUB_SPACING     = 0.060   # spacing between sub-options

# Seconds to wait after clicking the Chi button before clicking a sub-option.
# The sub-option panel animates in; too short and the click lands on nothing.
CHI_SUB_DELAY = 0.8
# ---------------------------------------------------------------------------
# Lobby navigation: post-game re-queue sequence
#
# Observed flow (screenshots):
#   1. Results screen (终局) – click "确认" button at bottom-right (×2)
#   2. Still on results/outro screen – click "再来一场" button at bottom-right
#   3. Confirmation dialog (再来一场) appears in center – click "确认"
# ---------------------------------------------------------------------------

# Steps 1 & 2 – "确认" button that appears at the bottom-right of the
# results screen.  The same button is clicked twice to advance past both
# the animated score panel and the final summary screen.
RESULT_CONFIRM_X = 0.870
RESULT_CONFIRM_Y = 0.930

# Step 3 – "再来一场" (Play Again) button, bottom-right of the results screen
# (sits to the left of the final 确认 button in the same row).
LOBBY_REMATCH_X = 0.790
LOBBY_REMATCH_Y = 0.930

# Step 4 – "确认" inside the "再来一场" confirmation dialog (center of screen)
LOBBY_REMATCH_CONFIRM_X = 0.420
LOBBY_REMATCH_CONFIRM_Y = 0.700

# ---------------------------------------------------------------------------
# Timing (seconds)
# ---------------------------------------------------------------------------

# Delay between the first (select) and second (confirm) click when discarding
# a tile.  Majsoul requires two clicks: the first lifts the tile, the second
# confirms the discard.  A short pause ensures the animation registers.
TILE_CONFIRM_DELAY = 0.15

# Time to wait after end_game before starting lobby navigation
# (allows result animation to finish)
LOBBY_RESULT_WAIT = 15.0

# Delay between each lobby navigation click
LOBBY_NAV_DELAY = 13.0

# Delay before clicking the hora (和牌) button, giving time to verify the win
HORA_BTN_DELAY = 3.0

# Delay before clicking pon / chi / kan / skip action buttons
ACTION_BTN_DELAY = 3.0
