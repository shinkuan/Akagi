"""
Docker コンテナに submission file を配置して実行するためのラッパー (HTTP版)

1. submission file をマウントした Docker コンテナを起動
2. Docker コンテナに submission file のアーカイブを展開する
3. Docker コンテナ内で `python/mjai/http_server/server.py` を実行する
"""
import json
import shutil
import subprocess
import time
from pathlib import Path

import requests
from loguru import logger
from mjai.exceptions import EngineRuntimeError, TimeoutExpired
from .bot.bot import Bot
try:
    from .bot_3p.bot import Bot as Bot3p
    three_player = True
except Exception:
    three_player = False
    pass

# MEMORY_SIZE = "1G"
# CPU_CORES = "1"


class MjaiPlayerClient:
    def __init__(
        self,
    ) -> None:
        self.player_id = 0

        self.bot = None

    def launch_bot(self, player_id: int, is_3p = False) -> None:
        self.player_id = player_id
        if is_3p:
            if not three_player:
                raise NotImplementedError("3p bot is not implemented")
            self.bot = Bot3p(player_id)
        else:
            self.bot = Bot(player_id)

    def delete_bot(self):
        self.bot = None

    def restart_bot(self, player_id: int) -> None:
        self.delete_bot()
        self.launch_bot(player_id)

    def react(self, events: str) -> str:
        if self.bot is None:
            raise ValueError("bot is not running (3)")

        try:
            input_data = events.encode("utf8")

            logger.debug(f"{self.player_id} <- {input_data}")
            outs = self.bot.react(input_data)
            logger.debug(f"{self.player_id} -> {outs}")

            if (
                json.loads(events)[-1]["type"] == "tsumo"
                and json.loads(events)[-1]["actor"] == self.player_id
            ):
                json_data = {}
                try:
                    json_data = json.loads(outs)
                except Exception:
                    raise RuntimeError(f"JSON parser error: {outs}")

                if json_data["type"] == "none":
                    raise RuntimeError(f"invalid response: {str(outs)}")

        except requests.Timeout:
            raise TimeoutExpired(self.player_id)
        except Exception as e:
            raise EngineRuntimeError(f"RuntimeError: {str(e)}", self.player_id)

        return outs
