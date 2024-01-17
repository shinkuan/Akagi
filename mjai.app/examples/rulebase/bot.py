import sys

from loguru import logger

logger.remove()
logger.add(sys.stderr, format="<green>{time:HH:mm:ss}</green> <level>{level} {message}</level>")  # noqa

from mjai import Bot


class RulebaseBot(Bot):
    def think(self) -> str:
        if self.can_tsumo_agari:
            return self.action_tsumo_agari()
        elif self.can_ron_agari:
            return self.action_ron_agari()
        elif self.can_riichi:
            return self.action_riichi()

        if self.can_pon and self.is_yakuhai(self.last_kawa_tile):
            pons = self.find_pon_candidates()
            for pon in pons:
                if pon["current_shanten"] > pon["next_shanten"]:
                    return self.action_pon(consumed=pon["consumed"])

        if self.can_pon and len(self.get_call_events(self.player_id)) > 0:
            pons = self.find_pon_candidates()
            for pon in pons:
                if pon["current_shanten"] > pon["next_shanten"]:
                    return self.action_pon(consumed=pon["consumed"])

        if self.can_chi and len(self.get_call_events(self.player_id)) > 0:
            chis = self.find_chi_candidates()
            best_ukeire = max([chi["next_ukeire"] for chi in chis])
            for chi in chis:
                if (
                    chi["current_shanten"] > chi["next_shanten"]
                    and chi["next_ukeire"] == best_ukeire
                ):
                    return self.action_chi(consumed=chi["consumed"])

        if self.can_discard:
            logger.info(
                f"{self.bakaze}{self.kyoku}-{self.honba}: {self.tehai} | {self.last_self_tsumo}"  # noqa
            )

            # Tsumogiri only
            if self.self_riichi_accepted:
                return self.action_discard(self.last_self_tsumo)

            candidates = self.find_improving_tiles()
            for candidate in candidates:
                discard_tile = candidate["discard_tile"]
                if self.forbidden_tiles.get(discard_tile[:2], True):
                    continue
                return self.action_discard(discard_tile)

            return self.action_discard(
                self.last_self_tsumo or self.tehai_mjai[0]
            )
        else:
            # Response toward start_game, ryukyoku, etc
            return self.action_nothing()


if __name__ == "__main__":
    RulebaseBot(player_id=int(sys.argv[1])).start()
