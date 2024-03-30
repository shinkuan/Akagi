import time
from .config import config as mitm_config
from .common import page_evaluate_list
from mhm.config import MITMPROXY, HOST
from pathlib import Path
from playwright.sync_api import sync_playwright


class PlaywrightController:
    def __init__(self):
        self.width = mitm_config.playwright.width
        self.height = mitm_config.playwright.height
        self.playwright_context_manager = sync_playwright()
        self.playwright = self.playwright_context_manager.__enter__()
        self.chromium = self.playwright.chromium
        self.browser = self.chromium.launch_persistent_context(
            user_data_dir=Path(__file__).parent / 'data',
            headless=False,
            viewport={'width': mitm_config.playwright.width, 'height': mitm_config.playwright.height},
            proxy={"server": f"socks5://{MITMPROXY}"},
            ignore_default_args=['--enable-automation']
        )
        self.page = self.browser.new_page()
        self.page.goto(HOST)

    def close_browser(self):
        self.playwright_context_manager.__exit__()

    def evaluate(self, script: str):
        return self.page.evaluate(script)

    def get_page(self):
        return self.page
    
    def run(self):
        now = time.time()
        while True:
            if time.time() - now > 1:
                for script in page_evaluate_list:
                    self.evaluate(script)
                page_evaluate_list.clear()
                now = time.time()