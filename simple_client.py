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
import mhm
from pathlib import Path
from optparse import OptionParser
from mitmproxy import proxy, options, ctx
from mitmproxy.tools.dump import DumpMaster
from xmlrpc.server import SimpleXMLRPCServer
from playwright.sync_api import sync_playwright, WebSocket
from playwright.sync_api._generated import Page

from liqi import LiqiProto
from majsoul2mjai import MajsoulBridge

activated_flows = [] # store all flow.id ([-1] is the recently opened)
activated_flows_instance = []
messages_dict = dict() # flow.id -> Queue[flow_msg]
stop = False
SHOW_LIQI = False

mhm.logger.setLevel("WARNING")

class ClientWebSocket:
    def __init__(self):
        self.liqi: dict[str, LiqiProto]={}
        self.bridge: dict[str, MajsoulBridge]={}
        pass

    # Websocket lifecycle
    def websocket_start(self, flow: mitmproxy.http.HTTPFlow):
        """

            A websocket connection has commenced.

        """
        # print('[new websocket]:',flow,flow.__dict__,dir(flow))
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows,messages_dict,activated_flows_instance
        
        activated_flows.append(flow.id)
        activated_flows_instance.append(flow)
        
        messages_dict[flow.id]=flow.websocket.messages

        self.liqi[flow.id] = LiqiProto()
        self.bridge[flow.id] = MajsoulBridge()

    def websocket_message(self, flow: mitmproxy.http.HTTPFlow):
        """

            Called when a WebSocket message is received from the client or

            server. The most recent message will be flow.messages[-1]. The

            message is user-modifiable. Currently there are two types of

            messages, corresponding to the BINARY and TEXT frame types.

        """
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        flow_msg = flow.websocket.messages[-1]
        
        parse_msg = self.liqi[flow.id].parse(flow_msg)
        mjai_msg = self.bridge[flow.id].input(parse_msg)
        if mjai_msg is not None:
            print('-'*65)
            print(mjai_msg)
        # composed_msg = self.bridge[flow.id].action(mjai_msg, self.liqi[flow.id])
        # if composed_msg is not None and AUTOPLAY:
        #     ws_composed_msg = mitmproxy.websocket.WebSocketMessage(2, True, composed_msg)
        #     flow.messages.append(ws_composed_msg)
        #     flow.inject_message(flow.server_conn, composed_msg)
        # print('='*65)
        if SHOW_LIQI:
            print(flow_msg.content)
            print(parse_msg)
            print('='*65)
            # if parse_msg['data']['name'] == 'ActionDiscardTile':
            #     print("Action is DiscardTile")
            #     if len(parse_msg['data']['data']['operation']['operationList'])>0:
            #         print(parse_msg['data']['data']['operation']['operationList'])
            #         print("OperationList is not empty")
            #         parse_msg['data']['data']['operation']['operationList'] = [
            #             {
            #                 'type': 3,
            #                 'combination': [
            #                     ['3m|4m', '4m|6m', '6m|7m']
            #                 ]
            #             },
            #             {
            #                 'type': 3,
            #                 'combination': [
            #                     ['0m|5m', '5m|5m']
            #                 ]
            #             },
            #             {
            #                 'type': 5,
            #                 'combination': [
            #                     ['0m|5m|5m']
            #                 ]
            #             },
            #             {
            #                 'type': 9
            #             }
            #         ]
            # print("Composing message...")
            # composed_msg = self.liqi[flow.id].compose(parse_msg)
            # flow.messages[-1].kill()
            # flow.messages.append(composed_msg)
            # flow.inject_message(flow.client_conn, composed_msg)
            # flow.messages[-1] = composed_msg
            # print('='*65)
            # print(parse_msg)
            # print('='*65)
            # print('='*65)
        # if not AUTOPLAY:
        #     print(mjai_msg)
        #     print('='*65)

        # packet = flow_msg.content
        # from_client = flow_msg.from_client
        # print("[" + ("Sended" if from_client else "Reveived") +
        #       "] from '"+flow.id+"': decode the packet here: %r…" % packet)

    def websocket_end(self, flow: mitmproxy.http.HTTPFlow):
        """

            A websocket connection has ended.

        """
        # print('[end websocket]:',flow,flow.__dict__,dir(flow))
        global activated_flows,messages_dict,activated_flows_instance
        activated_flows.remove(flow.id)
        activated_flows_instance.remove(flow)
        messages_dict.pop(flow.id)
        self.liqi.pop(flow.id)
        self.bridge.pop(flow.id)

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

async def start_proxy(host, port, enable_unlocker):
    opts = options.Options(listen_host=host, listen_port=port)

    master = DumpMaster(
        opts,
        with_termlog=False,
        with_dumper=False,
    )
    master.addons.add(ClientWebSocket())
    master.addons.add(ClientHTTP())
    if enable_unlocker:
        from mhm.addons import WebSocketAddon as Unlocker
        master.addons.add(Unlocker())
    await master.run()
    return master

if __name__ == '__main__':
    with open("settings.json", "r") as f:
        settings = json.load(f)
        mitm_port = settings["Port"]["MITM"]
        enable_unlocker = settings["Unlocker"]
        enable_helper = settings["Helper"]

    mitm_host="127.0.0.1"

    print("fetching resver...")
    mhm.fetch_resver()

    with open("mhmp.json", "r") as f:
        mhmp = json.load(f)
        mhmp["mitmdump"]["mode"] = [f"regular@{mitm_port}"]
        mhmp["hook"]["enable_aider"] = enable_helper
    with open("mhmp.json", "w") as f:
        json.dump(mhmp, f, indent=4)
    # Create and start the proxy server thread
    proxy_thread = threading.Thread(target=lambda: asyncio.run(start_proxy(mitm_host, mitm_port, enable_unlocker)))
    proxy_thread.start()

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        ctx.master.shutdown()
        exit(0)
