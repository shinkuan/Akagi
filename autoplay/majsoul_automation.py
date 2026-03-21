"""Majsoul browser automation using Playwright.

Uses reverse-engineered game JS APIs for login, match-making, and action execution.
All interaction is done via JS injection into the Laya engine runtime.

Key APIs:
- app.NetAgent.sendReq2Lobby('Lobby', rpc, data, cb) - lobby RPCs
- app.NetAgent.sendReq2MJ('FastTest', rpc, data, cb) - in-game RPCs
- uiscript.UI_Entrance.Inst._try_login_account() - login
"""

import asyncio
import time
import random
from pathlib import Path
from playwright.async_api import async_playwright, Page, Browser, BrowserContext
from settings.settings import settings
from .logger import logger

JS_DIR = Path(__file__).parent / "js"
MAJSOUL_URL = "https://game.maj-soul.com/1/"

# Match mode IDs from cfg.desktop.matchmode
# Format: (game_type, room) → mode_id
# room: 1=铜之间, 2=银之间, 3=金之间, 4=玉之间, 6=王座间
# 4p modes: mode=0(速胜), mode=1(東/East), mode=2(南/South)
# 3p modes: mode=11(East), mode=12(South)
MATCH_MODE_IDS = {
    # 4-player modes (mode=1: East, mode=2: South)
    ("4p_east", "bronze"): 2,
    ("4p_south", "bronze"): 3,
    ("4p_east", "silver"): 5,
    ("4p_south", "silver"): 6,
    ("4p_east", "gold"): 8,
    ("4p_south", "gold"): 9,
    ("4p_east", "jade"): 11,
    ("4p_south", "jade"): 12,
    ("4p_east", "throne"): 15,
    ("4p_south", "throne"): 16,
    # 3-player modes
    ("3p_east", "bronze"): 17,
    ("3p_south", "bronze"): 18,
    ("3p_east", "silver"): 19,
    ("3p_south", "silver"): 20,
    ("3p_east", "gold"): 21,
    ("3p_south", "gold"): 22,
    ("3p_east", "jade"): 23,
    ("3p_south", "jade"): 24,
    ("3p_east", "throne"): 25,
    ("3p_south", "throne"): 26,
}

# MJAI tile format → Majsoul tile format
MJAI_TO_MS_TILE = {
    '5mr': '0m', '5pr': '0p', '5sr': '0s',
    'E': '1z', 'S': '2z', 'W': '3z', 'N': '4z',
    'P': '5z', 'F': '6z', 'C': '7z',
}

# Operation type constants (from Liqi protocol)
OP_DISCARD = 1
OP_CHI = 2
OP_PENG = 3
OP_AN_GANG = 4
OP_MING_GANG = 5
OP_JIA_GANG = 6
OP_LIQI = 7
OP_ZIMO = 8
OP_HU = 9
OP_LIU_JU = 10


