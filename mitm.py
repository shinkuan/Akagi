import json
import threading
import asyncio
import signal
import time
import re
import mitmproxy.addonmanager
import mitmproxy.http
import mitmproxy.log
import mitmproxy.tcp
import mitmproxy.websocket
from pathlib import Path
from optparse import OptionParser
from mitmproxy import proxy, options, ctx
from mitmproxy.tools.dump import DumpMaster
from xmlrpc.server import SimpleXMLRPCServer
from playwright.sync_api import sync_playwright, WebSocket
from playwright.sync_api._generated import Page

activated_flows = [] # store all flow.id ([-1] is the recently opened)
messages_dict = dict() # flow.id -> Queue[flow_msg]
stop = False

class ClientWebSocket:

    def __init__(self):
        pass

    def websocket_start(self, flow: mitmproxy.http.HTTPFlow):
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows,messages_dict
        
        activated_flows.append(flow.id)
        messages_dict[flow.id]=[]

    def websocket_message(self, flow: mitmproxy.http.HTTPFlow):
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows,messages_dict

        messages_dict[flow.id].append(flow.websocket.messages[-1].content)

    def websocket_end(self, flow: mitmproxy.http.HTTPFlow):
        global activated_flows,messages_dict
        activated_flows.remove(flow.id)
        messages_dict.pop(flow.id)

class ClientHTTP:
    def __init__(self):
        pass

    def request(self, flow: mitmproxy.http.HTTPFlow):
        if flow.request.method == "GET":
            if re.search(r'^https://game\.maj\-soul\.(com|net)/[0-9]+/v[0-9\.]+\.w/code\.js$', flow.request.url):
                print("====== GET code.js ======"*3)
                print("====== GET code.js ======"*3)
                print("====== GET code.js ======"*3)
                flow.request.url = "http://cdn.jsdelivr.net/gh/Avenshy/majsoul_mod_plus/safe_code.js"
            elif re.search(r'^https://game\.mahjongsoul\.com/v[0-9\.]+\.w/code\.js$', flow.request.url):
                flow.request.url = "http://cdn.jsdelivr.net/gh/Avenshy/majsoul_mod_plus/safe_code.js"
            elif re.search(r'^https://mahjongsoul\.game\.yo-star\.com/v[0-9\.]+\.w/code\.js$', flow.request.url):
                flow.request.url = "http://cdn.jsdelivr.net/gh/Avenshy/majsoul_mod_plus/safe_code.js"

async def start_proxy(host, port, enable_unlocker, v10):
    opts = options.Options(listen_host=host, listen_port=port)

    master = DumpMaster(
        opts,
        with_termlog=False,
        with_dumper=False,
    )
    master.addons.add(ClientWebSocket())
    master.addons.add(ClientHTTP())
    if enable_unlocker:
        if v10:
            from unlocker_v10 import Unlocker
        else:
            from mhm.addons import WebSocketAddon as Unlocker
        master.addons.add(Unlocker())
    await master.run()
    return master

# Create a XMLRPC server
class LiqiServer:
    _rpc_methods_ = ['get_activated_flows', 'get_messages', 'reset_message_idx', 'page_clicker']
    def __init__(self, host, port):
        self.host = host
        self.port = port
        self.server = SimpleXMLRPCServer((self.host, self.port), allow_none=True, logRequests=False)
        for name in self._rpc_methods_:
            self.server.register_function(getattr(self, name))
        self.message_idx = dict() # flow.id -> int

    def get_activated_flows(self):
        return activated_flows
    
    def get_messages(self, flow_id):
        try:
            idx = self.message_idx[flow_id]
        except KeyError:
            self.message_idx[flow_id] = 0
            idx = 0
        if (flow_id not in activated_flows) or (len(messages_dict[flow_id])==0) or (self.message_idx[flow_id]>=len(messages_dict[flow_id])):
            return None
        msg = messages_dict[flow_id][idx]
        self.message_idx[flow_id] += 1
        return msg
    
    def reset_message_idx(self):
        for flow_id in activated_flows:
            self.message_idx[flow_id] = 0

    def page_clicker(self, xy):
        global click_list
        click_list.append(xy)
        return True

    def serve_forever(self):
        print(f"XMLRPC Server is running on {self.host}:{self.port}")
        self.server.serve_forever()

