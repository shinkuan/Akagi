import os
import sys
import gevent as gvt
import subprocess
from my_logger import both_logger
from .message_controller import MessageController

mitm_exec = None
message_controller = None
message_controller_thread = None

def start_mitm():
    both_logger.info("Starting mitm")
    global mitm_exec
    command = [sys.executable, "-m", "mitm"]

    if mitm_exec is not None:
        both_logger.info("mitm already running")
        return

    if sys.platform == "win32":
        # Windows特定代码
        mitm_exec = subprocess.Popen(command, creationflags=subprocess.CREATE_NEW_CONSOLE)
    else:
        # macOS和其他Unix-like系统
        mitm_exec = subprocess.Popen(command, preexec_fn=os.setsid)

def stop_mitm():
    both_logger.info("Stopping mitm")
    global mitm_exec

    if mitm_exec is None:
        both_logger.info("mitm is not running")
        return

    try:
        mitm_exec.kill()
    except Exception as e:
        both_logger.error(f"Error stopping mitm: {e}")
    
    mitm_exec = None

def start_message_controller():
    global message_controller, message_controller_thread

    if message_controller is not None:
        both_logger.info("Message Controller already running")
        return
    
    message_controller = MessageController()
    message_controller_thread = gvt.spawn(message_controller.start)

def stop_message_controller():
    global message_controller, message_controller_thread

    if message_controller is None:
        both_logger.info("Message Controller is not running")
        return

    message_controller.stop()
    message_controller_thread.join(timeout=3)
    both_logger.info("Message Controller stopped")
    message_controller = None
    message_controller_thread = None