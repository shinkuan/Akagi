import win32gui

from .logger import logger
from settings.settings import settings, MITMType


class WindowObject:
    """Represents a visible desktop window."""
    def __init__(self, hwnd: int, name: str):
        self.hwnd = hwnd
        self.name = name

    def __repr__(self):
        return f"WindowObject(hwnd={self.hwnd}, name={self.name!r})"


def _enum_visible_windows() -> list[WindowObject]:
    """Return all currently visible, titled windows."""
    windows: list[WindowObject] = []

    def _cb(hwnd, _):
        if win32gui.IsWindowVisible(hwnd):
            title = win32gui.GetWindowText(hwnd)
            if title:
                windows.append(WindowObject(hwnd, title))

    win32gui.EnumWindows(_cb, None)
    return windows


class AutoPlay:
    def __init__(self):
        self._bot = None
        self._impl = None   # game-specific implementation (e.g. MajsoulAutoPlay)

    # ------------------------------------------------------------------
    # Target window property (kept for API compatibility)
    # ------------------------------------------------------------------

    @property
    def target_window(self) -> WindowObject | None:
        if self._impl is None:
            return None
        hwnd = getattr(self._impl, "_hwnd", 0)
        if hwnd and win32gui.IsWindow(hwnd):
            return WindowObject(hwnd, win32gui.GetWindowText(hwnd))
        return None

    # ------------------------------------------------------------------
    # Setup
    # ------------------------------------------------------------------

    def set_bot(self, bot) -> None:
        self._bot = bot
        if self._impl is not None:
            self._impl._bot = bot

    def set_autoplay(self) -> None:
        """Instantiate the game-specific autoplay backend."""
        match settings.mitm.type:
            case MITMType.MAJSOUL:
                from .majsoul import MajsoulAutoPlay
                self._impl = MajsoulAutoPlay(self._bot)
                logger.info("AutoPlay: Majsoul backend initialised")
            case MITMType.AMATSUKI | MITMType.RIICHI_CITY:
                logger.warning(
                    f"AutoPlay: {settings.mitm.type.value} is not yet implemented"
                )
                self._impl = None
            case MITMType.TENHOU | MITMType.UNIFIED:
                logger.info(
                    f"AutoPlay: {settings.mitm.type.value} does not support autoplay"
                )
                self._impl = None
            case _:
                logger.error(f"AutoPlay: unknown MITM type {settings.mitm.type}")
                self._impl = None

    # ------------------------------------------------------------------
    # Window management
    # ------------------------------------------------------------------

    def get_windows(self) -> list[WindowObject]:
        return _enum_visible_windows()

    def select_window(self, hwnd: int) -> None:
        if self._impl is not None:
            self._impl.set_window(hwnd)
            logger.info(f"AutoPlay: window selected hwnd={hwnd}")

    def check_window(self) -> bool:
        if self._impl is None:
            return False
        hwnd = getattr(self._impl, "_hwnd", 0)
        return bool(hwnd and win32gui.IsWindow(hwnd))

    def auto_select_window(self) -> WindowObject | None:
        """
        Try to find the Majsoul game window automatically.
        Returns the matched WindowObject, or None if not found.
        """
        if self._impl is None:
            return None

        from .majsoul import MAJSOUL_WINDOW_KEYWORDS
        for win in _enum_visible_windows():
            if any(kw in win.name.lower() for kw in MAJSOUL_WINDOW_KEYWORDS):
                self._impl.set_window(win.hwnd)
                logger.info(f"AutoPlay: auto-selected window {win}")
                return win

        logger.warning("AutoPlay: could not auto-select a game window")
        return None

    # ------------------------------------------------------------------
    # In-game action
    # ------------------------------------------------------------------

    def act(self, mjai_msg: dict) -> bool:
        if self._impl is None:
            return False
        if not self.check_window():
            return False
        return self._impl.act(mjai_msg)

    # ------------------------------------------------------------------
    # Lobby: find and join next game
    # ------------------------------------------------------------------

    def join_next_game(self) -> None:
        """
        Navigate the lobby to start a new Silver Room 4-player South match.
        No-op if the backend is not available or no window is selected.
        """
        if self._impl is None:
            logger.warning("AutoPlay: join_next_game called but no backend available")
            return
        if not self.check_window():
            logger.warning("AutoPlay: join_next_game called but game window not found")
            return
        self._impl.join_next_game()