if __name__ == '__main__':
    with open("settings.json", "r") as f:
        settings = json.load(f)
        mitm_port = settings["Port"]["MITM"]
        rpc_port = settings["Port"]["XMLRPC"]
        enable_unlocker = settings["Unlocker"]
        v10 = settings["v10"]
        enable_helper = settings["Helper"]
        enable_playwright = settings["Playwright"]["enable"]
        playwright_width = settings["Playwright"]["width"]
        playwright_height = settings["Playwright"]["height"]
        scale = playwright_width / 16

    mitm_host="127.0.0.1"
    rpc_host="127.0.0.1"

    p = OptionParser()
    p.add_option("--mitm-host", default=None)
    p.add_option("--mitm-port", default=None)
    p.add_option("--rpc-host", default=None)
    p.add_option("--rpc-port", default=None)
    p.add_option("--unlocker", default=None)
    p.add_option("--v10", default=None)
    opts, arguments = p.parse_args()
    if opts.mitm_host is not None:
        mitm_host = opts.mitm_host
    if opts.mitm_port is not None:
        mitm_port = int(opts.mitm_port)
    if opts.rpc_host is not None:
        rpc_host = opts.rpc_host
    if opts.rpc_port is not None:
        rpc_port = int(opts.rpc_port)
    if opts.unlocker is not None:
        enable_unlocker = bool(opts.unlocker)
    if opts.v10 is not None:
        v10 = bool(opts.v10)

    with open("mhmp.json", "r") as f:
        mhmp = json.load(f)
        mhmp["mitmdump"]["mode"] = f"regular@{mitm_port}"
        mhmp["hook"]["enable_aider"] = enable_helper
    with open("mhmp.json", "w") as f:
        json.dump(mhmp, f, indent=4)
    # Create and start the proxy server thread
    proxy_thread = threading.Thread(target=lambda: asyncio.run(start_proxy(mitm_host, mitm_port, enable_unlocker, v10)))
    proxy_thread.start()

    liqiServer = LiqiServer(rpc_host, rpc_port)
    # Create and start the LiqiServer thread
    server_thread = threading.Thread(target=lambda: liqiServer.serve_forever())
    server_thread.start()

    page = None
    if enable_playwright:
        playwrightContextManager = sync_playwright()
        playwright = playwrightContextManager.__enter__()
        chromium = playwright.chromium
        browser = chromium.launch_persistent_context(
            user_data_dir=Path(__file__).parent / 'data',
            headless=False,
            viewport={'width': playwright_width, 'height': playwright_height},
            proxy={"server": f"http://localhost:{mitm_port}"},
            ignore_default_args=['--enable-automation']
        )

        print(f'startup browser success')

        page = browser.new_page()

        page.goto('https://game.maj-soul.com/1/')
        print(f'go to page success, url: {page.url}')

    click_list = []
    # On Ctrl+C, stop the other threads
    try:
        while True:
            if enable_playwright:
                if len(click_list) > 0:
                    xy = click_list.pop(0)
                    xy_scale = {"x":xy[0]*scale,"y":xy[1]*scale}
                    print(f"page_clicker: {xy_scale}")
                    page.mouse.move(x=xy_scale["x"], y=xy_scale["y"])
                    time.sleep(0.1)
                    page.mouse.click(x=xy_scale["x"], y=xy_scale["y"], delay=100)

                    # page.locator("#layaCanvas").click(position=xy_scale)
            time.sleep(1)  # main thread will block here
    except KeyboardInterrupt:
        # On Ctrl+C, stop the other threads
        if enable_playwright:
            playwrightContextManager.__exit__()
        ctx.master.shutdown()
        liqiServer.server.shutdown()
        exit(0)

# else:
with open("settings.json", "r") as f:
    settings = json.load(f)
    mitm_port = settings["Port"]["MITM"]
    rpc_port = settings["Port"]["XMLRPC"]
    enable_unlocker = settings["Unlocker"]
    v10 = settings["v10"]
if enable_unlocker:
    if v10:
        from unlocker_v10 import Unlocker
    else:
        from mhm.addons import WebSocketAddon as Unlocker
    addons = [ClientWebSocket(), Unlocker()]
else:
    addons = [ClientWebSocket()]
# start XMLRPC server
rpc_host="127.0.0.1"
liqiServer = LiqiServer(rpc_host, rpc_port)
server_thread = threading.Thread(target=lambda: liqiServer.serve_forever())
server_thread.start()
