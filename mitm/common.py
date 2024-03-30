from mitmproxy import ctx
from mitmproxy.options import Options
from mitmproxy.tools.dump import DumpMaster

from mhm import console
from mhm.addon import GameAddon
from mhm.config import load_resource, config as mhm_config
from mhm.__main__ import create_hooks

from .config import config as mitm_config
from .client_websocket import ClientWebSocket, activated_flows, messages_dict
from .xmlrpc_server import XMLRPCServer, page_evaluate_list
from .playwright_controller import PlaywrightController

async def start_proxy():
    console.log(f"Debug: {mhm_config.base.debug}")
    console.log("Load Resource")
    with console.status("[magenta]Fetch LQC.LQBIN"):
        qbin_version, resger = load_resource()
    console.log(f"LQBin Version: [cyan3]{qbin_version}")
    console.log(f"> {len(resger.item_rows):0>3} items")
    console.log(f"> {len(resger.title_rows):0>3} titles")
    console.log(f"> {len(resger.character_rows):0>3} characters")

    console.log("Init Hooks")
    hooks = create_hooks(resger)
    for h in hooks:
        console.log(f"> [cyan3]{h.__class__.__name__}")
    methods = [h.run for h in hooks]

    master = DumpMaster(Options(**mhm_config.mitmdump.args), **mhm_config.mitmdump.dump)
    master.addons.add(GameAddon(methods))
    master.addons.add(ClientWebSocket())
    await master.run()
    return master

async def start_xmlrpc_server():
    global xmlrpc_server
    xmlrpc_server = XMLRPCServer()
    xmlrpc_server.serve_forever()
    return xmlrpc_server

def start_playwright():
    global playwright
    playwright = PlaywrightController()
    playwright.run()
    pass

def stop():
    global xmlrpc_server, playwright
    if mitm_config.playwright.enable:
        playwright.close_browser()
    xmlrpc_server.server.shutdown()
    ctx.master.shutdown()
    pass