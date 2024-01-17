from mjai.bot.riichibot import RiichiBot


def test_riichibot_without_1k_points():
    player = RiichiBot(player_id=0)
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
            [["2m","3m","1p","1p","4p","5p","6p","7p","9p","4s","5s","P","F"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"6s"}]
            """.replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"dahai","pai":"P","actor":0,"tsumogiri":false}'
    )
    # Can't call riichi because we don't have 1000 points
    assert (
        player.react(
            """
            [{"type":"dahai","pai":"P","actor":0,"tsumogiri":false},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"1s","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"1s","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"2s","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"1m"}]
            """.replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"dahai","pai":"F","actor":0,"tsumogiri":false}'
    )


def test_riichibot_with_1k_points():
    player = RiichiBot(player_id=0)
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
            "honba":2,"kyotaku":0,"oya":1,"scores":[1000,61100,11300,26800],
            "tehais":
            [["2m","3m","1p","1p","4p","5p","6p","7p","9p","4s","5s","P","F"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
            ["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"6s"}]
            """.replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"dahai","pai":"P","actor":0,"tsumogiri":false}'
    )
    # Can't call riichi because we don't have 1000 points
    assert (
        player.react(
            """
            [{"type":"dahai","pai":"P","actor":0,"tsumogiri":false},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"1s","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"1s","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"2s","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"1m"}]
            """.replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"reach","actor":0}'
    )
    assert (
        player.react(
            """
            [{"type":"reach","actor":0}]
            """.replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"dahai","pai":"F","actor":0,"tsumogiri":false}'
    )
    assert (
        player.react(
            """
            [{"type":"dahai","pai":"F","actor":0,"tsumogiri":false},
            {"type":"reach_accepted","actor":0},
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"8p","tsumogiri":false}
            ]""".replace(
                "\n", ""
            ).strip()
        )
        == '{"type":"hora","actor":0,"target":1,"pai":"8p"}'
    )
