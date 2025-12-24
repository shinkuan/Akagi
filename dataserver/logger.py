from __future__ import annotations

import loguru
from loguru import logger as main_logger
from datetime import datetime
from pathlib import Path

# Log to: "./logs/dataserver_<timestamp>.log"
log_path = Path().cwd() / "logs" / f"dataserver_{datetime.now().strftime('%Y%m%d_%H%M%S')}.log"
logger: loguru.Logger = main_logger.bind(module="dataserver")
main_logger.add(log_path, level="DEBUG", filter=lambda record: record["extra"].get("module") == "dataserver")
