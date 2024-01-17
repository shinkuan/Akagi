import json

from mjai import MjaiPlayerClient


class Verification:
    def __init__(self, player: MjaiPlayerClient) -> None:
        self.player: MjaiPlayerClient = player

    def __enter__(self):
        if self.player.proc is not None:
            self.player.delete_container()
        return self

    def __exit__(self, type_, value_, traceback_):
        if self.player.proc is not None:
            self.player.delete_container()

    def verify_start_game_response(self) -> bool:
        if self.player.proc is not None:
            self.player.delete_container()
        self.player.launch_container(0)

        input_str = '[{"type":"start_game","names":["0","1","2","3"],"id":0}]'  # noqa: E501
        response = self.player.react(input_str)

        assert response != "", f"response is empty. input=`{input_str}`"
        response_json = json.loads(response)
        assert "type" in response_json and "none" in response_json["type"]
        return True

    def verify_start_kyoku_response(self) -> bool:
        if self.player.proc is not None:
            self.player.delete_container()
        self.player.launch_container(0)

        input_str = '[{"type":"start_game","names":["0","1","2","3"],"id":0}]'  # noqa: E501
        response = self.player.react(input_str)

        assert response != "", f"response is empty. input=`{input_str}`"
        response_json = json.loads(response)
        assert "type" in response_json and "none" in response_json["type"]

        input_str = '[{"type":"start_kyoku","bakaze":"E","dora_marker":"2s","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["E","6p","9m","8m","C","2s","7m","S","6m","1m","S","3s","8m"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},{"type":"tsumo","actor":0,"pai":"1m"}]'  # noqa: E501
        response = self.player.react(input_str)

        assert response != "", f"response is empty. input=`{input_str}`"
        response_json = json.loads(response)
        assert "type" in response_json and "dahai" in response_json["type"]

        # input_str = '[{"type":"dahai","actor":0,"pai":"C","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"3m","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"1m","tsumogiri":false}]'  # noqa: E501
        # response = self.player.react(input_str)

        # assert response != "", f"response is empty. input=`{input_str}`"
        # response_json = json.loads(response)
        # assert "type" in response_json and "none" == response_json["type"]

        # input_str = '[{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"1m","tsumogiri":false}]'  # noqa: E501
        # response = self.player.react(input_str)

        # assert response != "", f"response is empty. input=`{input_str}`"
        # response_json = json.loads(response)
        # assert "type" in response_json and "none" == response_json["type"]

        # input_str = '[{"type":"tsumo","actor":0,"pai":"C"}]'  # noqa: E501
        # response = self.player.react(input_str)

        # assert response != "", f"response is empty. input=`{input_str}`"
        # response_json = json.loads(response)
        # assert "type" in response_json and "dahai" in response_json["type"]
        return True
