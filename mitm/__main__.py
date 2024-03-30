import json
import threading
import asyncio
import time
from .common import start_proxy, start_xmlrpc_server, start_playwright, stop
from .config import config as mitm_config
from pathlib import Path
from playwright.sync_api import sync_playwright

def main():
    proxy_thread = threading.Thread(target=lambda: asyncio.run(start_proxy()))
    proxy_thread.start()

    xmlrpc_thread = threading.Thread(target=lambda: asyncio.run(start_xmlrpc_server()))
    xmlrpc_thread.start()

    try:
        if mitm_config.playwright.enable:
            start_playwright()
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        stop()
    pass


if __name__ == '__main__':
    main()

