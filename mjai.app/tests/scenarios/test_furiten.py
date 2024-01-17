# flake8: noqa
import importlib
import json
import sys


def test_furiten():
    events = [
        '[{"type":"start_kyoku","bakaze":"E","dora_marker":"7p","kyoku":4,"honba":4,"kyotaku":0,"oya":3,"scores":[23300,18900,4200,53600],"tehais":[["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["1s","3m","2p","8s","6p","3m","8p","5s","7s","6m","3m","5pr","2m"]]},{"type":"tsumo","actor":3,"pai":"2s"}]',
        '[{"type":"dahai","actor":3,"pai":"2p","tsumogiri":false},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"S","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"6p","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"5m","tsumogiri":false},{"type":"tsumo","actor":3,"pai":"8m"}]',
        '[{"type":"dahai","actor":3,"pai":"5s","tsumogiri":false},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"F","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"2s","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"5p","tsumogiri":false},{"type":"tsumo","actor":3,"pai":"2p"}]',
        '[{"type":"dahai","actor":3,"pai":"2p","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"C","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"1m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"4s","tsumogiri":false},{"type":"tsumo","actor":3,"pai":"3p"}]',
        '[{"type":"dahai","actor":3,"pai":"2s","tsumogiri":false},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"1s","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"1m","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"6s","tsumogiri":true}]',
        '[{"type":"chi","actor":3,"target":2,"pai":"6s","consumed":["7s","8s"]}]',
        '[{"type":"dahai","actor":3,"pai":"1s","tsumogiri":false},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"9s","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"2m","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"6p","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"C"}]',
        '[{"type":"dahai","actor":3,"pai":"C","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"3s","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"7m","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"3p","tsumogiri":false},{"type":"tsumo","actor":3,"pai":"S"}]',
        '[{"type":"dahai","actor":3,"pai":"S","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"3p","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"1p","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"5p","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"7m"}]',
        '[{"type":"dahai","actor":3,"pai":"3p","tsumogiri":false},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"C","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"7m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"2s","tsumogiri":false},{"type":"tsumo","actor":3,"pai":"N"}]',
        '[{"type":"dahai","actor":3,"pai":"N","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"9p","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"4p","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"8m","tsumogiri":true}]',
        '[{"type":"tsumo","actor":3,"pai":"3s"}]',
        '[{"type":"dahai","actor":3,"pai":"3s","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"E","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"1p","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"N","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"2p"}]',
        '[{"type":"dahai","actor":3,"pai":"2p","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"2s","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"1p","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"4s","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"5sr"}]',
        '[{"type":"dahai","actor":3,"pai":"5sr","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"E","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"3s","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"6m","tsumogiri":true}]',
        '[{"type":"tsumo","actor":3,"pai":"1m"}]',
        '[{"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"9p","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"9m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"3m","tsumogiri":true}]',
        '[{"type":"tsumo","actor":3,"pai":"W"}]',
        '[{"type":"dahai","actor":3,"pai":"W","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"1p","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"6m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"7p","tsumogiri":true}]',
        '[{"type":"chi","actor":3,"target":2,"pai":"7p","consumed":["6p","8p"]}]',
        '[{"type":"dahai","actor":3,"pai":"5pr","tsumogiri":false},{"type":"tsumo","actor":0,"pai":"?"},{"type":"reach","actor":0},{"type":"dahai","actor":0,"pai":"7m","tsumogiri":false},{"type":"reach_accepted","actor":0},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"F","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"3p","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"P"}]',
        '[{"type":"dahai","actor":3,"pai":"P","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"2p","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"4m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"6m","tsumogiri":true}]',
    ]

    sys.path.append("./examples")
    mod = importlib.import_module("tsumogiri.bot")

    player_id = 3
    bot = mod.Bot(player_id)
    resp = bot.react('[{"type":"start_game","id":' + str(player_id) + "}]")
    assert resp == '{"type":"none"}'

    for idx, ev in enumerate(events):
        resp = bot.react(ev)
        assert len(resp) > 0
        if idx == len(events) - 1:
            assert (
                "type" in json.loads(resp)
                and json.loads(resp)["type"] != "hora"
            )
