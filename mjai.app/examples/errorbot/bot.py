import json
import sys

from mahjong.shanten import Shanten
from mahjong.tile import TilesConverter


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


def get_best_tile(tiles):
    tileset = set(tiles)
    tile_shanten_pairs = []
    for tile in tileset:
        tiles.remove(tile)
        count = tiles_to_shanten(tiles)
        tiles.append(tile)
        tile_shanten_pairs.append((count, tile))

    best_choice = sorted(tile_shanten_pairs)[0][1]
    return best_choice


class Bot:
    def __init__(self, actor_id):
        self.actor_id = actor_id
        self.tehais: list[str] = []
        self.index = 0

    def react(self, events: str) -> str:
        self.index += 1
        if self.index > 200:
            print("Error")
            raise ValueError("Error for debugging")

        events = json.loads(events)
        assert len(events) > 0

        for event in events:
            if event["type"] == "start_kyoku":  # type: ignore
                self.tehais = event["tehais"][self.actor_id]  # type: ignore
            elif event["type"] == "tsumo" and event["actor"] == self.actor_id:  # type: ignore
                self.tehais.append(event["pai"])  # type: ignore
            elif event["type"] == "dahai" and event["actor"] == self.actor_id:  # type: ignore
                self.tehais.remove(event["pai"])  # type: ignore

        # 最後のイベントの `type` によって分岐
        if events[-1]["type"] == "tsumo":  # type: ignore
            tsumo_tile = events[-1]["pai"]  # type: ignore
            tiles = self.tehais.copy()  # type: ignore
            best_choice = get_best_tile(tiles)

            return json.dumps({
                "type": "dahai",
                "pai": best_choice,
                "actor": self.actor_id,
                "tsumogiri": tsumo_tile == best_choice,
            }, separators=(",", ":"))
        else:
            # それ以外
            # 他人の打牌に対して常にアクションしない
            return json.dumps({
                "type": "none",
            }, separators=(",", ":"))


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
