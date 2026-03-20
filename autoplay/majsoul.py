"""
Majsoul-specific AutoPlay implementation.

Handles two responsibilities:
1. In-game: clicking tiles / action buttons based on MJAI responses.
2. Lobby:   navigating to Silver Room (银之间) 4-player South and starting a match.
"""

import time
import random
import threading

import win32gui
import win32api
import win32con
import pyautogui

from settings.settings import settings
from .logger import logger
from . import coords as C

# Sizes of tehai_mjai when it is the player's turn to discard
# (13 hand tiles + 1 drawn tile, minus 3 per meld already made)
_HAND_SIZES_WITH_TSUMO = {14, 11, 8, 5, 2}

# Disable pyautogui fail-safe (moving mouse to corner won't abort)
pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0.0

# CEF render widget class name (Steam/standalone Majsoul uses Chromium Embedded Framework)
_CEF_RENDER_CLASS = "Chrome_RenderWidgetHostHWND"


def _find_cef_render_widget(hwnd: int) -> int:
    """
    Find the CEF render widget child window under the given top-level hwnd.
    Returns the child hwnd if found, otherwise 0.

    Steam Majsoul is a CEF app; actual mouse input is processed by the
    Chrome_RenderWidgetHostHWND child window, not the top-level window.
    PostMessage to this child works without bringing the window to foreground.
    """
    result: list[int] = []

    def _cb(child_hwnd: int, _):
        if win32gui.GetClassName(child_hwnd) == _CEF_RENDER_CLASS:
            result.append(child_hwnd)

    try:
        win32gui.EnumChildWindows(hwnd, _cb, None)
    except Exception:
        pass
    return result[0] if result else 0


# ---------------------------------------------------------------------------
# Window name patterns used to locate the Majsoul game window
# ---------------------------------------------------------------------------
MAJSOUL_WINDOW_KEYWORDS = [
    "mahjong soul",
    "雀魂",
    "jantama",
    "majsoul",
    # Steam client window titles (may include "steam" suffix or differ by locale)
    "雀魂麻将",
    "mah-jong soul",
]


def _window_matches(title: str) -> bool:
    return any(kw in title.lower() for kw in MAJSOUL_WINDOW_KEYWORDS)


