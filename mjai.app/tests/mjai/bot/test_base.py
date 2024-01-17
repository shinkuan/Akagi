import json
from pathlib import Path

from mock import MagicMock

from mjai import Bot


def test_action_kakan_mocktest():
    bot = Bot(player_id=0)

    # Case1
    bot.player_state = MagicMock()
    bot.player_state.tehai = list(
        map(int, list("00000041100000021110000000000000"))
    )
    bot.player_state.akas_in_hand = [False, False, False]
    bot.player_state.last_self_tsumo.return_value = "7m"
    assert bot.tehai == "777789m7789p1s"
    kakan_json = bot.action_kakan("7m")
    assert json.loads(kakan_json)["type"] == "kakan"
    assert json.loads(kakan_json)["actor"] == 0
    assert json.loads(kakan_json)["pai"] == "7m"
    assert json.loads(kakan_json)["consumed"] == ["7m", "7m", "7m"]

    # Case2: tsumo tile is red
    bot.player_state = MagicMock()
    bot.player_state.tehai = list(
        map(int, list("00004001100000021110000000000000"))
    )
    bot.player_state.akas_in_hand = [True, False, False]
    bot.player_state.last_self_tsumo.return_value = "5mr"
    assert bot.tehai == "055589m7789p1s"
    kakan_json = bot.action_kakan("5mr")
    assert json.loads(kakan_json)["type"] == "kakan"
    assert json.loads(kakan_json)["actor"] == 0
    assert json.loads(kakan_json)["pai"] == "5mr"
    assert json.loads(kakan_json)["consumed"] == ["5m", "5m", "5m"]

    # Case3: tsumo tile is not red
    bot.player_state = MagicMock()
    bot.player_state.tehai = list(
        map(int, list("00004001100000021110000000000000"))
    )
    bot.player_state.akas_in_hand = [True, False, False]
    bot.player_state.last_self_tsumo.return_value = "5m"
    assert bot.tehai == "055589m7789p1s"
    kakan_json = bot.action_kakan("5m")
    assert json.loads(kakan_json)["type"] == "kakan"
    assert json.loads(kakan_json)["actor"] == 0
    assert json.loads(kakan_json)["pai"] == "5m"
    assert json.loads(kakan_json)["consumed"] == ["5mr", "5m", "5m"]


def test_action_kakan_validation():
    bot = Bot(player_id=0)
    logs = """
[{"type":"start_kyoku","bakaze":"S","dora_marker":"1p","kyoku":2,
"honba":2,"kyotaku":0,"oya":1,"scores":[800,61100,11300,26800],
"tehais":
[["5m","5m","8m","9m","7p","7p","8p","9p","1s","S","N","N","N"],
["?","?","?","?","?","?","?","?","?","?","?","?","?"],
["?","?","?","?","?","?","?","?","?","?","?","?","?"],
["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
{"type":"tsumo","actor":1,"pai":"?"},
{"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
{"type":"tsumo","actor":2,"pai":"?"},
{"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
{"type":"tsumo","actor":3,"pai":"?"},
{"type":"dahai","actor":3,"pai":"5m","tsumogiri":true},
{"type":"pon","actor":0,"target":3,"pai":"5m","consumed":["5m","5m"]},
{"type":"dahai","actor":0,"pai":"N","tsumogiri":false},
{"type":"tsumo","actor":1,"pai":"?"},
{"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
{"type":"tsumo","actor":2,"pai":"?"},
{"type":"dahai","actor":2,"pai":"P","tsumogiri":true},
{"type":"tsumo","actor":3,"pai":"?"},
{"type":"dahai","actor":3,"pai":"S","tsumogiri":true},
{"type":"tsumo","actor":0,"pai":"5mr"}]"""
    logs.replace("\n", "").strip()
    events = json.loads(logs)
    for ev in events:
        bot.player_state.update(json.dumps(ev))
    kakan_json = bot.action_kakan("5mr")
    assert bot.validate_reaction(kakan_json) is None
    assert json.loads(kakan_json)["type"] == "kakan"
    assert json.loads(kakan_json)["actor"] == 0
    assert json.loads(kakan_json)["pai"] == "5mr"
    assert json.loads(kakan_json)["consumed"] == ["5m", "5m", "5m"]


def test_discardable():
    bot = Bot(player_id=0)

    # Case1: kuikae example from https://riichi.wiki/Kuikae
    bot.player_state = MagicMock()
    bot.player_state.tehai = list(
        map(int, list("00000011111100000021110000000000000"))
    )
    bot.player_state.akas_in_hand = [False, False, False]
    bot.player_state.forbidden_tiles = [
        x == "1" for x in list("0000001000000000000000000000000000")
    ]
    # 7m is forbidden to discard
    assert bot.tehai == "789m123p11234s"
    assert set(bot.discardable_tiles) == set(
        [
            "8m",
            "9m",
            "1p",
            "2p",
            "3p",
            "1s",
            "2s",
            "3s",
            "4s",
        ]
    )


