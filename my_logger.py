from __future__ import annotations

import os
os.environ["LOGURU_AUTOINIT"] = "False"

# import google.cloud.logging
# from google.auth._default import load_credentials_from_dict
# from google.cloud.logging_v2.handlers import CloudLoggingHandler

import json
import requests
import logging
import loguru
from loguru import logger

def my_sink(message: loguru.Message) -> None:
    record = message.record

logger.level("CLICK", no=10, icon="CLICK")
logger.add("akagi.log")
# logger.add(my_sink)


# def cloud_sink(message: loguru.Message) -> None:
#     record = message.record
#     _cloud_logger.log(message, resource={"type":"global", "labels":{"project_id":"akagi-327709"}})

# # Do not abuse this.
# res = requests.get("logging-key.json", allow_redirects=True)
# json_data = json.loads(res.content)
# credentials, project = load_credentials_from_dict(json_data)
# client = google.cloud.logging.Client(credentials=credentials)
# _cloud_logger = client.logger("akagi")

# logger.level("CLOUD", no=10, icon="CLOUD")
# cloud_logger_id = logger.add(cloud_sink, level="CLOUD", format="{message}", filter=lambda record: "cloud" in record["extra"])
# cloud_logger = logger.bind(cloud="google")

def game_result_log(mode_id: int, rank: int, score: int, model_hash: str) -> None:
    game_result = {
        "mode_id": mode_id,
        "rank": rank,
        "score": score,
        "model_hash": model_hash
    }
    with open("game_result.log", "a") as game_result_logger:
        game_result_logger.write(json.dumps(game_result) + "\n")
