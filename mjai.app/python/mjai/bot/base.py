import json
import sys

from mjai.bot.consts import MJAI_VEC34_TILES
from mjai.bot.tools import (
    calc_shanten,
    convert_mjai_to_vec34,
    convert_vec34_to_short,
    find_improving_tiles,
    fmt_call,
    fmt_calls,
    vec34_index_to_mjai_tile,
)
from mjai.mlibriichi.state import ActionCandidate, PlayerState  # type: ignore


class Bot:
    def __init__(self, player_id: int = 0):
        self.player_id = player_id
        self.player_state = PlayerState(player_id)
        self.action_candidate: ActionCandidate | None = None
        self.__discard_events: list[dict] = []
        self.__call_events: list[dict] = []
        self.__dora_indicators: list[str] = []

    # ==========================================================
    # action_candidate properties

    @property
    def can_discard(self) -> bool:
        """
        Whether the player can discard a tile.
        """
        assert self.action_candidate is not None
        return self.action_candidate.can_discard

    @property
    def can_riichi(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_riichi

    @property
    def can_agari(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_agari

    @property
    def can_tsumo_agari(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_tsumo_agari

    @property
    def can_ron_agari(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_ron_agari

    @property
    def can_ryukyoku(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_ryukyoku

    @property
    def can_kakan(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_kakan

    @property
    def can_daiminkan(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_daiminkan

    @property
    def can_kan(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_kan

    @property
    def can_ankan(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_ankan

    @property
    def can_pon(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_pon

    @property
    def can_chi(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_chi

    @property
    def can_chi_low(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_chi_low

    @property
    def can_chi_mid(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_chi_mid

    @property
    def can_chi_high(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_chi_high

    @property
    def can_act(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_act

    @property
    def can_pass(self) -> bool:
        assert self.action_candidate is not None
        return self.action_candidate.can_pass

    @property
    def target_actor(self) -> int:
        assert self.action_candidate is not None
        return self.action_candidate.target_actor

    @property
    def target_actor_rel(self) -> int:
        """
        Relative position of target actor.

        1 = shimocha, 2 = toimen, 3 = kamicha.
        """
        return (self.target_actor - self.player_id + 4) % 4

    # ==========================================================
    # player state properties

    def validate_reaction(self, reaction: str) -> bool:
        """
        Validate the reaction string.
        """
        return self.player_state.validate_reaction(reaction)

    def brief_info(self) -> None:
        """
        Print brief information about the player state.
        """
        self.player_state.brief_info()

    @property
    def kyotaku(self) -> int:
        return self.player_state.kyotaku

    @property
    def at_furiten(self) -> bool:
        return self.player_state.at_furiten

    @property
    def is_oya(self) -> bool:
        return self.player_state.is_oya

    @property
    def last_self_tsumo(self) -> str:
        """
        Last tile that the player drew by itself.

        Tile format is mjai-style like '5mr' or 'P'.
        Return a empty string when the player's last action is not tsumo.
        """
        return self.player_state.last_self_tsumo() or ""

    @property
    def last_kawa_tile(self) -> str:
        """
        Last discarded tile in the game.

        Mjai-style tile string (like '5mr' or 'P')
        """
        return self.player_state.last_kawa_tile()

    @property
    def self_riichi_declared(self) -> bool:
        return self.player_state.self_riichi_declared

    @property
    def self_riichi_accepted(self) -> bool:
        return self.player_state.self_riichi_accepted

    @property
    def tehai_vec34(self) -> list[int]:
        """
        Player's hand as a list of tile counts.
        Aka dora is not distinguished.
        For identifying aka dora, use `self.player_akas_in_hand`.

        Example:
            >>> bot.tehai_vec34
            [1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1,
             1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0]
        """
        return self.player_state.tehai

    @property
    def tehai_mjai(self) -> list[str]:
        """
        Player's hand as a list of tile strings in mjai format.

        Example:
            >>> bot.tehai_mjai
            ["1m", "2m", "6m", "9m", "1p", "3p", "4p", "3s", "4s", "5s",
             "7s", "9s", "5z", "6z"]
        """
        tiles = []
        for tile_idx, tile_count in enumerate(self.player_state.tehai):
            if tile_idx == 4 and self.akas_in_hand[0]:
                tile_count -= 1
                tiles.append("5mr")
            elif tile_idx == 4 + 9 and self.akas_in_hand[1]:
                tile_count -= 1
                tiles.append("5pr")
            elif tile_idx == 4 + 18 and self.akas_in_hand[2]:
                tile_count -= 1
                tiles.append("5sr")

            for _ in range(tile_count):
                tiles.append(MJAI_VEC34_TILES[tile_idx])

        return tiles

    @property
    def tehai(self) -> str:
        """
        Player's hand in the riichi-tools-rs format (like 123m0456p789s111z)

        ## Open shape

        * (pXCIr) is pon, where X is 0-9, C is the color (m, p, s, z) and
          I is the index of the player from who we called
          (1 = shimocha, 2 = toimen, 3 = kamicha).
          In case the pon has a red 5, the representation will use 0
          - for example (p0m2). If it was the red 5 that was called, r is added
        * (XYZCI) is chi, where XYZ are consecutive numbers,
          C is the color (m, p, s) and I is the index of the called tile (0-2).
        * (kXCIr) is closed kan or daiminkan (open called kan).
          Same rules as pon apply, but I is optional
          - if not available, the kan is considered closed.
        * (sXCIr) is a shouminkan (added kan).
          Same rules as daiminkan above.

        ref: https://github.com/harphield/riichi-tools-rs#hand-representation-parsing  # noqa

        Example:
            >>> bot.tehai
            "1269m134p34579s56z"

            >>> bot.tehai
            "012346789m11122z"

            >>> bot.tehai
            "123m134p4567s6z(p5z3)"
        """
        tehai_str = convert_vec34_to_short(
            self.player_state.tehai, self.player_state.akas_in_hand
        )
        events = self.get_call_events(self.player_id)
        call_str = fmt_calls(events, self.player_id)

        return tehai_str + call_str

    @property
    def akas_in_hand(self) -> list[bool]:
        """
        List of aka dora indices in the player's hand.

        Example:
            >>> bot.player_akas_in_hand
            [False, False, False]
        """
        return self.player_state.akas_in_hand

    @property
    def shanten(self) -> int:
        """
        Shanten of the player's hand.

        0 indicates tenpai.
        """
        return self.player_state.shanten - 1

    @property
    def discardable_tiles(self) -> list[str]:
        """
        List of discardable tiles.

        Example:
            >>> bot.discardable_tiles
            ["1m", "2m", "6m", "9m", "1p", "3p", "4p", "3s", "4s", "5s",
             "7s", "9s", "5z", "6z"]
        """
        discardable_tiles = list(
            set(
                [
                    tile
                    for tile in self.tehai_mjai
                    if not self.forbidden_tiles[tile]
                ]
            )
        )
        return discardable_tiles

    @property
    def discardable_tiles_riichi_declaration(self) -> list[str]:
        """
        List of discardable tiles just after riichi declaration.

        Example:
            >>> bot.discardable_tiles_riichi_declaration
            ["1m", "6m"]
        """
        discardable_tiles = list(
            set(
                [
                    tile
                    for idx, tile in enumerate(self.tehai_mjai)
                    if calc_shanten(
                        convert_vec34_to_short(
                            convert_mjai_to_vec34(
                                self.tehai_mjai[:idx]
                                + self.tehai_mjai[idx + 1 :]  # noqa
                            )
                        )
                    )
                    == 0
                ]
            )
        )
        return discardable_tiles

    # ==========================================================
    # table state

    @property
    def dora_indicators(self) -> list[str]:
        return self.__dora_indicators

    def discarded_tiles(self, player_id: int | None = None) -> list[str]:
        if player_id is not None:
            return [
                ev["pai"]
                for ev in self.__discard_events
                if ev["actor"] == player_id
            ]
        return [ev["pai"] for ev in self.__discard_events]

    def get_call_events(self, player_id: int | None = None) -> list[dict]:
        if player_id is not None:
            return [
                ev for ev in self.__call_events if ev["actor"] == player_id
            ]
        return self.__call_events

    @property
    def honba(self) -> int:
        return self.player_state.honba

    @property
    def kyoku(self) -> int:
        """
        Current kyoku as 1-indexed number.
        East 1 is 1, East 2 is 2, ..., South 4 is 4.

        Example:
            >>> bot.kyoku
            2
        """
        return self.player_state.kyoku + 1

    @property
    def scores(self) -> list[int]:
        """
        Current scores of the players.

        Example:
            >>> bot.scores
            [25000, 25000, 25000, 25000]
        """
        return self.player_state.scores

    @property
    def jikaze(self) -> str:
        chars = ["E", "S", "W", "N"]
        assert self.player_state.jikaze >= 27 and self.player_state.jikaze < 31
        return chars[self.player_state.jikaze - 27]

    @property
    def bakaze(self) -> str:
        chars = ["E", "S", "W", "N"]
        assert self.player_state.bakaze >= 27 and self.player_state.bakaze < 31
        return chars[self.player_state.bakaze - 27]

    @property
    def tiles_seen(self) -> dict[str, int]:
        """
        Observable number of tiles from the player.

        Including:
        - Dora indicator tiles
        - Tiles in the player's hand
        - Tiles discarded by opponents
        - Open tiles

        Example:
            >>> bot.tiles_seen
            {"1m": 2, "2m": 1, "3m": 3, ...}
        """
        assert self.player_state.tiles_seen is not None
        assert len(self.player_state.tiles_seen) == len(MJAI_VEC34_TILES)
        return dict(zip(MJAI_VEC34_TILES, self.player_state.tiles_seen))

    @property
    def forbidden_tiles(self) -> dict[str, bool]:
        """
        Forbidden tiles to discard by Kuikae (喰い替え; swap calling) rule.

        ref: https://riichi.wiki/Kuikae

        Example:
            >>> bot.forbidden_tiles
            {"1m": False, "2m": Fales, "3m": False, ...}
        """
        assert self.player_state.forbidden_tiles is not None
        assert len(self.player_state.forbidden_tiles) == len(MJAI_VEC34_TILES)
        return dict(zip(MJAI_VEC34_TILES, self.player_state.forbidden_tiles))

    # ==========================================================
    # actions

    def action_discard(self, tile_str: str) -> str:
        """
        Return a dahai event as a JSON string.
        """
        last_self_tsumo = self.player_state.last_self_tsumo()
        return json.dumps(
            {
                "type": "dahai",
                "pai": tile_str,
                "actor": self.player_id,
                "tsumogiri": tile_str == last_self_tsumo,
            },
            separators=(",", ":"),
        )

    def action_nothing(self) -> str:
        """
        Return a none event as a JSON string.
        """
        return json.dumps(
            {
                "type": "none",
            },
            separators=(",", ":"),
        )

    def action_tsumo_agari(self) -> str:
        return json.dumps(
            {
                "type": "hora",
                "actor": self.player_id,
                "target": self.target_actor,
                "pai": self.last_self_tsumo,
            },
            separators=(",", ":"),
        )

    def action_ron_agari(self) -> str:
        return json.dumps(
            {
                "type": "hora",
                "actor": self.player_id,
                "target": self.target_actor,
                "pai": self.last_kawa_tile,
            },
            separators=(",", ":"),
        )

    def action_riichi(self) -> str:
        return json.dumps(
            {
                "type": "reach",
                "actor": self.player_id,
            },
            separators=(",", ":"),
        )

    def action_ankan(self, consumed: list[str]) -> str:
        return json.dumps(
            {
                "type": "ankan",
                "actor": self.player_id,
                "consumed": consumed,  # 4 tiles to be consumed
            },
            separators=(",", ":"),
        )

    def action_kakan(self, pai: str) -> str:
        """
        Return a kakan event as a JSON string.

        Args:
            pai: Tile for kakan. Mjai-style tile string (like 'N' or '5mr')

        Example:
            >>> bot.action_kakan("5m")
            '{"type":"kakan","actor":0,"pai":"5m","consumed":["5mr","5m","5m"]}'

            >>> bot.action_kakan("5sr")
            '{"type":"kakan","actor":0,"pai":"5sr","consumed":["5s","5s","5s"]}'
        """
        consumed = [pai.replace("r", "")] * 3
        if pai[0] == "5" and not pai.endswith("r"):
            consumed[0] = consumed[0] + "r"

        return json.dumps(
            {
                "type": "kakan",
                "actor": self.player_id,
                "pai": pai,
                "consumed": consumed,  # 3 tiles to be consumed
            },
            separators=(",", ":"),
        )

    def action_daiminkan(self, consumed: list[str]) -> str:
        return json.dumps(
            {
                "type": "daiminkan",
                "actor": self.player_id,
                "target": self.target_actor,
                "pai": self.last_kawa_tile,
                "consumed": consumed,  # 3 tiles to be consumed
            },
            separators=(",", ":"),
        )

    def action_pon(self, consumed: list[str]) -> str:
        return json.dumps(
            {
                "type": "pon",
                "actor": self.player_id,
                "target": self.target_actor,
                "pai": self.last_kawa_tile,
                "consumed": consumed,
            },
            separators=(",", ":"),
        )

    def action_chi(self, consumed: list[str]) -> str:
        return json.dumps(
            {
                "type": "chi",
                "actor": self.player_id,
                "target": self.target_actor,
                "pai": self.last_kawa_tile,
                "consumed": consumed,
            },
            separators=(",", ":"),
        )

    def action_ryukyoku(self) -> str:
        return json.dumps(
            {"type": "ryukyoku"},
            separators=(",", ":"),
        )

    # ==========================================================
    # main

    def think(self) -> str:
        """
        Logic part of the bot.

        Override this method to implement your own logic!
        Default logic is tsumogiri: discard the last tile that the player drew.
        """
        if self.can_discard:
            tile_str = self.last_self_tsumo
            return self.action_discard(tile_str)
        else:
            return self.action_nothing()

    def react(self, input_str: str) -> str:
        try:
            events = json.loads(input_str)
            if len(events) == 0:
                raise ValueError("Empty events")
            for event in events:
                if event["type"] == "start_game":
                    self.__discard_events = []
                    self.__call_events = []
                    self.__dora_indicators = []
                if event["type"] == "start_kyoku" or event["type"] == "dora":
                    self.__dora_indicators.append(event["dora_marker"])
                if event["type"] == "dahai":
                    self.__discard_events.append(event)
                if event["type"] in [
                    "chi",
                    "pon",
                    "daiminkan",
                    "kakan",
                    "ankan",
                ]:
                    self.__call_events.append(event)

                self.action_candidate = self.player_state.update(
                    json.dumps(event)
                )

            # NOTE: Skip `think()` if the player's riichi is accepted and
            # no call actions are allowed.
            if (
                self.self_riichi_accepted
                and not (self.can_agari or self.can_kakan or self.can_ankan)
                and self.can_discard
            ):
                return self.action_discard(self.last_self_tsumo)

            resp = self.think()
            return resp

        except Exception as e:
            print(
                "===========================================", file=sys.stderr
            )
            print(f"Exception: {str(e)}", file=sys.stderr)
            print("Brief info:", file=sys.stderr)
            print(self.brief_info(), file=sys.stderr)
            print("", file=sys.stderr)

        return json.dumps({"type": "none"}, separators=(",", ":"))

    def start(self) -> None:
        while line := sys.stdin.readline():
            line = line.strip()
            resp = self.react(line)
            sys.stdout.write(resp + "\n")
            sys.stdout.flush()

    # ==========================================================
    # utils

    def is_yakuhai(self, tile: str) -> bool:
        return tile in [self.jikaze, self.bakaze] or self.is_dragon(tile)

    def is_dragon(self, tile: str) -> bool:
        return tile in ["P", "F", "C"]

    def find_pon_candidates(self) -> list[dict]:
        """

        Example:
            >>> bot.find_pon_candidates()
            [
                {
                    "consumed": ["5m", "5m"],
                    "current_shanten": 1,
                    "current_ukeire": 8,
                    "next_shanten": 0,
                    "next_ukeire": 6,
                    "discard_candidates": [
                        {
                            "discard_tile": "1m",
                            "improving_tiles": ["2m", "5m"],
                            "ukeire": 6,
                            "shanten": 0,
                        },
                        ...
                    ]
                },
                ...
            ]
        """
        current_shanten = calc_shanten(self.tehai)
        current_improving_tiles = self.find_improving_tiles()  # with 13 tiles
        current_ukeire = 0
        for current_improving in current_improving_tiles:
            current_ukeire = current_improving["ukeire"]

        pon_candidates = []
        if self.last_kawa_tile[0] == "5" and self.last_kawa_tile[1] != "z":
            if self.tehai_mjai.count(self.last_kawa_tile[:2]) >= 2:
                consumed = [self.last_kawa_tile[:2], self.last_kawa_tile[:2]]
                pon_candidates.append(
                    self.__new_pon_candidate(
                        consumed, current_shanten, current_ukeire
                    )
                )
            elif self.tehai_mjai.count(self.last_kawa_tile[:2] + "r") == 1:
                consumed = [
                    self.last_kawa_tile[:2],
                    self.last_kawa_tile[:2] + "r",
                ]
                pon_candidates.append(
                    self.__new_pon_candidate(
                        consumed, current_shanten, current_ukeire
                    )
                )
            return pon_candidates
        else:
            consumed = [
                self.last_kawa_tile,
                self.last_kawa_tile,
            ]
            pon_candidates.append(
                self.__new_pon_candidate(
                    consumed, current_shanten, current_ukeire
                )
            )

        return pon_candidates

    def __new_pon_candidate(
        self, consumed: list[str], current_shanten: int, current_ukeire: int
    ) -> dict:
        new_tehai_mjai = self.tehai_mjai.copy()
        new_tehai_mjai.remove(consumed[0])
        new_tehai_mjai.remove(consumed[1])
        new_call_str = fmt_call(
            {
                "type": "pon",
                "consumed": consumed,
                "pai": self.last_kawa_tile,
                "target": self.target_actor,
                "actor": self.player_id,
            },
            self.player_id,
        )

        tehai_str = convert_vec34_to_short(
            convert_mjai_to_vec34(new_tehai_mjai),
            self.player_state.akas_in_hand,
        )
        events = self.get_call_events(self.player_id)
        call_str = fmt_calls(events, self.player_id)
        tehai_str = tehai_str + call_str

        new_shanten = calc_shanten(tehai_str + new_call_str)

        # NOTE: aka is not distinguished in {discard,improving}_tiles
        candidates = find_improving_tiles(tehai_str + new_call_str)
        candidates = list(
            sorted(candidates, key=lambda x: len(x[1]), reverse=True)
        )
        candidates = [
            (
                vec34_index_to_mjai_tile(discard_tile_idx)
                if discard_tile_idx < 34
                else "",
                [
                    vec34_index_to_mjai_tile(tile_idx)
                    for tile_idx in improving_tile_indices
                ],
            )
            for discard_tile_idx, improving_tile_indices in candidates
        ]
        discard_candidates = []
        next_best_shanten = new_shanten
        next_best_ukeire = 0
        for discard_tile, improving_tiles in candidates:
            next_ukeire = 0
            for improving_tile in improving_tiles:
                next_ukeire += 4 - self.tiles_seen.get(improving_tile, 0)
            next_best_ukeire = max(next_best_ukeire, next_ukeire)
            discard_candidates.append(
                {
                    "discard_tile": discard_tile,
                    "improving_tiles": improving_tiles,
                    "ukeire": next_ukeire,
                    "shanten": new_shanten,
                }
            )

        return {
            "consumed": consumed,
            "current_shanten": current_shanten,
            "current_ukeire": current_ukeire,
            "discard_candidates": discard_candidates,
            "next_shanten": next_best_shanten,
            "next_ukeire": next_best_ukeire,
        }

    def find_chi_candidates(self) -> list[dict]:
        """

        Examples:
            >>> bot.find_chi_candidates()
        """
        current_shanten = calc_shanten(self.tehai)
        current_improving_tiles = self.find_improving_tiles()  # with 13 tiles
        current_ukeire = 0
        for current_improving in current_improving_tiles:
            current_ukeire = current_improving["ukeire"]

        chi_candidates = []

        color = self.last_kawa_tile[1]
        chi_num = int(self.last_kawa_tile[0])
        if (
            self.can_chi_high
            and f"{chi_num-2}{color}" in self.tehai_mjai
            and f"{chi_num-1}{color}" in self.tehai_mjai
        ):
            consumed = [f"{chi_num-2}{color}", f"{chi_num-1}{color}"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_high
            and f"{chi_num-2}{color}r" in self.tehai_mjai
            and f"{chi_num-1}{color}" in self.tehai_mjai
        ):
            consumed = [f"{chi_num-2}{color}r", f"{chi_num-1}{color}"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_high
            and f"{chi_num-2}{color}" in self.tehai_mjai
            and f"{chi_num-1}{color}r" in self.tehai_mjai
        ):
            consumed = [f"{chi_num-2}{color}", f"{chi_num-1}{color}r"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_low
            and f"{chi_num+1}{color}" in self.tehai_mjai
            and f"{chi_num+2}{color}" in self.tehai_mjai
        ):
            consumed = [f"{chi_num+1}{color}", f"{chi_num+2}{color}"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_low
            and f"{chi_num+1}{color}r" in self.tehai_mjai
            and f"{chi_num+2}{color}" in self.tehai_mjai
        ):
            consumed = [f"{chi_num+1}{color}r", f"{chi_num+2}{color}"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_low
            and f"{chi_num+1}{color}" in self.tehai_mjai
            and f"{chi_num+2}{color}r" in self.tehai_mjai
        ):
            consumed = [f"{chi_num+1}{color}", f"{chi_num+2}{color}r"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_mid
            and f"{chi_num-1}{color}" in self.tehai_mjai
            and f"{chi_num+1}{color}" in self.tehai_mjai
        ):
            consumed = [f"{chi_num-1}{color}", f"{chi_num+1}{color}"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_mid
            and f"{chi_num-1}{color}r" in self.tehai_mjai
            and f"{chi_num+1}{color}" in self.tehai_mjai
        ):
            consumed = [f"{chi_num-1}{color}r", f"{chi_num+1}{color}"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )
        if (
            self.can_chi_mid
            and f"{chi_num-1}{color}" in self.tehai_mjai
            and f"{chi_num+1}{color}r" in self.tehai_mjai
        ):
            consumed = [f"{chi_num-1}{color}", f"{chi_num+1}{color}r"]
            chi_candidates.append(
                self.__new_chi_candidate(
                    consumed,
                    current_shanten,
                    current_ukeire,
                )
            )

        return chi_candidates

    def __new_chi_candidate(
        self, consumed: list[str], current_shanten: int, current_ukeire: int
    ):
        new_tehai_mjai = self.tehai_mjai.copy()
        new_tehai_mjai.remove(consumed[0])
        new_tehai_mjai.remove(consumed[1])
        new_call_str = fmt_call(
            {
                "type": "chi",
                "consumed": consumed,
                "pai": self.last_kawa_tile,
                "target": self.target_actor,
                "actor": self.player_id,
            },
            self.player_id,
        )
        tehai_str = convert_vec34_to_short(
            convert_mjai_to_vec34(new_tehai_mjai),
            self.player_state.akas_in_hand,
        )
        events = self.get_call_events(self.player_id)
        call_str = fmt_calls(events, self.player_id)
        tehai_str = tehai_str + call_str

        new_shanten = calc_shanten(tehai_str + new_call_str)

        # NOTE: aka is not distinguished in {discard,improving}_tiles
        candidates = find_improving_tiles(tehai_str + new_call_str)
        candidates = list(
            sorted(candidates, key=lambda x: len(x[1]), reverse=True)
        )
        candidates = [
            (
                vec34_index_to_mjai_tile(discard_tile_idx)
                if discard_tile_idx < 34
                else "",
                [
                    vec34_index_to_mjai_tile(tile_idx)
                    for tile_idx in improving_tile_indices
                ],
            )
            for discard_tile_idx, improving_tile_indices in candidates
        ]
        discard_candidates = []
        next_best_shanten = new_shanten
        next_best_ukeire = 0
        for discard_tile, improving_tiles in candidates:
            next_ukeire = 0
            for improving_tile in improving_tiles:
                next_ukeire += 4 - self.tiles_seen.get(improving_tile, 0)
            next_best_ukeire = max(next_best_ukeire, next_ukeire)
            discard_candidates.append(
                {
                    "discard_tile": discard_tile,
                    "improving_tiles": improving_tiles,
                    "ukeire": next_ukeire,
                    "shanten": new_shanten,
                }
            )

        return {
            "consumed": consumed,
            "current_shanten": current_shanten,
            "current_ukeire": current_ukeire,
            "discard_candidates": discard_candidates,
            "next_shanten": next_best_shanten,
            "next_ukeire": next_best_ukeire,
        }

    def find_improving_tiles(self) -> list[dict]:
        """
        Find tiles that improve the hand.

        Example:
            >>> bot.tehai
            "1269m134p34579s56z"

            >>> ret = bot.find_improving_tiles()
            >>> len(ret)
            6

            >>> ret[0]
            {
                "discard_tile": "6m",
                "improving_tiles": ["3m", "9m", "1p", "2p", "4p", "5p", "8s", "P", "F"],  # noqa
                "ukeire": 9
            }
        """

        def _aka(tile: str) -> str:
            # Use aka if needeed
            if (
                tile == "5m"
                and self.tehai_vec34[4] == 1
                and self.akas_in_hand[0]
            ):
                return "5mr"
            if (
                tile == "5p"
                and self.tehai_vec34[4 + 9] == 1
                and self.akas_in_hand[1]
            ):
                return "5pr"
            if (
                tile == "5s"
                and self.tehai_vec34[4 + 18] == 1
                and self.akas_in_hand[2]
            ):
                return "5sr"
            return tile

        candidates = find_improving_tiles(self.tehai)
        candidates = list(
            sorted(candidates, key=lambda x: len(x[1]), reverse=True)
        )
        candidates = [
            {
                "discard_tile": (
                    _aka(vec34_index_to_mjai_tile(discard_tile_idx))
                    if discard_tile_idx < 34
                    else ""
                ),
                "improving_tiles": [
                    vec34_index_to_mjai_tile(tile_idx)
                    for tile_idx in improving_tile_indices
                ],
                "ukeire": sum(
                    [
                        4
                        - self.tiles_seen.get(
                            vec34_index_to_mjai_tile(tile_idx)[:2], 0
                        )
                        for tile_idx in improving_tile_indices
                    ]
                ),
            }
            for discard_tile_idx, improving_tile_indices in candidates
        ]
        candidates = list(
            sorted(candidates, key=lambda x: x["ukeire"], reverse=True)
        )
        return candidates
