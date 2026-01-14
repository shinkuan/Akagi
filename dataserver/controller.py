import traceback

from akagi.libriichi_helper import meta_to_recommend
from mjai_bot.bot import AkagiBot
from settings.settings import settings

from .dataserver import DataServer
from .logger import logger


class DataServerController:
    """Manage dataserver lifecycle and push game state to SSE clients."""

    def __init__(self):
        self.server: DataServer | None = None

    def start(self) -> None:
        cfg = settings.dataserver
        if not cfg.enable:
            return
        if self.server and self.server.running:
            return

        self.server = DataServer(host=cfg.host, port=cfg.port)
        try:
            self.server.start()
            logger.info(f"Dataserver started on {cfg.host}:{cfg.port}")
        except Exception:
            logger.error(f"Failed to start dataserver: {traceback.format_exc()}")
            self.server = None

    def stop(self) -> None:
        if self.server and self.server.running:
            logger.info("Stopping dataserver...")
            self.server.stop()
            try:
                self.server.join(timeout=5)
            except Exception:
                pass
        self.server = None

    def push(self, mjai_msg: dict, bot: AkagiBot | None) -> None:
        """
        Push game state to dataserver for SSE broadcast.

        Payload structure:
        {
            "recommendations": [                    # Top action recommendations from model
                {
                    "action": str,                  # Action type: discard tile / reach / chi_xxx / pon / ...
                    "confidence": float,            # Model confidence score
                    "tile": str,                    # Target tile (mjai format)
                    "consumed": list[str] | None,   # Tiles consumed for chi/pon/kan
                },
                ...
            ],
            "tehai": list[str],                     # Current hand tiles (mjai format)
            "last_kawa_tile": str | None,           # Last discarded tile by others
            "best_action": {                        # Model's best action decision
                "type": str,                        # Action type
                "pai": str | None,                  # Target tile
                "consumed": list[str] | None,       # Consumed tiles
                "tsumogiri": bool | None,           # Whether it's tsumogiri
                "actor": int | None,                # Actor seat
            },
        }
        """
        if not (settings.dataserver.enable and bot and self.server and self.server.running):
            return

        # Parse recommendations from model meta
        meta = mjai_msg.get("meta")
        recommendations = None
        if meta and "q_values" in meta and "mask_bits" in meta:
            try:
                recommendations = meta_to_recommend(meta, bot.is_3p)
            except Exception as e:
                logger.debug(f"Failed to parse recommendations: {e}")

        # Format recommendations for frontend
        formatted = []
        for action, confidence in recommendations or []:
            rec = self._format_rec(action, confidence, bot)
            if rec:
                formatted.append(rec)

        # Build best action from mjai response
        best_action = None
        if isinstance(mjai_msg, dict) and isinstance(mjai_msg.get("type"), str):
            best_action = {"type": mjai_msg["type"]}
            for key in ("pai", "consumed", "tsumogiri", "actor"):
                if key in mjai_msg:
                    best_action[key] = mjai_msg[key]

        payload = {
            "recommendations": formatted or None,
            "tehai": bot.tehai_mjai,
            "last_kawa_tile": bot.last_kawa_tile,
            "best_action": best_action,
        }

        try:
            # logger.debug(f"Pushing to dataserver: {payload}")
            self.server.update(payload)
        except Exception:
            logger.error(f"Failed to push to dataserver: {traceback.format_exc()}")

    def _format_rec(self, action: str, confidence: float, bot: AkagiBot) -> dict | None:
        """
        Format a single recommendation for SSE payload.
        Maps action to tile and consumed tiles based on current game state.
        """
        rec = {"action": action, "confidence": float(confidence)}
        last = bot.last_kawa_tile

        try:
            # Reach - tile unknown until discard
            if action == "reach":
                if not bot.can_riichi:
                    return None
                rec["tile"] = "?"
                return rec

            # Chi - find matching meld from candidates
            if action in ("chi_low", "chi_mid", "chi_high"):
                if not last:
                    return None
                chi = bot.find_chi_candidates_simple()
                meld = None
                if action == "chi_low" and bot.can_chi_low:
                    meld = chi.chi_low_meld
                elif action == "chi_mid" and bot.can_chi_mid:
                    meld = chi.chi_mid_meld
                elif action == "chi_high" and bot.can_chi_high:
                    meld = chi.chi_high_meld
                if not meld:
                    return None
                rec["tile"], rec["consumed"] = meld
                return rec

            # Pon
            if action == "pon":
                if not (bot.can_pon and last):
                    return None
                rec["tile"] = last
                rec["consumed"] = [last[:2]] * 2
                return rec

            # Kan
            if action == "kan_select":
                if not bot.can_kan:
                    return None
                if bot.can_daiminkan and last:
                    rec["tile"] = last
                    rec["consumed"] = [last[:2]] * 3
                else:
                    rec["tile"] = "?"
                return rec

            # Hora (agari)
            if action == "hora":
                if not bot.can_agari:
                    return None
                if bot.can_ron_agari and last:
                    rec["tile"] = last
                elif bot.can_tsumo_agari and bot.last_self_tsumo:
                    rec["tile"] = bot.last_self_tsumo
                else:
                    rec["tile"] = "?"
                return rec

            # Ryukyoku
            if action == "ryukyoku":
                if not bot.can_ryukyoku:
                    return None
                rec["tile"] = "?"
                return rec

            # Nukidora (3p only)
            if action == "nukidora":
                rec["tile"] = "N"
                return rec

            # None (pass)
            if action == "none":
                rec["tile"] = "?"
                return rec

            # Default: discard tile
            if not bot.can_discard:
                return None
            rec["tile"] = action
            return rec

        except Exception as e:
            logger.debug(f"Failed to format recommendation '{action}': {e}")
            return None
