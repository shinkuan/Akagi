import json
import sys

from . import model
from .logger import logger

class Bot:
    def __init__(self):
        self.player_id: int = None
        self.model = None
        # ========== Online Server =========== #
        model.online_settings_init()
        # ==================================== #

    def react(self, events: str) -> str:
        """
        # How to implement this function

        One `start_game` event must be sent before any other events.
        Once the bot receives a `start_game` event, it will reinitialize itself and set the player_id.

        `start_game` event can be sent any time to reset the bot.
        `end_game` event can be sent to set model to None.

        :param events: JSON string of events
        :return: JSON string of action

        Example:
        ```
        bot = Bot()
        res = bot.react('[{"type":"start_game","names":["0","1","2","3"],"id":0}]')
        # res == '{"type":"none"}'

        events = str([
            {
                "type":"start_kyoku",
                "bakaze":"S",
                "dora_marker":"1p",
                "kyoku":2,"honba":2,
                "kyotaku":0,
                "oya":1,
                "scores":[800,61100,11300,26800],
                "tehais":[
                    ["4p","4s","P","3p","1p","5s","2m","F","1m","7s","9m","6m","9s"],
                    ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                    ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                    ["?","?","?","?","?","?","?","?","?","?","?","?","?"]
                ]
            },
            {"type":"tsumo","actor":1,"pai":"?"},
            {"type":"dahai","actor":1,"pai":"F","tsumogiri":false},
            {"type":"tsumo","actor":2,"pai":"?"},
            {"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},
            {"type":"tsumo","actor":3,"pai":"?"},
            {"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},
            {"type":"tsumo","actor":0,"pai":"3s"}
        ])

        res = bot.react(events)
        # res == '{"type":"dahai","pai":"3s","actor":0,"tsumogiri":true}'
        ...        
        res = bot.react('[{"type":"start_game","names":["0","1","2","3"],"id":3}]')
        # res == '{"type":"none"}'
        ...
        ```

        For more information, please refer to https://github.com/smly/mjai.app
        """
        try:
            events = json.loads(events)
        except json.JSONDecodeError as e:
            logger.error(f"Failed to parse events: {events}, {e}")
            return json.dumps({"type":"none"}, separators=(",", ":"))

        return_action = None
        for e in events:
            if e["type"] == "start_game":
                self.player_id = e["id"]
                self.model = model.load_model(self.player_id)
                continue
            if self.model is None or self.player_id is None:
                logger.error(f"Model is not loaded yet")
                continue
            if e["type"] == "end_game":
                self.player_id = None
                self.model = None
                continue
            return_action = self.model.react(json.dumps(e, separators=(",", ":")))

        if return_action is None:
            # Model didn't react to any event - no action needed
            # can_act=False tells the caller NOT to send a skip RPC
            raw_data = {"type":"none", "can_act": False}
            # ========== Online Server =========== #
            if model.ot_settings['online']:
                raw_data["meta"] = {"online": model.is_online}
            # ==================================== #
            return json.dumps(raw_data, separators=(",", ":"))
        else:
            # Model reacted - either a real action or explicit pass
            # can_act=True tells the caller this is a deliberate decision
            raw_data = json.loads(return_action)
            raw_data["can_act"] = True
            # ========== Online Server =========== #
            if model.ot_settings['online']:
                if "meta" not in raw_data:
                    raw_data["meta"] = {}
                raw_data["meta"]["online"] = model.is_online
            # ==================================== #
            return json.dumps(raw_data, separators=(",", ":"))