class MajsoulAutoPlay:
    """
    Performs in-game actions and lobby navigation for Majsoul.

    Usage:
        impl = MajsoulAutoPlay(mjai_bot)
        impl.set_window(hwnd)
        impl.act(mjai_response)          # called each time AI makes a decision
        impl.join_next_game()            # called after end_game to re-queue
    """

    def __init__(self, bot):
        self._bot = bot          # AkagiBot instance (provides tehai_mjai, etc.)
        self._hwnd: int = 0

    # ------------------------------------------------------------------
    # Window helpers
    # ------------------------------------------------------------------

    def set_window(self, hwnd: int) -> None:
        self._hwnd = hwnd

    def get_window_rect(self) -> tuple[int, int, int, int] | None:
        """
        Returns (left, top, right, bottom) of the game window in screen
        coordinates (same coordinate system as pyautogui).
        Returns None if the window handle is not valid.
        """
        if not self._hwnd or not win32gui.IsWindow(self._hwnd):
            return None
        return win32gui.GetWindowRect(self._hwnd)

    def _get_cef_render_widget(self) -> int:
        """Return cached CEF render widget hwnd, re-querying if stale."""
        render_hwnd = _find_cef_render_widget(self._hwnd)
        if render_hwnd:
            logger.debug(f"CEF render widget found: hwnd={render_hwnd}")
        else:
            logger.debug("CEF render widget not found, will fall back to top-level hwnd")
        return render_hwnd

    def _click(self, rel_x: float, rel_y: float, delay: float = 0.0) -> None:
        """
        Click at a relative position inside the game window.

        Tries background PostMessage to the CEF render widget first (no focus
        required, real mouse cursor stays untouched).  Falls back to the old
        SetForegroundWindow + pyautogui path if the render widget is not found.
        """
        if delay > 0:
            time.sleep(delay)

        if not self._hwnd or not win32gui.IsWindow(self._hwnd):
            logger.error("_click: game window not found, skipping click")
            return

        render_hwnd = self._get_cef_render_widget()

        if render_hwnd:
            self._click_postmessage(render_hwnd, rel_x, rel_y)
        else:
            self._click_foreground(rel_x, rel_y)

    def _click_postmessage(self, target_hwnd: int, rel_x: float, rel_y: float) -> None:
        """
        Send WM_LBUTTONDOWN / WM_LBUTTONUP to the CEF render widget using
        PostMessage.  Coordinates are in client space of the target window.
        The game window does NOT need to be in the foreground.
        """
        rect = win32gui.GetClientRect(target_hwnd)  # (0, 0, width, height)
        w = rect[2]
        h = rect[3]
        cx = int(rel_x * w)
        cy = int(rel_y * h)
        lparam = win32api.MAKELONG(cx, cy)

        win32api.PostMessage(target_hwnd, win32con.WM_LBUTTONDOWN, win32con.MK_LBUTTON, lparam)
        time.sleep(0.05)
        win32api.PostMessage(target_hwnd, win32con.WM_LBUTTONUP, 0, lparam)

        logger.debug(
            f"PostMessage click rel=({rel_x:.3f}, {rel_y:.3f}) → "
            f"client=({cx}, {cy}) on hwnd={target_hwnd} [{w}x{h}]"
        )

    def _click_foreground(self, rel_x: float, rel_y: float) -> None:
        """Fallback: bring window to foreground and use pyautogui."""
        rect = self.get_window_rect()
        if rect is None:
            logger.error("_click_foreground: get_window_rect returned None")
            return
        left, top, right, bottom = rect
        w = right - left
        h = bottom - top
        x = int(left + rel_x * w)
        y = int(top  + rel_y * h)
        try:
            win32gui.SetForegroundWindow(self._hwnd)
        except Exception:
            pass
        pyautogui.click(x, y)
        logger.debug(
            f"Foreground click rel=({rel_x:.3f}, {rel_y:.3f}) → screen=({x}, {y}) "
            f"[window: left={left} top={top} {w}x{h}]"
        )

    # ------------------------------------------------------------------
    # In-game tile / button clicks
    # ------------------------------------------------------------------

    def _tile_x(self, index: int) -> float:
        """Relative X of the tile at the given hand index (0-based)."""
        return C.HAND_TILE_START_X + index * C.HAND_TILE_WIDTH

    def _tsumo_x(self, hand_size: int) -> float:
        """Relative X of the drawn tile (tsumo), placed after the hand."""
        return C.HAND_TILE_START_X + hand_size * C.HAND_TILE_WIDTH + C.HAND_TSUMO_GAP

    def _random_delay(self) -> float:
        cfg = settings.autoplay_time
        return random.uniform(cfg.rand_min, cfg.rand_max)

    def _candidate_delay(self) -> float:
        return settings.autoplay_time.candidate

    def _build_visual_hand(self) -> tuple[list[str], str | None]:
        """
        Reproduce the same hand/tsumo split used by Akagi's Tehai widget.

        Returns:
            (visual_hand, tsumo_tile)
            - visual_hand: the 13/10/7/4/1 sorted tiles shown in the hand area
            - tsumo_tile:  the drawn tile shown separately on the right, or None
        """
        tehai: list[str] = list(self._bot.tehai_mjai)
        tsumo: str = self._bot.last_self_tsumo

        if len(tehai) not in _HAND_SIZES_WITH_TSUMO:
            return tehai, None

        # Exact match first
        if tsumo in tehai:
            visual_hand = tehai[:]
            visual_hand.remove(tsumo)
            return visual_hand, tsumo

        # Handle red-dora mismatch: e.g. tsumo="5mr" but tehai has "5m", or vice versa
        tsumo_base = tsumo.replace("r", "")
        for i, t in enumerate(tehai):
            if t.replace("r", "") == tsumo_base:
                visual_hand = tehai[:]
                visual_hand.pop(i)
                return visual_hand, t  # return the actual tile name in hand as tsumo

        return tehai, None

    def click_tile(self, pai: str, tsumogiri: bool) -> None:
        """Click the tile to discard.

        Majsoul requires two clicks on the same tile: the first selects it
        (the tile rises up), the second confirms the discard.
        """
        delay = self._random_delay()
        visual_hand, tsumo_tile = self._build_visual_hand()
        logger.debug(
            f"click_tile: pai={pai!r} tsumogiri={tsumogiri} "
            f"visual_hand={visual_hand} tsumo={tsumo_tile!r}"
        )

        # Click the drawn tile on the far right
        if tsumogiri or (tsumo_tile is not None and pai == tsumo_tile):
            if tsumo_tile is None:
                # No separate tsumo, fall through to hand search below
                pass
            else:
                rel_x = self._tsumo_x(len(visual_hand))
                logger.debug(f"  → tsumo click at rel_x={rel_x:.3f}")
                self._click(rel_x, C.HAND_TILE_Y, delay=delay)
                self._click(rel_x, C.HAND_TILE_Y, delay=C.TILE_CONFIRM_DELAY)
                return

        # Find tile index in the visual hand (NOT in the full tehai_mjai)
        try:
            idx = visual_hand.index(pai)
        except ValueError:
            # Try matching without red-dora suffix (e.g. "5mr" → "5m")
            base = pai.replace("r", "")
            try:
                idx = next(i for i, t in enumerate(visual_hand) if t.replace("r", "") == base)
            except StopIteration:
                logger.error(
                    f"Tile {pai!r} not found in visual hand {visual_hand} "
                    f"(full tehai_mjai={list(self._bot.tehai_mjai)})"
                )
                return

        rel_x = self._tile_x(idx)
        logger.debug(f"  → hand tile index={idx} rel_x={rel_x:.3f}")
        self._click(rel_x, C.HAND_TILE_Y, delay=delay)
        self._click(rel_x, C.HAND_TILE_Y, delay=C.TILE_CONFIRM_DELAY)

    def click_action_button(self, rel_x: float) -> None:
        self._click(rel_x, C.ACTION_BTN_Y, delay=self._candidate_delay())

    def click_chi_sub(self, sub_index: int) -> None:
        """Click the chi sub-option at position sub_index (0 = leftmost).

        Uses C.CHI_SUB_DELAY so the panel animation has time to finish before
        the click lands (adjustable in coords.py).
        """
        rel_x = C.CHI_SUB_BTN_START_X + sub_index * C.CHI_SUB_SPACING
        self._click(rel_x, C.CHI_SUB_BTN_Y, delay=C.CHI_SUB_DELAY)

    # ------------------------------------------------------------------
    # Main act() dispatcher
    # ------------------------------------------------------------------

    def act(self, mjai_response: dict) -> bool:
        """
        Process one MJAI response and perform the corresponding UI action.
        Returns True if an action was performed, False otherwise.
        """
        action_type = mjai_response.get("type", "none")

        if action_type == "dahai":
            pai = mjai_response.get("pai", "")
            tsumogiri = mjai_response.get("tsumogiri", False)
            self.click_tile(pai, tsumogiri)
            return True

        if action_type == "reach":
            time.sleep(C.ACTION_BTN_DELAY)
            self.click_action_button(C.RIICHI_BTN_X)
            return True

        if action_type == "pon":
            time.sleep(C.ACTION_BTN_DELAY)
            self.click_action_button(C.PON_BTN_X)
            return True

        if action_type == "chi":
            consumed = mjai_response.get("consumed", [])
            time.sleep(C.ACTION_BTN_DELAY)
            self._handle_chi(consumed)
            return True

        if action_type in ("daiminkan", "ankan", "kakan"):
            time.sleep(C.ACTION_BTN_DELAY)
            self.click_action_button(C.KAN_BTN_X)
            return True

        if action_type == "hora":
            time.sleep(C.HORA_BTN_DELAY)
            self.click_action_button(C.HORA_BTN_X)
            return True

        if action_type == "none":
            # Only click skip if the bot is expected to act (game is waiting)
            if self._bot.can_act or self._bot.can_act_3p:
                time.sleep(C.ACTION_BTN_DELAY)
                self.click_action_button(C.SKIP_BTN_X)
                return True
            return False

        if action_type == "ryukyoku":
            # Nine terminals draw – click pass/skip to accept the draw
            time.sleep(C.ACTION_BTN_DELAY)
            self.click_action_button(C.SKIP_BTN_X)
            return True

        logger.warning(f"Unhandled action type for autoplay: {action_type!r}")
        return False

    def _handle_chi(self, consumed: list[str]) -> None:
        """Click chi button then select the correct chi combo sub-option."""
        self.click_action_button(C.CHI_BTN_X)

        # Identify which combo index matches `consumed`
        try:
            combos: list[list[str]] = self._bot.find_chi_consume_simple()
        except Exception as e:
            logger.warning(f"Could not enumerate chi combos: {e}")
            # Fall back: click the first sub-option
            self.click_chi_sub(0)
            return

        # Normalize for comparison: ignore tile order and red-dora suffix.
        # Majsoul shows ONE sub-option per unique sequence regardless of whether
        # a red-dora variant exists, so deduplicate before computing the index.
        def norm(tiles):
            return tuple(sorted(t.replace("r", "") for t in tiles))

        target = norm(consumed)

        seen: list[tuple] = []
        unique_combos: list[list[str]] = []
        for combo in combos:
            n = norm(combo)
            if n not in seen:
                seen.append(n)
                unique_combos.append(combo)

        sub_index = 0
        for i, combo in enumerate(unique_combos):
            if norm(combo) == target:
                sub_index = i
                break
        else:
            logger.warning(
                f"chi consumed={consumed} not matched in combos={combos}; "
                "defaulting to sub_index=0"
            )

        logger.debug(
            f"_handle_chi: consumed={consumed} target={target} "
            f"unique_combos={unique_combos} → sub_index={sub_index}"
        )
        self.click_chi_sub(sub_index)

    # ------------------------------------------------------------------
    # Lobby navigation: find and join Silver Room 4-player South
    # ------------------------------------------------------------------

    def join_next_game(self) -> None:
        """
        Navigate the Majsoul lobby to enter Silver Room (银之间) 4-player South
        and start matchmaking.  Called after each end_game event.

        Runs in a background thread so it does not block the Textual UI.
        """
        thread = threading.Thread(target=self._lobby_nav_sequence, daemon=True)
        thread.start()

    def _lobby_nav_sequence(self) -> None:
        logger.info("AutoPlay: starting lobby navigation for next game")

        try:
            # Step 0 – wait for result/settlement animation to finish
            time.sleep(C.LOBBY_RESULT_WAIT)

            # Steps 1-4 – click "确认" four times to advance through all
            # result/summary screens before the rematch button appears
            for _ in range(3):
                self._click(C.RESULT_CONFIRM_X, C.RESULT_CONFIRM_Y)
                time.sleep(C.LOBBY_NAV_DELAY)

            # Step 5 – click "再来一场" (Play Again) at the bottom-right
            self._click(C.LOBBY_REMATCH_X, C.LOBBY_REMATCH_Y)
            time.sleep(C.LOBBY_NAV_DELAY)

            # Step 4 – confirm the "再来一场" dialog (center "确认" button)
            self._click(C.LOBBY_REMATCH_CONFIRM_X, C.LOBBY_REMATCH_CONFIRM_Y)

            logger.info("AutoPlay: rematch confirmed, waiting for next game")
        except Exception as e:
            logger.error(f"AutoPlay lobby navigation failed: {e}")
