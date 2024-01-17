import json
import sys

from mahjong.hand_calculating.hand_config import HandConfig
from mahjong.shanten import Shanten
from mahjong.tile import TilesConverter
from mahjong.hand_calculating.hand import HandCalculator
from mahjong.constants import EAST, WEST, SOUTH, NORTH


def tiles_to_136_array(tiles):
    man_strs, pin_strs, sou_strs, honors_strs = "", "", "", ""
    tile_map = {
        "E": "1z",
        "S": "2z",
        "W": "3z",
        "N": "4z",
        "P": "5z",
        "F": "6z",
        "C": "7z",
    }

    for tile_ in sorted(tiles):
        tile = tile_map.get(tile_, tile_)
        tile_type = tile[1]
        tile_num = tile[0] if tile[0] != '0' else '5'
        if tile_type == 'm':
            man_strs += tile_num
        elif tile_type == 'p':
            pin_strs += tile_num
        elif tile_type == 's':
            sou_strs += tile_num
        else:
            honors_strs += tile_num

    return TilesConverter.string_to_136_array(man=man_strs, pin=pin_strs, sou=sou_strs, honors=honors_strs)


def tiles_to_hand(tiles: list[str], win_tile: str, is_tsumo: bool, bakaze: str, kaze: str):
    calculator = HandCalculator()
    tiles_ = tiles_to_136_array(tiles)
    win_tile_ = tiles_to_136_array([win_tile])[0]

    # 平和判定のため場風と自風の情報が必要
    round_wind, player_wind = EAST, EAST
    if bakaze == "S":
        round_wind = SOUTH
    elif bakaze == "W":
        round_wind = WEST
    if kaze == "S":
        player_wind = SOUTH
    elif kaze == "W":
        player_wind = WEST
    elif kaze == "E":
        player_wind = EAST
    else:
        player_wind = NORTH

    result = calculator.estimate_hand_value(tiles_, win_tile_, config=HandConfig(is_tsumo=is_tsumo, player_wind=player_wind, round_wind=round_wind))
    return result


def tiles_to_shanten(tiles):
    shanten = Shanten()

    man_strs = ''
    pin_strs = ''
    sou_strs = ''
    honors_strs = ''

    tile_map = {
        "E": "1z",
        "S": "2z",
        "W": "3z",
        "N": "4z",
        "P": "5z",
        "F": "6z",
        "C": "7z",
    }

    for tile_ in sorted(tiles):
        tile = tile_map.get(tile_, tile_)
        tile_type = tile[1]
        tile_num = tile[0] if tile[0] != '0' else '5'
        if tile_type == 'm':
            man_strs += tile_num
        elif tile_type == 'p':
            pin_strs += tile_num
        elif tile_type == 's':
            sou_strs += tile_num
        else:
            honors_strs += tile_num

    tiles = TilesConverter.string_to_34_array(man=man_strs, pin=pin_strs, sou=sou_strs, honors=honors_strs)
    res = shanten.calculate_shanten(tiles)
    return res


