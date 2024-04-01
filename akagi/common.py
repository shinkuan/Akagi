import os
import sys
import asyncio
import threading
import subprocess
from my_logger import both_logger
from .message_controller import MessageController

from mitm.common import start_proxy, start_xmlrpc_server, start_playwright, stop as mitm_stop
from mitm.config import config as mitm_config

mitm_exec = None
message_controller = None
message_controller_thread = None

# proxy_thread = None
# xmlrpc_thread = None
# playwright_thread = None

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

    return mitm_exec

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
    return None

# def start_mitm():
#     global proxy_thread, xmlrpc_thread, playwright_thread
#     both_logger.info("Starting mitm")
#     proxy_thread = threading.Thread(target=lambda: asyncio.run(start_proxy()))
#     xmlrpc_thread = threading.Thread(target=lambda: asyncio.run(start_proxy()))
#     proxy_thread.start()
#     xmlrpc_thread.start()
#     if mitm_config.playwright.enable:
#         playwright_thread = threading.Thread(target=start_playwright)
#         playwright_thread.start()

# def stop_mitm():
#     global mitm_exec, mitm_stop_flag, proxy_thread, xmlrpc_thread, playwright_thread
#     both_logger.info("Stopping mitm")
#     mitm_stop_flag = True
#     mitm_stop()
#     proxy_thread.join(timeout=3)
#     xmlrpc_thread.join(timeout=3)
#     if mitm_config.playwright.enable:
#         playwright_thread.join(timeout=3)

def start_message_controller():
    global message_controller, message_controller_thread

    if message_controller is not None:
        both_logger.info("Message Controller already running")
        return
    
    message_controller = MessageController()
    message_controller_thread = threading.Thread(target=message_controller.start)
    message_controller_thread.start()
    return message_controller

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
    return None