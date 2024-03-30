from __future__ import annotations

import sys
import os
os.environ["LOGURU_AUTOINIT"] = "False"

import json
import requests
import logging
from loguru import logger
from aliyun.log.logger_hanlder import QueuedLogHandler, LogFields

LOGGER_FORMAT = "<green>{time:YYYY-MM-DD HH:mm:ss.SSS}</green> | <level>{level: <8}</level> | <cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> - <level>{message}</level>"

### Loguru Logger

logger.add("akagi.log", level="DEBUG", format=LOGGER_FORMAT, filter=lambda record: "akagi" in record["extra"])
logger.add(sys.stderr, level="INFO", format=LOGGER_FORMAT, backtrace=True, diagnose=True, colorize=True, filter=lambda record: "stderr" in record["extra"])

akagi_logger = logger.bind(akagi=True)
stderr_logger = logger.bind(stderr=True)
both_logger = logger.bind(akagi=True, stderr=True)

### Game Result Logger

RECORD_LOG_FIELDS = set((LogFields.record_name, LogFields.level))
res = requests.get("https://cdn.jsdelivr.net/gh/shinkuan/RandomStuff/aliyun_log_handler_arg.json", allow_redirects=True)
json_data = json.loads(res.content)

handler = QueuedLogHandler(
    **json_data,
    fields=RECORD_LOG_FIELDS,
)
game_result_logger = logging.getLogger("game_result_log")
game_result_logger.setLevel(logging.INFO)
game_result_logger.addHandler(handler)

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
