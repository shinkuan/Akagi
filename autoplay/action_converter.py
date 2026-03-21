"""Convert MJAI action dicts to Majsoul Liqi protocol bytes."""

from mitm.bridge.majsoul.bridge import MJAI_TILE_2_MS_TILE, Operation
from mitm.bridge.majsoul.liqi import LiqiProto, MsgType


class ActionConverter:
    """Converts MJAI bot actions to Majsoul Liqi protocol format."""

    def __init__(self):
        self.liqi_proto = LiqiProto()

    def _tile_to_ms(self, mjai_tile: str) -> str:
        return MJAI_TILE_2_MS_TILE.get(mjai_tile, mjai_tile)

    def convert(self, mjai_action: dict, seat: int) -> bytes | None:
        """Convert an MJAI action dict to Liqi protocol bytes.

        Args:
            mjai_action: MJAI action from bot, e.g. {"type": "dahai", "pai": "5m"}
            seat: Player's seat index (0-3)

        Returns:
            Encoded Liqi protocol bytes, or None if action is skip/none.
        """
        action_type = mjai_action.get("type")

        if action_type in ("none", None):
            return None

        if action_type == "dahai":
            return self._build_discard(mjai_action, seat)
        elif action_type == "reach":
            return self._build_reach(mjai_action, seat)
        elif action_type == "chi":
            return self._build_chi(mjai_action, seat)
        elif action_type == "pon":
            return self._build_pon(mjai_action, seat)
        elif action_type in ("daiminkan", "kakan", "ankan"):
            return self._build_kan(mjai_action, seat)
        elif action_type == "hora":
            return self._build_hora(mjai_action, seat)
        elif action_type == "ryukyoku":
            return self._build_ryukyoku(seat)
        else:
            return None

    def _build_discard(self, action: dict, seat: int) -> bytes:
        tile = self._tile_to_ms(action["pai"])
        moqie = action.get("tsumogiri", False)
        return self._compose_action(".lq.FastTest.inputOperation", {
            "type": Operation.Discard,
            "tile": tile,
            "moqie": moqie,
            "timeuse": 3,
        })

    def _build_reach(self, action: dict, seat: int) -> bytes:
        return self._compose_action(".lq.FastTest.inputOperation", {
            "type": Operation.Liqi,
            "tile": self._tile_to_ms(action.get("pai", "")),
            "moqie": action.get("tsumogiri", False),
            "timeuse": 3,
        })

    def _build_chi(self, action: dict, seat: int) -> bytes:
        return self._compose_action(".lq.FastTest.inputChiPengGang", {
            "type": 0,  # Chi
            "index": 0,
            "timeuse": 3,
        })

    def _build_pon(self, action: dict, seat: int) -> bytes:
        return self._compose_action(".lq.FastTest.inputChiPengGang", {
            "type": 1,  # Peng
            "index": 0,
            "timeuse": 3,
        })

    def _build_kan(self, action: dict, seat: int) -> bytes:
        action_type = action["type"]
        if action_type == "daiminkan":
            return self._compose_action(".lq.FastTest.inputChiPengGang", {
                "type": 2,  # Gang
                "index": 0,
                "timeuse": 3,
            })
        elif action_type == "ankan":
            return self._compose_action(".lq.FastTest.inputOperation", {
                "type": Operation.AnGang,
                "tile": self._tile_to_ms(action.get("consumed", [""])[0]),
                "timeuse": 3,
            })
        elif action_type == "kakan":
            return self._compose_action(".lq.FastTest.inputOperation", {
                "type": Operation.JiaGang,
                "tile": self._tile_to_ms(action.get("pai", "")),
                "timeuse": 3,
            })
        return None

    def _build_hora(self, action: dict, seat: int) -> bytes:
        is_tsumo = action.get("actor") == action.get("target", action.get("actor"))
        op_type = Operation.Zimo if is_tsumo else Operation.Hu
        return self._compose_action(".lq.FastTest.inputOperation", {
            "type": op_type,
            "timeuse": 1,
        })

    def _build_ryukyoku(self, seat: int) -> bytes:
        return self._compose_action(".lq.FastTest.inputOperation", {
            "type": Operation.LiuJu,
            "timeuse": 1,
        })

    def _compose_action(self, method: str, data: dict) -> bytes:
        """Compose a Liqi protocol request message."""
        msg = {
            "type": MsgType.Req,
            "method": method,
            "data": data,
        }
        return self.liqi_proto.compose(msg)