def get_best_tile(tiles, other_riichi, other_anpai) -> str:
    tileset = set(tiles)

    # Shanten count
    shanten_scores = {}
    for tile in tileset:
        tiles.remove(tile)
        count = tiles_to_shanten(tiles)
        tiles.append(tile)
        shanten_scores[tile] = count
    # Eval num
    num_scores = {}
    for tile in tileset:
        if tile in ["E", "S", "W", "N", "F", "C", "P"]:
            num_scores[tile] = 4
        else:
            num_scores[tile] = abs(int(tile[0]) - 5)
    # Dup count
    dup_count = {}
    for tile in tiles:
        dup_count[tile] = dup_count.get(tile, 0) + 1
    # Neighbor count
    neighbor_count = {}
    for tile in tileset:
        tmp_count = 0
        if tile in ["E", "S", "W", "N", "F", "C", "P"]:
            pass
        elif tile[1] in ["p", "m", "s"]:
            tile_num = int(tile[0])
            tile_type = tile[1]

            if tile_num == 1:
                tmp_count = 1 if f"2{tile_type}" in tiles else 0
            elif tile_num == 9:
                tmp_count = 1 if f"8{tile_type}" in tiles else 0
            else:
                tmp_count = (
                    (1 if f"{tile_num - 1}{tile_type}" in tiles else 0) + (
                        1 if f"{tile_num + 1}{tile_type}" in tiles else 0))
        neighbor_count[tile] = tmp_count

    # Same color count
    color_count = {}
    for tile in tiles:
        if tile in ["E", "S", "W", "N", "F", "C", "P"]:
            color_count["z"] = color_count.get("z", 0) + 1
        else:
            color_count[tile[1]] = color_count.get(tile[1], 0) + 1

    def to_tiletype(s):
        if s in ["E", "S", "W", "N", "F", "C", "P"]:
            return "z"
        else:
            return s[1]

    # anpai
    anpai_score = {}
    for tile in tileset:
        for riichi_actor_id in other_riichi.keys():
            if other_riichi[riichi_actor_id]:
                anpai_score[tile] = anpai_score.get(tile, 0) + (
                    1 if tile in other_anpai[riichi_actor_id] else 0
                )

    tile_scores = [((
        shanten_scores[tile] * 100
        - anpai_score.get(tile, 0) * 10
        + num_scores[tile] * 8
        - dup_count[tile] * 10
        - color_count[to_tiletype(tile)]
        - neighbor_count[tile] * 3
    ), tile) for tile in tileset]
    # 以下のルールで捨て牌を選択する:
    # * 同じ色が多くなるように
    # * 19 など端牌から捨てる
    # * ドラは気持ち残す
    # * 2枚ない役牌は捨てる
    best_choice = sorted(tile_scores)[0][1]
    return best_choice


def get_best_shanten(tiles) -> int:
    tileset = set(tiles)
    tile_shanten_pairs = []
    for tile in tileset:
        tiles.remove(tile)
        count = tiles_to_shanten(tiles)
        tiles.append(tile)
        tile_shanten_pairs.append((count, tile))

    return sorted(tile_shanten_pairs)[0][0]


