import os
import sys
import platform
import pathlib
import subprocess
import eel
from my_logger import both_logger
from .eel_app import start_eel
from .common import start_message_controller, stop_message_controller, start_mitm, stop_mitm
from .config import config as akagi_config

if __name__ == "__main__":
    start_eel()

    while True:
        try:
            eel.sleep(0.1)
        except KeyboardInterrupt:
            stop_message_controller()
            stop_mitm()
            sys.exit(0)
        pass