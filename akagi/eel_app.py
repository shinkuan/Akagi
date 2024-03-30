import os
import sys
import platform
import pathlib
import subprocess
import eel
from my_logger import both_logger
from .common import start_mitm as _start_mitm, stop_mitm as _stop_mitm, start_message_controller, stop_message_controller
from .config import config as akagi_config


@eel.expose
def start_mitm():
    _start_mitm()
    eel.sleep(1)
    start_message_controller()

@eel.expose
def stop_mitm():
    stop_message_controller()
    eel.sleep(1)
    _stop_mitm()

def start_eel():
    both_logger.info("Starting Eel")
    gui_path = pathlib.Path(__file__).parent.joinpath('gui')
    eel.init(gui_path)
    try:
        eel.start('main.html', mode='chrome',
                  size=(akagi_config.eel.width,akagi_config.eel.height), 
                  host=akagi_config.eel.host, port=akagi_config.eel.port,
                  block=False)
    except EnvironmentError:
        # If Chrome isn't found, fallback to Microsoft Edge on Win10 or greater
        if sys.platform in ['win32', 'win64'] and int(platform.release()) >= 10:
            eel.start('main.html', mode='edge', 
                      size=(akagi_config.eel.width,akagi_config.eel.height),
                      host=akagi_config.eel.host, port=akagi_config.eel.port,
                      block=False)
        else:
            raise
    both_logger.info("Eel started")
    pass