class MajsoulAutomation:
    """Controls Majsoul browser via Playwright + JS injection."""

    def __init__(self):
        self.playwright = None
        self.browser: Browser | None = None
        self.context: BrowserContext | None = None
        self.page: Page | None = None

    async def start_browser(self, proxy_port: int = None):
        """Launch Chromium with proxy pointing to MITM.

        Uses a persistent user data directory so game assets are cached
        across runs, avoiding slow re-downloads through the MITM proxy.
        """
        self.playwright = await async_playwright().start()

        # Persistent profile directory for asset caching
        user_data_dir = Path(__file__).parent.parent / ".browser_profile"
        user_data_dir.mkdir(exist_ok=True)

        launch_args = {
            "headless": settings.autoplay_headless,
            "args": [
                "--disable-blink-features=AutomationControlled",
                "--no-sandbox",
            ],
        }

        if proxy_port:
            launch_args["proxy"] = {
                "server": f"http://127.0.0.1:{proxy_port}",
            }

        self.context = await self.playwright.chromium.launch_persistent_context(
            str(user_data_dir),
            viewport={"width": 1280, "height": 720},
            ignore_https_errors=True,
            **launch_args,
        )
        self.page = self.context.pages[0] if self.context.pages else await self.context.new_page()

        # Inject WebSocket hook before any page loads
        hook_js = (JS_DIR / "hook_websocket.js").read_text()
        await self.context.add_init_script(hook_js)

        logger.info("Browser started (persistent profile)")

    async def navigate_to_game(self):
        """Open Majsoul website and wait for game engine to load."""
        await self.page.goto(MAJSOUL_URL, wait_until="domcontentloaded", timeout=120000)
        logger.info("Majsoul page loaded")

    async def wait_for_entrance(self, timeout: float = 180):
        """Wait for the game to fully initialize to the entrance screen."""
        logger.info("Waiting for game to initialize...")
        start = time.time()
        screenshot_taken = False
        while time.time() - start < timeout:
            ready = await self.page.evaluate("""() => {
                try {
                    return !!(typeof uiscript !== 'undefined' &&
                              uiscript.UI_Entrance && uiscript.UI_Entrance.Inst);
                } catch(e) { return false; }
            }""")
            if ready:
                elapsed = int(time.time() - start)
                logger.info(f"Game initialized in {elapsed}s")
                return True
            elapsed = int(time.time() - start)
            if elapsed >= 30 and not screenshot_taken:
                await self.page.screenshot(path="logs/init_debug.png")
                logger.info("Saved init debug screenshot at 30s")
                screenshot_taken = True
            await asyncio.sleep(1)

        await self.page.screenshot(path="logs/init_timeout.png")
        logger.error(f"Game initialization timeout ({timeout}s)")
        return False

    async def login(self, username: str, password: str):
        """Auto-login using the game's own _try_login_account JS API.

        This calls the game's internal login function directly,
        which handles the full Yostar email/password login flow
        including RPC, session setup, and lobby transition.
        """
        logger.info(f"Logging in as {username}...")

        await asyncio.sleep(2)

        result = await self.page.evaluate("""(creds) => {
            try {
                const inst = uiscript.UI_Entrance.Inst;
                // login_index is the internal counter that must match
                // for the login to proceed (anti-replay)
                const counterKey = window.$u[11161];
                const counter = inst[counterKey];
                // Call the game's native login function
                inst._try_login_account(counter, creds[0], creds[1]);
                return {ok: true};
            } catch(e) { return {error: e.message}; }
        }""", [username, password])

        if result.get("error"):
            logger.error(f"Login call failed: {result['error']}")
            return False

        logger.info("Login RPC sent via game API")
        return True

    async def wait_for_lobby(self, timeout: float = 120):
        """Wait until the game enters the lobby state (or reconnects to game).

        After detecting UI_Lobby.Inst, waits extra time for the lobby
        to finish its "正在打扫大厅..." loading animation.

        Returns:
            "lobby" if entered lobby normally
            "game" if auto-reconnected to an ongoing game
            None if timed out
        """
        logger.info("Waiting for lobby...")
        start = time.time()
        while time.time() - start < timeout:
            state = await self.page.evaluate("""() => {
                try {
                    return {
                        lobby: !!(uiscript.UI_Lobby && uiscript.UI_Lobby.Inst),
                        in_game: !!(typeof view !== 'undefined' && view.DesktopMgr && view.DesktopMgr.Inst),
                        logined: GameMgr.Inst.logined || false,
                        account_id: GameMgr.Inst.account_id || -1,
                    };
                } catch(e) { return {error: e.message}; }
            }""")
            if state.get("in_game"):
                logger.info(f"Auto-reconnected to ongoing game (account_id: {state.get('account_id')})")
                return "game"
            if state.get("lobby"):
                logger.info(f"In lobby (account_id: {state.get('account_id')})")
                # Wait for lobby loading animation to finish
                logger.info("Waiting for lobby to fully load...")
                await asyncio.sleep(15)
                return "lobby"
            await asyncio.sleep(1)

        logger.error("Lobby timeout")
        return None

    def _get_match_sid(self) -> str | None:
        """Build match_sid string for startUnifiedMatch.

        Format: "{match_group}:{mode_id}" where match_group=1 for ranked.
        """
        mode_type = settings.autoplay_mode.type
        room = settings.autoplay_mode.room
        mode_id = MATCH_MODE_IDS.get((mode_type, room))
        if mode_id is None:
            return None
        # All ranked modes use match_group=1
        return f"1:{mode_id}"

    async def start_match(self):
        """Start ranked match queue via startUnifiedMatch RPC.

        Uses the new unified match API with match_sid format "group:mode_id".
        """
        match_sid = self._get_match_sid()
        if match_sid is None:
            mode_type = settings.autoplay_mode.type
            room = settings.autoplay_mode.room
            logger.error(f"Unknown match mode: {mode_type} {room}")
            return False

        logger.info(f"Starting match: {settings.autoplay_mode.type} "
                     f"{settings.autoplay_mode.room} (match_sid={match_sid})")

        result = await self.page.evaluate("""(sid) => {
            try {
                const versionStr = GameMgr.Inst.getClientVersion();
                return new Promise((resolve) => {
                    app.NetAgent.sendReq2Lobby('Lobby', 'startUnifiedMatch', {
                        match_sid: sid,
                        client_version_string: versionStr,
                    }, function(errcode, msg) {
                        resolve({errcode: errcode || 0, msg: msg || {}});
                    });
                    setTimeout(() => resolve({error: 'timeout'}), 15000);
                });
            } catch(e) { return {error: e.message}; }
        }""", match_sid)

        if result.get("error"):
            logger.error(f"startUnifiedMatch failed: {result}")
            return False

        errcode = result.get("errcode", 0)
        msg = result.get("msg", {})
        msg_error = msg.get("error", {}) if isinstance(msg, dict) else {}

        if errcode or msg_error:
            logger.error(f"startUnifiedMatch error: errcode={errcode}, msg_error={msg_error}")
            return False

        logger.info("Match queued successfully")
        return True

    async def cancel_match(self):
        """Cancel the current match queue."""
        match_sid = self._get_match_sid() or "1:5"

        await self.page.evaluate("""(sid) => {
            app.NetAgent.sendReq2Lobby('Lobby', 'cancelUnifiedMatch', {
                match_sid: sid,
            }, function(errcode, msg) {
                console.log('[Akagi] cancelUnifiedMatch:', errcode, msg);
            });
        }""", match_sid)
        logger.info("Match cancelled")

    async def wait_for_game_start(self, timeout: float = 300):
        """Wait for the match to start (game found).

        Detects game start by checking for DesktopMgr (the in-game manager).
        """
        logger.info("Waiting for game to start...")
        start = time.time()
        while time.time() - start < timeout:
            state = await self.page.evaluate("""() => {
                try {
                    return {
                        in_game: !!(view && view.DesktopMgr && view.DesktopMgr.Inst),
                        pipei: !!(uiscript.UI_PiPei && uiscript.UI_PiPei.Inst &&
                                  uiscript.UI_PiPei.Inst._enable),
                    };
                } catch(e) { return {error: e.message}; }
            }""")
            if state.get("in_game"):
                logger.info("Game started!")
                return True
            await asyncio.sleep(1)

        logger.error("Game start timeout")
        return False

    async def execute_action(self, mjai_action: dict, seat: int):
        """Execute an AI-recommended action via the game's JS API.

        Uses app.NetAgent.sendReq2MJ('FastTest', ...) to call the game's
        own RPC functions, which handles msg_id, protobuf, and state correctly.
        """
        action_type = mjai_action.get("type")
        if action_type in ("none", None):
            # Skip/pass: decline chi/pon/kan/hu opportunity
            await self._send_skip()
            return

        # Apply random delay to look human-like
        delay = random.uniform(
            settings.autoplay_time.rand_min,
            settings.autoplay_time.rand_max,
        )
        await asyncio.sleep(delay)

        pai = mjai_action.get("pai", "")
        tile = MJAI_TO_MS_TILE.get(pai, pai)

        if action_type == "dahai":
            await self._send_discard_with_retry(
                tile, mjai_action.get("tsumogiri", False)
            )
        elif action_type == "reach":
            # Riichi requires a valid tile from the server's combination list.
            # The tsumo tile (pai) may not be a valid riichi discard.
            from mitm.bridge.majsoul.bridge import get_last_operation_list
            operations = get_last_operation_list()
            liqi_op = next((op for op in operations if op.get('type') == OP_LIQI), None)
            valid_tiles = liqi_op.get('combination', []) if liqi_op else []

            if valid_tiles:
                if tile in valid_tiles:
                    # Model's chosen tile is valid
                    riichi_tile = tile
                    moqie = mjai_action.get("tsumogiri", False)
                else:
                    # Model's tile not in valid list; pick first valid option
                    riichi_tile = valid_tiles[0]
                    moqie = False
                    logger.warning(f"Riichi: model tile {tile} not in valid={valid_tiles}")
                logger.info(f"Riichi: valid={valid_tiles}, using={riichi_tile} moqie={moqie}")
            else:
                # Fallback: use tsumo tile (may fail if invalid)
                riichi_tile = tile
                moqie = mjai_action.get("tsumogiri", False)
                logger.warning(f"Riichi: no operation list, fallback tile={riichi_tile}")

            await self._send_input_operation({
                "type": OP_LIQI,
                "tile": riichi_tile,
                "moqie": moqie,
                "timeuse": 3,
            })
        elif action_type == "chi":
            await self._send_chi_peng_gang(OP_CHI, mjai_action)
        elif action_type == "pon":
            await self._send_chi_peng_gang(OP_PENG, mjai_action)
        elif action_type == "daiminkan":
            await self._send_chi_peng_gang(OP_MING_GANG, mjai_action)
        elif action_type == "ankan":
            consumed = mjai_action.get("consumed", [""])
            an_tile = MJAI_TO_MS_TILE.get(consumed[0], consumed[0]) if consumed else ""
            await self._send_input_operation({
                "type": OP_AN_GANG,
                "tile": an_tile,
                "timeuse": 3,
            })
        elif action_type == "kakan":
            await self._send_input_operation({
                "type": OP_JIA_GANG,
                "tile": tile,
                "timeuse": 3,
            })
        elif action_type == "hora":
            is_tsumo = mjai_action.get("actor") == mjai_action.get(
                "target", mjai_action.get("actor")
            )
            await self._send_input_operation({
                "type": OP_ZIMO if is_tsumo else OP_HU,
                "timeuse": 1,
            })
        elif action_type == "ryukyoku":
            await self._send_input_operation({
                "type": OP_LIU_JU,
                "timeuse": 1,
            })
        else:
            logger.warning(f"Unknown action type: {action_type}")

    async def _send_discard_with_retry(self, tile: str, moqie: bool) -> bool:
        """Send a discard with retry if the server silently ignores it.

        The Majsoul server sometimes silently ignores the first inputOperation
        after ActionNewRound when the player is the dealer (14 tiles, step 0).
        This method detects that by checking if ActionDiscardTile was received
        (via a monotonically increasing counter), and retries with increasing delays.
        """
        from mitm.bridge.majsoul.bridge import get_discard_counter

        pre_count = get_discard_counter()

        for attempt in range(3):
            params = {
                "type": OP_DISCARD,
                "tile": tile,
                "moqie": moqie,
                "timeuse": 3 + attempt * 3,
            }
            ok = await self._send_input_operation(params)
            if not ok:
                return False

            # Wait up to 5 seconds for ActionDiscardTile confirmation
            for _ in range(25):
                await asyncio.sleep(0.2)
                if get_discard_counter() > pre_count:
                    return True  # Discard was processed

            if attempt < 2:
                logger.warning(
                    f"Discard not acknowledged (attempt {attempt + 1}/3), retrying..."
                )

        logger.error("Discard failed after 3 attempts")
        return False

    async def _send_input_operation(self, params: dict) -> bool:
        """Send inputOperation RPC via game's network layer."""
        result = await self.page.evaluate("""(params) => {
            try {
                return new Promise((resolve) => {
                    app.NetAgent.sendReq2MJ('FastTest', 'inputOperation', params,
                        function(errcode, msg) {
                            resolve({errcode: errcode || 0});
                        });
                    setTimeout(() => resolve({error: 'timeout'}), 10000);
                });
            } catch(e) { return {error: e.message}; }
        }""", params)

        if result.get("error"):
            logger.error(f"inputOperation error: {result}")
            return False
        if result.get("errcode"):
            logger.error(f"inputOperation errcode={result['errcode']}")
            return False
        logger.info(f"OK: inputOperation type={params.get('type')} tile={params.get('tile', '')}")
        return True

    async def _send_skip(self) -> bool:
        """Send skip/pass via inputChiPengGang with cancel_operation=true."""
        await asyncio.sleep(0.5)
        result = await self.page.evaluate("""() => {
            try {
                return new Promise((resolve) => {
                    app.NetAgent.sendReq2MJ('FastTest', 'inputChiPengGang', {
                        cancel_operation: true,
                        timeuse: 2,
                    }, function(errcode, msg) {
                        resolve({errcode: errcode || 0});
                    });
                    setTimeout(() => resolve({error: 'timeout'}), 10000);
                });
            } catch(e) { return {error: e.message}; }
        }""")

        if result.get("error"):
            logger.error(f"skip error: {result}")
            return False
        if result.get("errcode"):
            logger.warning(f"skip errcode={result['errcode']} (may be no pending operation)")
            return False
        logger.info("OK: skip (pass)")
        return True

    async def _send_chi_peng_gang(self, type_: int, mjai_action: dict) -> bool:
        """Send inputChiPengGang RPC via game's network layer.

        type_: OP_CHI(2), OP_PENG(3), OP_MING_GANG(5) from Operation enum
        """
        from mitm.bridge.majsoul.bridge import get_last_operation_list

        # Convert consumed tiles for index matching
        consumed = mjai_action.get("consumed", [])
        ms_consumed = [MJAI_TO_MS_TILE.get(t, t) for t in consumed]

        # Find correct combination index (mainly needed for chi)
        index = 0
        if type_ == OP_CHI and ms_consumed:
            operations = get_last_operation_list()
            chi_op = next((op for op in operations if op.get('type') == 2), None)
            if chi_op and chi_op.get('combination'):
                consumed_sorted = '|'.join(sorted(ms_consumed))
                for i, combo in enumerate(chi_op['combination']):
                    combo_sorted = '|'.join(sorted(combo.split('|')))
                    if combo_sorted == consumed_sorted:
                        index = i
                        break
                logger.info(f"Chi combinations: {chi_op['combination']} → index={index}")

        result = await self.page.evaluate("""(params) => {
            try {
                return new Promise((resolve) => {
                    app.NetAgent.sendReq2MJ('FastTest', 'inputChiPengGang', params,
                        function(errcode, msg) {
                            resolve({errcode: errcode || 0});
                        });
                    setTimeout(() => resolve({error: 'timeout'}), 10000);
                });
            } catch(e) { return {error: e.message}; }
        }""", {
            "type": type_,
            "index": index,
            "timeuse": 3,
        })

        if result.get("error"):
            logger.error(f"inputChiPengGang error: {result}")
            return False
        if result.get("errcode"):
            logger.error(f"inputChiPengGang errcode={result['errcode']}")
            return False
        op_name = {OP_CHI: "chi", OP_PENG: "pon", OP_MING_GANG: "kan"}.get(type_, str(type_))
        logger.info(f"OK: {op_name} consumed={ms_consumed} index={index}")
        return True

    async def handle_end_game(self):
        """Handle the end-of-game result screen.

        Uses a combination of mouse clicks to dismiss result screens
        and checks for lobby re-entry.
        """
        logger.info("Handling end game screen...")
        await asyncio.sleep(5)

        # Click through result screens (multiple clicks needed for
        # round results → game results → back to lobby)
        for i in range(15):
            # Check if we're already back in lobby
            in_lobby = await self.page.evaluate("""() => {
                try {
                    return !!(uiscript.UI_Lobby && uiscript.UI_Lobby.Inst);
                } catch(e) { return false; }
            }""")
            if in_lobby:
                logger.info(f"Returned to lobby after {i} clicks")
                return True

            # Click center-ish area to dismiss overlays
            await self.page.mouse.click(640, 450)
            await asyncio.sleep(1.5)

        # Final wait for lobby (some transitions take longer)
        for _ in range(20):
            in_lobby = await self.page.evaluate("""() => {
                try {
                    return !!(uiscript.UI_Lobby && uiscript.UI_Lobby.Inst);
                } catch(e) { return false; }
            }""")
            if in_lobby:
                logger.info("Returned to lobby")
                return True
            await self.page.mouse.click(640, 400)
            await asyncio.sleep(2)

        logger.warning("Could not confirm lobby return")
        return False

    async def check_connection(self) -> bool:
        """Check if the game is still connected and responsive."""
        try:
            return await self.page.evaluate("window.__akagi_ws_ready()")
        except Exception:
            return False

    async def recover(self):
        """Try to recover from errors by reloading and re-logging in."""
        logger.info("Attempting recovery...")
        try:
            await self.page.reload(wait_until="domcontentloaded", timeout=120000)
            if not await self.wait_for_entrance():
                return False
            if not await self.login(
                settings.autoplay_account.username,
                settings.autoplay_account.password,
            ):
                return False
            return await self.wait_for_lobby()
        except Exception as e:
            logger.error(f"Recovery failed: {e}")
            return False

    async def close(self):
        """Clean up browser resources."""
        if self.context:
            await self.context.close()
        if self.playwright:
            await self.playwright.stop()
        logger.info("Browser closed")
