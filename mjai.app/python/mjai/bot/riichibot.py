"""Reference implementtation with using high-level API.
"""
from mjai import Bot


class RiichiBot(Bot):
    def think(self) -> str:
        if self.can_tsumo_agari:
            return self.action_tsumo_agari()
        elif self.can_ron_agari:
            return self.action_ron_agari()
        elif self.can_riichi:
            return self.action_riichi()
        elif self.can_discard:
            candidates = self.find_improving_tiles()
            for candidate in candidates:
                return self.action_discard(candidate["discard_tile"])
            return self.action_discard(
                self.last_self_tsumo or self.tehai_mjai[0]
            )
        else:
            # Response toward start_game, ryukyoku, etc
            return self.action_nothing()
