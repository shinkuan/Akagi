"""
Tile Conversion:
- convert_mjai_to_vec34(mjai_tiles: list[str]) -> list[int]
- convert_vec34_to_short(tehai_vec34: list[int], akas_in_hand: list[bool] | None) -> str  # noqa
- vec34_index_to_short_tile(index: int) -> str
- vec34_index_to_mjai_tile(index: int) -> str

"""
from mjai.bot.consts import MJAI_VEC34_TILES
from mjai.mlibriichi.tools import calc_shanten  # noqa, type: ignore
from mjai.mlibriichi.tools import find_improving_tiles  # noqa, type: ignore


def convert_mjai_to_vec34(
    mjai_tiles: list[str],
) -> list[int]:
    """
    Convert mjai tiles to vec34 format

    Example:
        >>> convert_mjai_tiles_to_vec34(["1m", "2m", "3m", "5mr", "5m", "1p"])
        [1, 1, 1, 0, 2, 0, 0, 0, 0, 1, ...]

    """
    vec34_tiles = [0] * 34
    for mjai_tile in mjai_tiles:
        mjai_tile = mjai_tile.replace("r", "")
        idx = MJAI_VEC34_TILES.index(mjai_tile)
        vec34_tiles[idx] += 1
    return vec34_tiles


def convert_vec34_to_short(
    tehai_vec34: list[int], akas_in_hand: list[bool] | None = None
) -> str:
    """
    Convert tehai_vec34 to short format

    NOTE: Open shapes are ignored
    """
    ms, ps, ss, zis = [], [], [], []
    shortline_elems = []
    for tile_idx, tile_count in enumerate(tehai_vec34):
        if tile_idx == 4:
            if akas_in_hand and akas_in_hand[0]:
                ms.append(0)
            ms += [5] * (
                tile_count - 1
                if akas_in_hand and akas_in_hand[0]
                else tile_count
            )
        elif tile_idx == 4 + 9:
            if akas_in_hand and akas_in_hand[1]:
                ps.append(0)
            ps += [5] * (
                tile_count - 1
                if akas_in_hand and akas_in_hand[1]
                else tile_count
            )
        elif tile_idx == 4 + 18:
            if akas_in_hand and akas_in_hand[2]:
                ss.append(0)
            ss += [5] * (
                tile_count - 1
                if akas_in_hand and akas_in_hand[2]
                else tile_count
            )
        elif tile_idx < 9:
            ms += [tile_idx + 1] * tile_count
        elif tile_idx < 18:
            ps += [tile_idx - 9 + 1] * tile_count
        elif tile_idx < 27:
            ss += [tile_idx - 18 + 1] * tile_count
        else:
            zis += [tile_idx - 27 + 1] * tile_count
    if len(ms) > 0:
        shortline_elems.append("".join(map(str, ms)) + "m")
    if len(ps) > 0:
        shortline_elems.append("".join(map(str, ps)) + "p")
    if len(ss) > 0:
        shortline_elems.append("".join(map(str, ss)) + "s")
    if len(zis) > 0:
        shortline_elems.append("".join(map(str, zis)) + "z")

    return "".join(shortline_elems)


def vec34_index_to_short_tile(index: int) -> str:
    """
    Vec34 index to short format

    Example:
        >>> vec34_index_to_short_tile(0)
        "1m"
        >>> vec34_index_to_short_tile(33)
        "7z"
    """
    if index < 0 or index > 33:
        raise ValueError(f"index {index} is out of range [0, 33]")

    tiles = [
        "1m",
        "2m",
        "3m",
        "4m",
        "5m",
        "6m",
        "7m",
        "8m",
        "9m",
        "1p",
        "2p",
        "3p",
        "4p",
        "5p",
        "6p",
        "7p",
        "8p",
        "9p",
        "1s",
        "2s",
        "3s",
        "4s",
        "5s",
        "6s",
        "7s",
        "8s",
        "9s",
        "1z",
        "2z",
        "3z",
        "4z",
        "5z",
        "6z",
        "7z",
    ]
    return tiles[index]


def vec34_index_to_mjai_tile(index: int) -> str:
    """
    Vec34 index to mjai format

    Example:
        >>> vec34_index_to_mjai_tile(0)
        "1m"
        >>> vec34_index_to_mjai_tile(33)
        "C"
    """
    if index < 0 or index > 33:
        raise ValueError(f"index {index} is out of range [0, 33]")

    tiles = [
        "1m",
        "2m",
        "3m",
        "4m",
        "5m",
        "6m",
        "7m",
        "8m",
        "9m",
        "1p",
        "2p",
        "3p",
        "4p",
        "5p",
        "6p",
        "7p",
        "8p",
        "9p",
        "1s",
        "2s",
        "3s",
        "4s",
        "5s",
        "6s",
        "7s",
        "8s",
        "9s",
        "E",
        "S",
        "W",
        "N",
        "P",
        "F",
        "C",
    ]
    return tiles[index]


