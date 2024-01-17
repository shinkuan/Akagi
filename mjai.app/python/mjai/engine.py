import json


class BaseMjaiLogEngine:
    def __init__(self, name: str):
        self.engine_type = "mjai-log"
        self.name = name
        self.player_ids: list[int] = []

    def set_player_ids(self, player_ids: list[int]):
        self.player_ids = player_ids

    def react_batch(self, game_states):
        res = []
        for game_state in game_states:
            game_idx = game_state.game_index
            state = game_state.state
            events_json = game_state.events_json

            events = json.loads(events_json)
            assert events[0]["type"] == "start_kyoku"

            player_id = self.player_ids[game_idx]
            cans = state.last_cans
            if cans.can_discard:
                tile = state.last_self_tsumo()
                res.append(
                    json.dumps(
                        {
                            "type": "dahai",
                            "actor": player_id,
                            "pai": tile,
                            "tsumogiri": True,
                        }
                    )
                )
            else:
                res.append('{"type":"none"}')

        return res

    # They will be executed at specific events. They can be no-op but must be
    # defined.
    def start_game(self, game_idx: int):
        pass

    def end_kyoku(self, game_idx: int):
        pass

    def end_game(self, game_idx: int, scores: list[int]):
        pass


class DockerMjaiLogEngine(BaseMjaiLogEngine):
    def __init__(self, name: str, player):
        super().__init__(name)
        self.engine_type = "mjai-log"
        self.player = player
        self.player_ids: list[int] = []
        self.last_event_idx = 0
        self.player_id = 0

    def react_batch(self, game_states):
        events = []
        for game_state in game_states:
            self.player_id = self.player_ids[game_state.game_index]
            events_json = game_state.events_json
            events += json.loads(events_json)

        if self.last_event_idx > len(events):
            self.last_event_idx = 0

        event_buffer = []
        for ev in events[self.last_event_idx :]:  # noqa: E203
            # 自分以外の tsumo は "?" に置換
            if ev["type"] == "tsumo" and ev["actor"] != self.player_id:
                ev["pai"] = "?"
            # 自分以外の tehais は "?" に置換
            if ev["type"] == "start_kyoku":
                for player_id in range(4):
                    if self.player_id != player_id:
                        ev["tehais"][player_id] = ["?"] * 13

            event_buffer.append(ev)

        if len(event_buffer) == 0:
            self.last_event_idx = 0
            return ['{"type":"none"}']

        self.last_event_idx = len(events)

        res = self.player.react(
            json.dumps(event_buffer, indent=0, separators=(",", ":")).replace(
                "\n", ""
            )
        )

        json.loads(res)  # check json

        return [res]

    def start_game(self, game_idx: int) -> None:
        events = [
            {
                "type": "start_game",
                "names": ["0", "1", "2", "3"],
                "id": game_idx,
            }
        ]
        self.player.react(
            json.dumps(events, indent=0, separators=(",", ":")).replace(
                "\n", ""
            )
        )

    def end_kyoku(self, game_idx: int):
        events = [{"type": "end_kyoku"}]
        self.player.react(
            json.dumps(events, indent=0, separators=(",", ":")).replace(
                "\n", ""
            )
        )
        self.last_event_idx = 0

    def end_game(self, game_idx: int, scores: list[int]):
        events = [{"type": "end_game"}]
        self.player.react(
            json.dumps(events, indent=0, separators=(",", ":")).replace(
                "\n", ""
            )
        )


"""
* start_game と dahai で区切る

[{"type":"start_game","names":["player0","player1","player2","player3"]}]
[{"type":"start_kyoku","bakaze":"E","dora_marker":"2s","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["E","6p","9m","8m","C","2s","7m","S","6m","1m","S","3s","8m"],["5pr","2p","1p","C","3p","9p","9m","7s","2p","8s","3p","3m","3p"],["5p","7m","4p","6s","9s","8s","8p","P","8m","5p","7p","8s","1s"],["1s","3p","4p","7s","4m","6m","F","S","N","5m","7p","3m","1s"]]},{"type":"tsumo","actor":0,"pai":"1m"}]
[{"type":"dahai","actor":0,"pai":"1m","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"2s"},{"type":"dahai","actor":1,"pai":"2s","tsumogiri":true}]
[{"type":"tsumo","actor":2,"pai":"4s"},{"type":"dahai","actor":2,"pai":"4s","tsumogiri":true}]
[{"type":"tsumo","actor":3,"pai":"4m"},{"type":"dahai","actor":3,"pai":"4m","tsumogiri":true}]
[{"type":"tsumo","actor":0,"pai":"9p"}]
"""