def test_discardable_tiles_riichi_declaration():
    bot = Bot(player_id=0)

    # Case1: after tenpai
    bot.player_state = MagicMock()
    bot.player_state.tehai = list(
        map(int, list("0000111001110000002111000000030000"))
    )
    bot.player_state.akas_in_hand = [False, False, False]
    assert bot.tehai == "567m123p11234s333z"
    assert bot.tehai_mjai.count("5m") == 1
    assert bot.tehai_mjai.count("5mr") == 0
    assert bot.tehai_mjai.count("W") == 3
    assert len(bot.discardable_tiles_riichi_declaration) == 11

    # Case2: before tenpai
    bot.player_state = MagicMock()
    bot.player_state.tehai = list(
        map(int, list("0000111001110000000111110000030000"))
    )
    bot.player_state.akas_in_hand = [False, False, False]
    assert bot.tehai == "567m123p23456s333z"
    assert bot.tehai_mjai.count("5m") == 1
    assert bot.tehai_mjai.count("5mr") == 0
    assert bot.tehai_mjai.count("W") == 3
    assert set(bot.discardable_tiles_riichi_declaration) == set(
        [
            "2s",
            "3s",
            "5s",
            "6s",
            "W",
        ]
    )


def test_tehai_mjai():
    bot = Bot(player_id=0)
    bot.player_state = MagicMock()

    # Case1
    bot.player_state.tehai = list(
        map(int, list("1111111110000000000000000000000000"))
    )
    bot.player_state.akas_in_hand = [True, False, False]
    assert bot.tehai == "123406789m"
    assert bot.tehai_mjai.count("5m") == 0
    assert bot.tehai_mjai.count("5mr") == 1

    # Case2
    bot.player_state.tehai = list(
        map(int, list("1111211110000000000000000000000000"))
    )
    bot.player_state.akas_in_hand = [True, False, False]
    assert bot.tehai == "1234056789m"
    assert bot.tehai_mjai.count("5m") == 1
    assert bot.tehai_mjai.count("5mr") == 1

    # Case3
    bot.player_state.tehai = list(
        map(int, list("1111311110000000000000000000000000"))
    )
    bot.player_state.akas_in_hand = [False, False, False]
    assert bot.tehai == "12345556789m"
    assert bot.tehai_mjai.count("5m") == 3
    assert bot.tehai_mjai.count("5mr") == 0

    # Case4
    bot.player_state.tehai = list(
        map(int, list("1111311110000100000000100000000000"))
    )
    bot.player_state.akas_in_hand = [False, False, False]
    assert bot.tehai == "12345556789m5p5s"
    assert bot.tehai_mjai.count("5m") == 3
    assert bot.tehai_mjai.count("5mr") == 0
    assert bot.tehai_mjai.count("5p") == 1
    assert bot.tehai_mjai.count("5pr") == 0
    assert bot.tehai_mjai.count("5s") == 1
    assert bot.tehai_mjai.count("5sr") == 0

    # Case5
    bot.player_state.tehai = list(
        map(int, list("1111311110000100000000100000000000"))
    )
    bot.player_state.akas_in_hand = [False, False, True]
    assert bot.tehai == "12345556789m5p0s"
    assert bot.tehai_mjai.count("5m") == 3
    assert bot.tehai_mjai.count("5mr") == 0
    assert bot.tehai_mjai.count("5p") == 1
    assert bot.tehai_mjai.count("5pr") == 0
    assert bot.tehai_mjai.count("5s") == 0
    assert bot.tehai_mjai.count("5sr") == 1

    # Case6
    bot.player_state.tehai = list(
        map(int, list("0111411110000100000000100000000000"))
    )
    bot.player_state.akas_in_hand = [True, False, True]
    assert bot.tehai == "23405556789m5p0s"
    assert bot.tehai_mjai.count("5m") == 3
    assert bot.tehai_mjai.count("5mr") == 1