def fmt_calls(events: list[dict], player_id: int) -> str:
    calls = []
    kakan_calls = []

    for ev in events:
        call = fmt_call(ev, player_id)
        if ev["type"] == "kakan":
            kakan_calls.append(call)
        elif ev["type"] in ["chi", "pon", "daiminkan", "ankan"]:
            calls.append(call)

    for kakan_call in kakan_calls:
        tile = kakan_call[2:4]
        for i, call in enumerate(calls):
            if call[2:4] == tile and call[1] == "p":
                rel_pose = call[4]
                kakan_call[4] = rel_pose
                calls[i] = kakan_call
                break

    return "".join(calls)


def fmt_call(ev: dict, player_id: int) -> str:
    if ev["type"] == "pon":
        rel_pos = (ev["target"] - player_id + 4) % 4
        call_tiles = [
            __mjai_tile_to_short(ev["pai"]),
            __mjai_tile_to_short(ev["consumed"][0]),
            __mjai_tile_to_short(ev["consumed"][1]),
        ]
        return "(p{}{}{})".format(
            __deaka_short_tile(call_tiles[0]),
            rel_pos,
            "r"
            if any([__is_aka_short_tile(tile) for tile in call_tiles])
            else "",
        )
    elif ev["type"] == "chi":
        color = ev["pai"][1]
        consecutive_nums = "".join(
            list(
                sorted(
                    [
                        __mjai_tile_to_short(ev["pai"])[0],
                        __mjai_tile_to_short(ev["consumed"][0])[0],
                        __mjai_tile_to_short(ev["consumed"][1])[0],
                    ]
                )
            )
        )
        called_tile_idx = 0
        if consecutive_nums[0] == __mjai_tile_to_short(ev["pai"])[0]:
            called_tile_idx = 0
        elif consecutive_nums[1] == __mjai_tile_to_short(ev["pai"])[0]:
            called_tile_idx = 1
        else:
            called_tile_idx = 2
        return f"({consecutive_nums}{color}{called_tile_idx})"
    elif ev["type"] in ["daiminkan"]:
        rel_pos = (ev["target"] - player_id + 4) % 4
        call_tiles = [
            __mjai_tile_to_short(ev["pai"]),
            __mjai_tile_to_short(ev["consumed"][0]),
            __mjai_tile_to_short(ev["consumed"][1]),
            __mjai_tile_to_short(ev["consumed"][2]),
        ]
        return "(k{}{}{})".format(
            __deaka_short_tile(call_tiles[0]),
            rel_pos,
            "r"
            if any([__is_aka_short_tile(tile) for tile in call_tiles])
            else "",
        )
    elif ev["type"] in ["ankan"]:
        rel_pos = (ev["target"] - player_id + 4) % 4
        call_tiles = [
            __mjai_tile_to_short(ev["consumed"][0]),
            __mjai_tile_to_short(ev["consumed"][1]),
            __mjai_tile_to_short(ev["consumed"][2]),
            __mjai_tile_to_short(ev["consumed"][3]),
        ]
        return "(k{}{}{})".format(
            __deaka_short_tile(call_tiles[0]),
            rel_pos,
            "r"
            if any([__is_aka_short_tile(tile) for tile in call_tiles])
            else "",
        )
    elif ev["type"] == "kakan":
        rel_pos = 0  # NOTE: `rel_pose` will be replaced in `fmt_calls`

        call_tiles = [
            __mjai_tile_to_short(ev["pai"]),
            __mjai_tile_to_short(ev["consumed"][0]),
            __mjai_tile_to_short(ev["consumed"][1]),
            __mjai_tile_to_short(ev["consumed"][2]),
        ]
        return "(s{}{}{})".format(
            __deaka_short_tile(call_tiles[0]),
            rel_pos,
            "r"
            if any([__is_aka_short_tile(tile) for tile in call_tiles])
            else "",
        )
    else:
        return ""


def __is_aka_short_tile(tile: str) -> bool:
    return tile in ["0m", "0p", "0s"]


def __deaka_short_tile(tile: str) -> str:
    """
    Convert akadora to 5{m,p,s} tile

    Example:
        >>> __deaka_short_tile("0m")
        "5m"

        >>> __deaka_short_tile("1z")
        "1z"
    """
    return f"5{tile[1]}" if __is_aka_short_tile(tile) else tile


def __mjai_tile_to_short(tile: str) -> str:
    mapping = {
        "5mr": "0m",
        "5pr": "0s",
        "5sr": "0p",
        "E": "1z",
        "S": "2z",
        "W": "3z",
        "N": "4z",
        "P": "5z",
        "F": "6z",
        "C": "7z",
    }
    return mapping.get(tile, tile)
