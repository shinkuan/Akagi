import os
import json
import importlib
from .base.bot import Bot
from .logger import logger
from settings.settings import settings


class Controller(object):
    def __init__(self):
        self.available_bots: list[type[Bot]] = []
        self.available_bots_names: list[str] = []
        self.bot: Bot | None = None
        self.list_available_bots()
        self.bot: Bot = self.available_bots[0]() if self.available_bots else None
        self.choose_bot_name(settings.model)
        self.temp_mjai_msg: list[dict] = []
        self.starting_game: bool = False

    def list_available_bots(self) -> list[type[Bot]]:
        bots = []
        bots_names = []
        # Get the current package folder
        current_dir = os.path.dirname(__file__)
        for item in os.listdir(current_dir):
            if item.startswith("__"):
                continue
            if item == "base":
                continue
            dir_path = os.path.join(current_dir, item)
            # Check if folder and has a bot.py file
            if os.path.isdir(dir_path) and os.path.exists(os.path.join(dir_path, "bot.py")):
                try:
                    # Import the bot module using a relative import
                    module = importlib.import_module(f".{item}.bot", package=__package__)
                    if hasattr(module, "Bot"):
                        bot_class = getattr(module, "Bot")
                        bots.append(bot_class)
                        bots_names.append(item)
                except Exception as e:
                    # Logging error or handling exception
                    logger.error(f"Error importing bot from {item}: {e}")
        self.available_bots = bots
        self.available_bots_names = bots_names
        return bots

    def react(self, events: list[dict]) -> dict:
        if settings.auto_switch_model:
            for event in events:
                if event["type"] == "start_game":
                    self.starting_game = True
                    self.temp_mjai_msg = []
                    self.temp_mjai_msg.append(event)
                    continue
                if event["type"] == "start_kyoku" and self.starting_game:
                    self.starting_game = False
                    if (
                        event["scores"][0] == 35000 and
                        event["scores"][1] == 35000 and
                        event["scores"][2] == 35000 and
                        event["scores"][3] == 0
                    ):
                        if not self.choose_bot_name("mortal3p"):
                            logger.error("Failed to switch to mortal3p bot")
                    else:
                        if not self.choose_bot_name("mortal"):
                            logger.error("Failed to switch to mortal bot")
                    continue
                if self.starting_game:
                    logger.error("Event after start_game is not start_kyoku!")
                    logger.error(f"Event: {event}")
                    continue
            if self.starting_game:
                return {"type": "none", "can_act": False}
            events = self.temp_mjai_msg + events
            self.temp_mjai_msg = []
            ans = self.bot.react(json.dumps(events, separators=(",", ":")))
            return json.loads(ans)
        else:
            if not self.bot:
                logger.error("No bot available")
                return {"type": "none"}
            ans = self.bot.react(json.dumps(events, separators=(",", ":")))
            return json.loads(ans)

    def choose_bot_index(self, bot_index: int) -> bool:
        if 0 <= bot_index < len(self.available_bots):
            self.bot = self.available_bots[bot_index]()
            return True
        return False
    
    def choose_bot_name(self, bot_name: str) -> bool:
        if bot_name in self.available_bots_names:
            index = self.available_bots_names.index(bot_name)
            self.bot = self.available_bots[index]()
            return True
        return False
