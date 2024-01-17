import json

import pytest
from mjai.bot.rulebase import RulebaseBot


def fmt(events_str: str):
    return events_str.replace("\n", "").strip()


def test_custom_bot():
    player = RulebaseBot(player_id=0)

    assert (
        player.react(
            """[{"type":"start_game","names":["0","1","2","3"],"id":0}]"""
        )
        == '{"type":"none"}'
    )

    assert (
        player.react(
            fmt(
                """
                    [{"type":"start_kyoku","bakaze":"S","dora_marker":"1p","kyoku":2,
                    "honba":2,"kyotaku":0,"oya":1,"scores":[800,61100,11300,26800],
                    "tehais":
                    [["4p","4s","6s","3p","1p","5s","2m","F","1m","7s","9m","P","P"],
                    ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                    ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                    ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
                    {"type":"tsumo","actor":1,"pai":"?"},
                    {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
                    {"type":"tsumo","actor":2,"pai":"?"},
                    {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
                    {"type":"tsumo","actor":3,"pai":"?"},
                    {"type":"dahai","actor":3,"pai":"P","tsumogiri":true}
                    ]
                """
            )
        )
        == '{"type":"pon","actor":0,"target":3,"pai":"P","consumed":["P","P"]}'
    )
    assert player.last_kawa_tile == "P"
    assert player.last_self_tsumo == ""  # No tsumo events yet
    assert player.tehai == "129m134p4567s556z"

    assert (
        player.react(
            fmt(
                """
                [{"type":"pon","actor":0,"target":3,"pai":"P","consumed":["P","P"]}]
                """
            )
        )
        == '{"type":"dahai","pai":"F","actor":0,"tsumogiri":false}'
    )
    assert player.tehai == "129m134p4567s6z(p5z3)"

    assert (
        player.react(
            fmt(
                """
                [{"type":"dahai","actor":0,"pai":"F","tsumogiri":false},
                {"type":"tsumo","actor":1,"pai":"?"},
                {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
                {"type":"tsumo","actor":2,"pai":"?"},
                {"type":"dahai","actor":2,"pai":"1m","tsumogiri":true},
                {"type":"tsumo","actor":3,"pai":"?"},
                {"type":"dahai","actor":3,"pai":"2p","tsumogiri":true}
                ]"""
            )
        )
        == '{"type":"chi","actor":0,"target":3,"pai":"2p","consumed":["3p","4p"]}'  # noqa
    )
    assert player.tehai == "129m134p4567s(p5z3)"

    assert (
        player.react(
            fmt(
                """[{"type":"chi","actor":0,"target":3,"pai":"2p","consumed":["3p","4p"]}]"""  # noqa
            )
        )
        == '{"type":"dahai","pai":"1p","actor":0,"tsumogiri":false}'
    )
    assert player.tehai == "129m1p4567s(p5z3)(234p0)"


def test_error_case():
    bot = RulebaseBot(player_id=1)

    assert (
        bot.react(
            """[{"type":"start_game","names":["0","1","2","3"],"id":0}]"""
        )
        == '{"type":"none"}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"start_kyoku","bakaze":"E","dora_marker":"6s","kyoku":2,"honba":1,"kyotaku":0,"oya":1,"scores":[13000,45000,21000,21000],"tehais":[["?","?","?","?","?","?","?","?","?","?","?","?","?"],["2m","2p","7s","8p","F","1p","3m","7p","N","5mr","E","5s","W"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},{"type":"tsumo","actor":1,"pai":"2m"}]
        """
            )
        )
        == '{"type":"dahai","pai":"E","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"E","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"E","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"S","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"1s","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"3s"}]
        """
            )
        )
        == '{"type":"dahai","pai":"W","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"W","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"9m","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"5p","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"3p","tsumogiri":false}]
        """
            )
        )
        == '{"type":"none"}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"tsumo","actor":1,"pai":"9s"}]
"""
            )
        )
        == '{"type":"dahai","pai":"N","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"N","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"2p","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"4m","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"6m","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"1m"}]
"""
            )
        )
        == '{"type":"dahai","pai":"F","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"F","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"F","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"2m","tsumogiri":true}]
"""
            )
        )
        == '{"type":"none"}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"7m","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"6m"}]
"""
            )
        )
        == '{"type":"dahai","pai":"2m","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"2m","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"5pr","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"6m","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"8s","tsumogiri":true}]
"""
            )
        )
        == '{"type":"none"}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"tsumo","actor":1,"pai":"8m"}]
"""
            )
        )
        == '{"type":"dahai","pai":"8m","actor":1,"tsumogiri":true}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"8m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"6m","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"7s","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"E","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"9s"}]
"""
            )
        )
        == '{"type":"dahai","pai":"1p","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"1p","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"P","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"3p","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"4m","tsumogiri":true}]
"""
            )
        )
        == '{"type":"none"}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"tsumo","actor":1,"pai":"2s"}]
"""
            )
        )
        == '{"type":"dahai","pai":"2p","actor":1,"tsumogiri":false}'
    )

    assert (
        bot.react(
            fmt(
                """
[{"type":"dahai","actor":1,"pai":"2p","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"W","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"6s","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"F","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"9p"}]
"""
            )
        )
        == '{"type":"dahai","pai":"5s","actor":1,"tsumogiri":false}'
    )


def test_validation_tedashi_after_riichi():
    player = RulebaseBot(player_id=0)
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
            "honba":2,"kyotaku":0,"oya":1,"scores":[1800,61100,11300,26800],
            "tehais":
            [["1p","2p","3p","4p","5p","6p","7p","8p","9p","N","N","N","9s"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"S"}]""".replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"reach","actor":0}'
    )
    assert (
        player.react(
            """
            [{"type":"reach","actor":0}]""".replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"dahai","pai":"9s","actor":0,"tsumogiri":false}'
    )
    events = json.loads(
        """
            [{"type":"dahai","pai":"9s","actor":0,"tsumogiri":false},
            {"type":"reach_accepted","actor":0},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"S"}]""".replace(
            "\n", ""
        ).strip()
    )
    for ev in events:
        player.player_state.update(json.dumps(ev))

    resp = player.player_state.validate_reaction(
        '{"type":"hora","actor":0,"target":0,"pai":"S"}'
    )
    assert resp is None

    resp = player.player_state.validate_reaction(
        '{"type":"dahai","actor":0,"pai":"S","tsumogiri":true}'
    )
    assert resp is None

    with pytest.raises(RuntimeError):
        player.player_state.validate_reaction(
            '{"type":"dahai","actor":0,"pai":"N","tsumogiri":false}'
        )


def test_validation_tedashi_before_riichi_accepted():
    player = RulebaseBot(player_id=0)
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
            "honba":2,"kyotaku":0,"oya":1,"scores":[1800,61100,11300,26800],
            "tehais":
            [["1p","2p","3p","4p","5p","6p","7p","8p","9p","N","N","N","9s"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"S"}]""".replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"reach","actor":0}'
    )
    player.player_state.update('{"type":"reach","actor":0}')
    player.player_state.validate_reaction(
        '{"type":"dahai","pai":"S","actor":0,"tsumogiri":true}'
    )
    player.player_state.validate_reaction(
        '{"type":"dahai","pai":"9s","actor":0,"tsumogiri":false}'
    )
    assert True
