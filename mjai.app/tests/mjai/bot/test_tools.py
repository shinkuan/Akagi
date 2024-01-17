import pytest
from mjai.bot.tools import (
    convert_vec34_to_short,
    find_improving_tiles,
    vec34_index_to_mjai_tile,
    vec34_index_to_short_tile,
)


@pytest.fixture
def tehai_vec34_random() -> list[int]:
    # 113479m4p33556s47z
    vec34_str = "201100101 000100000 002021000 0001001"
    return list(map(int, list(vec34_str.replace(" ", ""))))


@pytest.fixture
def tehai_vec34_ryanpeikou_chanta() -> list[int]:
    # 112233m112233p77z
    # Ryanpeikou (二盃口), Chanta (混全帯么九)
    # ref: https://riichi.wiki/Ryanpeikou
    # ref: https://riichi.wiki/Chanta
    vec34_str = "222000000 222000000 000000000 0000002"
    return list(map(int, list(vec34_str.replace(" ", ""))))


def test_find_improving_tiles(tehai_vec34_random):
    ret = find_improving_tiles("1269m134p34579s56z")
    assert len(ret) == 6

    # discard tiles
    assert vec34_index_to_short_tile(ret[0][0]) == "6m"
    assert vec34_index_to_short_tile(ret[1][0]) == "9m"
    assert vec34_index_to_short_tile(ret[2][0]) == "5z"
    assert vec34_index_to_short_tile(ret[3][0]) == "6z"
    assert vec34_index_to_short_tile(ret[4][0]) == "1p"
    assert vec34_index_to_short_tile(ret[5][0]) == "4p"
    assert vec34_index_to_mjai_tile(ret[3][0]) == "F"
    assert vec34_index_to_mjai_tile(ret[4][0]) == "1p"
    assert vec34_index_to_mjai_tile(ret[5][0]) == "4p"
    assert ret[0][1] == [2, 8, 9, 10, 12, 13, 25, 31, 32]
    assert [vec34_index_to_mjai_tile(x) for x in ret[0][1]] == [
        "3m",
        "9m",
        "1p",
        "2p",
        "4p",
        "5p",
        "8s",
        "P",
        "F",
    ]


def test_convert_tehai_vec34_as_short(
    tehai_vec34_ryanpeikou_chanta, tehai_vec34_random
):
    assert (
        convert_vec34_to_short(tehai_vec34_ryanpeikou_chanta)
        == "112233m112233p77z"
    )
    assert convert_vec34_to_short(tehai_vec34_random) == "113479m4p33556s47z"


def test_vec34_index_to_mjai_tile():
    assert vec34_index_to_mjai_tile(0) == "1m"
    assert vec34_index_to_mjai_tile(4) == "5m"
    assert vec34_index_to_mjai_tile(8) == "9m"
    assert vec34_index_to_mjai_tile(9) == "1p"
    assert vec34_index_to_mjai_tile(17) == "9p"
    assert vec34_index_to_mjai_tile(18) == "1s"
    assert vec34_index_to_mjai_tile(26) == "9s"
    assert vec34_index_to_mjai_tile(27) == "E"
    assert vec34_index_to_mjai_tile(28) == "S"
    assert vec34_index_to_mjai_tile(29) == "W"
    assert vec34_index_to_mjai_tile(30) == "N"
    assert vec34_index_to_mjai_tile(31) == "P"
    assert vec34_index_to_mjai_tile(32) == "F"
    assert vec34_index_to_mjai_tile(33) == "C"


def test_vec34_index_to_short_tile():
    assert vec34_index_to_short_tile(0) == "1m"
    assert vec34_index_to_short_tile(4) == "5m"
    assert vec34_index_to_short_tile(8) == "9m"
    assert vec34_index_to_short_tile(9) == "1p"
    assert vec34_index_to_short_tile(17) == "9p"
    assert vec34_index_to_short_tile(18) == "1s"
    assert vec34_index_to_short_tile(26) == "9s"
    assert vec34_index_to_short_tile(27) == "1z"
    assert vec34_index_to_short_tile(28) == "2z"
    assert vec34_index_to_short_tile(29) == "3z"
    assert vec34_index_to_short_tile(30) == "4z"
    assert vec34_index_to_short_tile(31) == "5z"
    assert vec34_index_to_short_tile(32) == "6z"
    assert vec34_index_to_short_tile(33) == "7z"