def test_find_improving_tiles():
    player = Bot(player_id=0)
    assert (
        player.react(
            """[{"type":"start_game","names":["0","1","2","3"],"id":0}]"""
        )
        == '{"type":"none"}'
    )
    assert player.tehai == ""

    assert (
        player.react(
            json.dumps(
                [
                    {
                        "type": "start_kyoku",
                        "bakaze": "S",
                        "dora_marker": "1p",
                        "kyoku": 2,
                        "honba": 2,
                        "kyotaku": 0,
                        "oya": 1,
                        "scores": [800, 61100, 11300, 26800],
                        "tehais": [
                            "4p 4s P 3p 1p 5s 2m F 1m 7s 9m 6m 9s".split(),
                            "? ? ? ? ? ? ? ? ? ? ? ? ?".split(),
                            "? ? ? ? ? ? ? ? ? ? ? ? ?".split(),
                            "? ? ? ? ? ? ? ? ? ? ? ? ?".split(),
                        ],
                    },
                    {"type": "tsumo", "actor": 1, "pai": "?"},
                    {
                        "type": "dahai",
                        "actor": 1,
                        "pai": "F",
                        "tsumogiri": False,
                    },
                    {"type": "tsumo", "actor": 2, "pai": "?"},
                    {
                        "type": "dahai",
                        "actor": 2,
                        "pai": "3m",
                        "tsumogiri": True,
                    },
                    {"type": "tsumo", "actor": 3, "pai": "?"},
                    {
                        "type": "dahai",
                        "actor": 3,
                        "pai": "1m",
                        "tsumogiri": True,
                    },
                    {"type": "tsumo", "actor": 0, "pai": "3s"},
                ]
            )
        )
        == '{"type":"dahai","pai":"3s","actor":0,"tsumogiri":true}'
    )
    assert len(player.tehai_mjai) == 14


def test_tsumogiri_bot():
    bot = Bot(player_id=0)
    assert (
        bot.react(
            """[{"type":"start_game","names":["0","1","2","3"],"id":0}]"""
        )
        == '{"type":"none"}'
    )
    assert bot.tehai == ""

    assert (
        bot.react(
            """
            [{"type":"start_kyoku","bakaze":"S","dora_marker":"1p","kyoku":2,
            "honba":2,"kyotaku":1,"oya":1,"scores":[800,61100,11300,26800],
            "tehais":
            [["4p","4s","P","3p","1p","5s","2m","F","1m","7s","9m","6m","9s"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"3s"}]""".replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"dahai","pai":"3s","actor":0,"tsumogiri":true}'
    )
    assert bot.tiles_seen["F"] == 2  # actor1's tehai and actor0's dahai
    assert bot.tiles_seen["3m"] == 1  # actor3's dahai

    assert len(bot.tehai_mjai) == 14
    assert (
        bot.tehai == "1269m134p34579s56z"
    )  # NOTE: state just before last own reaction
    assert bot.is_oya is False
    assert bot.last_self_tsumo == "3s"
    assert bot.can_discard is True
    assert bot.honba == 2
    assert bot.kyoku == 2
    assert bot.kyotaku == 1
    assert bot.last_kawa_tile == "1m"

    assert bot.scores == [800, 61100, 11300, 26800]
    assert bot.jikaze == "N"
    assert bot.bakaze == "S"
    assert bot.player_state.at_furiten is False
    assert len(bot.tiles_seen) == 34
    assert len(bot.forbidden_tiles) == 34
    assert bot.tiles_seen["F"] == 2
    assert bot.tiles_seen["1p"] == 2
    assert (
        bot.discarded_tiles(0) == []
    )  # NOTE: state just before last own reaction

    assert bot.get_call_events(0) == []
    assert len(bot.dora_indicators) == 1
    assert bot.dora_indicators[0] == "1p"


class MyBot(Bot):
    def __init__(self, player_id: int = 0):
        super().__init__(player_id)

    def think(self) -> str:
        return self.action_nothing()


def test_custom_bot():
    player = MyBot(player_id=0)
    assert (
        player.react(
            """[{"type":"start_game","names":["0","1","2","3"],"id":0}]"""
        )
        == '{"type":"none"}'
    )
    assert player.tehai == ""

    assert (
        player.react(
            """
            [{"type":"start_kyoku","bakaze":"S","dora_marker":"1p","kyoku":2,
            "honba":2,"kyotaku":0,"oya":1,"scores":[800,61100,11300,26800],
            "tehais":
            [["4p","4s","P","3p","1p","5s","2m","F","1m","7s","9m","6m","9s"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"P"}]""".replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"none"}'
    )
    assert player.last_kawa_tile == "1m"
    assert player.last_self_tsumo == "P"
    assert player.tehai == "1269m134p4579s556z"


def test_tehai_mjai_with_akadora():
    lines = [
        line.strip()
        for line in Path("tests/mjai/bot/data_base_akadora.log").open("r")
    ]
    bot = Bot(player_id=1)
    for line in lines:
        bot.react(line)
    assert bot.tehai == "88m44p0788s113445z"
    assert "5sr" in bot.tehai_mjai
    assert "5s" not in bot.tehai_mjai