class Bot:
    def __init__(self, actor_id):
        self.actor_id = actor_id
        self.tehais: list[str] = []
        self.dahai: list[str] = []
        self.bakaze = "E"
        self.kaze = "E"
        self.last_tsumo = ""
        self.is_riichi = False
        self.left_tiles = 70
        self.other_riichi = {0: False, 1: False, 2: False, 3: False}
        self.other_anpai = {0: [], 1: [], 2: [], 3: []}

    def react(self, events: str) -> str:
        events = json.loads(events)
        assert len(events) > 0

        for event in events:
            if event["type"] == "start_kyoku":  # type: ignore
                self.tehais = event["tehais"][self.actor_id]  # type: ignore
                self.dahai = []
                self.bakaze = event["bakaze"]  # type: ignore
                self.is_riichi = False
                self.other_riichi = {0: False, 1: False, 2: False, 3: False}
                self.other_anpai = {0: [], 1: [], 2: [], 3: []}
                self.left_tiles = 70
                zero_indexed_kyoku = event["kyoku"] - 1  # type: ignore
                if self.bakaze == "S":
                    zero_indexed_kyoku = 4 + event["kyoku"] - 1  # type: ignore
                elif self.bakaze == "W":
                    zero_indexed_kyoku = 8 + event["kyoku"] - 1  # type: ignore
                wind_idx = (self.actor_id - zero_indexed_kyoku)
                self.kaze = ["E", "S", "W", "N"][wind_idx % 4]
            elif event["type"] == "tsumo":  # type: ignore
                if event["actor"] == self.actor_id:  # type: ignore
                    self.tehais.append(event["pai"])  # type: ignore
                self.left_tiles -= 1
            elif event["type"] == "reach" and event["actor"] != self.actor_id:
                self.other_riichi[self.actor_id] = True
            elif event["type"] == "dahai":
                if event["actor"] == self.actor_id:  # type: ignore
                    self.tehais.remove(event["pai"])  # type: ignore
                    self.dahai.append(event["pai"])  # type: ignore
                else:
                    self.other_anpai[self.actor_id].append(event["pai"])

                for riichi_actor_id in self.other_riichi.keys():
                    if self.other_riichi[riichi_actor_id]:
                        self.other_anpai[riichi_actor_id].append(event["pai"])

        # 最後のイベントの `type` によって分岐
        if events[-1]["type"] == "tsumo":  # type: ignore
            tsumo_tile = events[-1]["pai"]  # type: ignore
            self.last_tsumo = tsumo_tile

            # ツモできるならばする
            if self.check_hora(events[-1]):
                return json.dumps({
                    "type": "hora",
                    "actor": self.actor_id,
                    "target": self.actor_id,
                    "pai": events[-1]["pai"]  # type: ignore
                }, separators=(",", ":"))

            # リーチしているならばツモ切り
            if self.is_riichi:
                return json.dumps({
                    "type": "dahai",
                    "pai": tsumo_tile,
                    "actor": self.actor_id,
                    "tsumogiri": True,
                }, separators=(",", ":"))

            # 他がリーチしていなくてリーチできるならリーチする
            if self.can_riichi() and sum(self.other_riichi.values()) == 0:
                return json.dumps({
                    "type": "reach",
                    "actor": self.actor_id,
                })

            # 捨牌選択
            best_choice = get_best_tile(self.tehais.copy(), self.other_riichi, self.other_anpai)  # type: ignore
            return json.dumps({
                "type": "dahai",
                "pai": best_choice,
                "actor": self.actor_id,
                "tsumogiri": tsumo_tile == best_choice,
            }, separators=(",", ":"))

        elif events[-1]["type"] == "reach" and events[-1]["actor"] == self.actor_id:
            self.is_riichi = True
            best_choice = get_best_tile(self.tehais.copy(), self.other_riichi, self.other_anpai)  # type: ignore
            return json.dumps({
                "type": "dahai",
                "pai": best_choice,
                "actor": self.actor_id,
                "tsumogiri": self.last_tsumo == best_choice,
            }, separators=(",", ":"))
        else:
            # ロンできるならばする
            if (events[-1]["type"] == "dahai"
                and events[-1]["actor"] != self.actor_id
                and self.check_hora(events[-1], is_tsumo=False)
            ):
                return json.dumps({
                    "type": "hora",
                    "actor": self.actor_id,
                    "target": events[-1]["actor"],  # type: ignore
                    "pai": events[-1]["pai"]  # type: ignore
                }, separators=(",", ":"))

            # それ以外
            # 他人の打牌に対して常にアクションしない
            return json.dumps({
                "type": "none",
            }, separators=(",", ":"))

    def can_riichi(self) -> bool:
        # テンパイになるか判別
        return get_best_shanten(self.tehais.copy()) == 0 and self.left_tiles >= 4

    def check_hora(self, ev: dict, is_tsumo: bool = True) -> bool:
        # フリテンならロンしない
        if ev["pai"] in self.dahai:
            return False

        if is_tsumo:
            assert len(self.tehais) == 14
            hand = tiles_to_hand(self.tehais, ev["pai"], is_tsumo=is_tsumo, bakaze=self.bakaze, kaze=self.kaze)
        else:
            assert len(self.tehais) == 13
            hand = tiles_to_hand(self.tehais + [ev["pai"]], ev["pai"], is_tsumo=is_tsumo, bakaze=self.bakaze, kaze=self.kaze)
        return hand.cost is not None


def main():
    player_id = int(sys.argv[1])
    assert player_id in range(4)
    bot = Bot(player_id)

    while True:
        line = sys.stdin.readline().strip()
        resp = bot.react(line)
        sys.stdout.write(resp + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
