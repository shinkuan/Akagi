import importlib
import json
import sys


def test_multiple_players_with_docker_wrapper():
    """九種九牌の流局後に適切に打牌できるか確認"""
    sys.path.append("./examples/shanten")
    mod = importlib.import_module("bot")

    player_id = 0
    bot = mod.Bot(player_id)
    resp = bot.react('[{"type":"start_game","id":' + str(player_id) + "}]")
    assert resp == '{"type":"none"}'

    resp = bot.react('[{"type":"end_kyoku"}]')
    assert resp == '{"type":"none"}'

    resp = bot.react(
        '[{"type":"start_kyoku","bakaze":"S","dora_marker":"1p","kyoku":2,'
        '"honba":2,"kyotaku":0,"oya":1,'
        '"scores":[800,61100,11300,26800],'
        '"tehais":['
        '["4p","4s","P","3p","1p","5s","2m","F","1m","7s","9m","6m","9s"],'
        '["?","?","?","?","?","?","?","?","?","?","?","?","?"],'
        '["?","?","?","?","?","?","?","?","?","?","?","?","?"],'
        '["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},'
        '{"type":"tsumo","actor":1,"pai":"?"},'
        '{"type":"dahai","actor":1,"pai":"F","tsumogiri":false},'
        '{"type":"tsumo","actor":2,"pai":"?"},'
        '{"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},'
        '{"type":"tsumo","actor":3,"pai":"?"},'
        '{"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},'
        '{"type":"tsumo","actor":0,"pai":"3s"}]'
    )
    resp_json = json.loads(resp)
    assert "type" in resp_json and resp_json["type"] == "dahai"
    assert "pai" in resp_json and resp_json["pai"] in [
        "4p",
        "4s",
        "P",
        "3p",
        "1p",
        "5s",
        "2m",
        "F",
        "1m",
        "7s",
        "9m",
        "6m",
        "9s",
        "3s",
    ]
