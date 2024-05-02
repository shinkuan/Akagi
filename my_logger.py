from __future__ import annotations

import os
os.environ["LOGURU_AUTOINIT"] = "False"

import json
import requests
import logging
import loguru
from loguru import logger
from aliyun.log.logger_hanlder import QueuedLogHandler, LogFields

def my_sink(message: loguru.Message) -> None:
    record = message.record

game_result_logger = logging.getLogger("game_result_log")
game_result_logger.setLevel(logging.INFO)
try:
    logger.level("CLICK", no=10, icon="CLICK")
    logger.add("akagi.log")
    # logger.add(my_sink)

    RECORD_LOG_FIELDS = set((LogFields.record_name, LogFields.level))
    res = requests.get("https://cdn.jsdelivr.net/gh/shinkuan/RandomStuff/aliyun_log_handler_arg.json", allow_redirects=True)
    json_data = json.loads(res.content)

    handler = QueuedLogHandler(
        **json_data,
        fields=RECORD_LOG_FIELDS,
    )

    game_result_logger.addHandler(handler)
except Exception as e:
    logger.error(f"Failed to set up log handler: {e}")

def game_result_log(mode_id: int, rank: int, score: int, model_hash: str) -> None:
    if any((mode_id is None, rank is None, score is None, model_hash is None)):
        logger.error("Invalid game result")
        return
    game_result = {
        "mode_id": mode_id,
        "rank": rank,
        "score": score,
        "model_hash": model_hash
    }
    game_result_logger.info(game_result)
