import json
import random
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
from action import LOCATION
from tileUnicode import TILE_2_UNICODE

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

async def start_proxy(host, port, enable_unlocker):
    opts = options.Options(listen_host=host, listen_port=port)

    master = DumpMaster(
        opts,
        with_termlog=False,
        with_dumper=False,
    )
    master.addons.add(ClientWebSocket())

    import mhm
    print("fetching resver...")
    mhm.fetch_resver()
    from mhm.addons import WebSocketAddon as Unlocker
    master.addons.add(Unlocker())

    await master.run()
    return master

# Create a XMLRPC server
class LiqiServer:
    _rpc_methods_ = ['get_activated_flows', 'get_messages', 'reset_message_idx', 'page_clicker', 
                     'do_autohu', 'evaluate', 'start_overlay_action', 'stop_overlay_action', 'draw_weight',
                     'draw_top3', 'clear_top3', 'ping']
    def __init__(self, host, port):
        self.host = host
        self.port = port
        self.server = SimpleXMLRPCServer((self.host, self.port), allow_none=True, logRequests=False)
        for name in self._rpc_methods_:
            self.server.register_function(getattr(self, name))
        self.message_idx = dict() # flow.id -> int

        self._canvas_id = None
        self.page = None

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
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.click_list.append(xy)
        return True

    def do_autohu(self):
        global do_autohu
        if do_autohu:
            return self.evaluate("() => view.DesktopMgr.Inst.setAutoHule(true)")
        return False

    def evaluate(self, script):
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.evaluate_list.append(script)
        return True

    def start_overlay_action(self):
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.start_overlay_action()
        return True
    
    def stop_overlay_action(self):
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.stop_overlay_action()
        return True

    def draw_weight(self, weight):
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.draw_weight(weight)
        return True

    def draw_top3(self, top3):
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.draw_top3(top3)
        return True

    def clear_top3(self):
        global enable_playwright, playwright_controller
        if not enable_playwright:
            return False
        playwright_controller.clear_top3()
        return True

    def ping(self):
        return True

    def serve_forever(self):
        print(f"XMLRPC Server is running on {self.host}:{self.port}")
        self.server.serve_forever()


ACTION_2_UNICODE = {
    'chi': '吃',
    'chi_low': '吃',
    'chi_mid': '吃',
    'chi_high': '吃',
    'pon': '碰',
    'kan': '槓',
    'daiminkan': '槓',
    'ankan': '槓',
    'kakan': '槓',
    'hora': '和',
    'zimo': '和',
    'ryukyoku': '流局',
    'reach': '立直',
    'nukidora': '拔北',
    'none': '跳過',
}


class PlaywrightController:
    def __init__(self, width, height, mitm_port, majsoul_url):
        self.playwrightContextManager = sync_playwright()
        self.playwright = self.playwrightContextManager.__enter__()
        self.chromium = self.playwright.chromium
        self.browser = self.chromium.launch_persistent_context(
            user_data_dir=Path(__file__).parent / 'data',
            headless=False,
            viewport={'width': width, 'height': height},
            proxy={"server": f"http://localhost:{mitm_port}"},
            ignore_default_args=['--enable-automation']
        )
        self.width = width
        self.height = height
        self._canvas_id = None
        self._top_3_canvas_id = None
        self._recommendation_canvas_id = None
        self.scale = width / 16
        self.tiles_location_scaled = [[x*self.scale, y*self.scale] for x, y in LOCATION["tiles"]]
        self.tsumo_space_scaled = int(LOCATION["tsumo_space"] * self.scale)

        print(f'startup browser success')

        self.page = self.browser.new_page()

        self.page.goto(majsoul_url)
        # self.page.goto('https://game.mahjongsoul.com/')
        print(f'go to page success, url: {self.page.url}')

        self._t3c_width = self.width * 0.16
        self._t3c_height = self.height * 0.14
        self._t3c_x = self.width - self._t3c_width
        self._t3c_y = self.height - self._t3c_height

        self.click_list = []
        self.evaluate_list = []

    def run(self):
        while True:
            if len(self.click_list) > 0:
                xy = self.click_list.pop(0)
                xy_scale = {"x":xy[0]*self.scale,"y":xy[1]*self.scale}
                print(f"page_clicker: {xy_scale}")
                self.page.mouse.move(x=xy_scale["x"], y=xy_scale["y"])
                time.sleep(0.1)
                self.page.mouse.click(x=xy_scale["x"], y=xy_scale["y"], delay=100)
            if self.evaluate_list:
                for _ in range(len(self.evaluate_list)):
                    script = self.evaluate_list.pop(0)
                    # print(f"evaluate: {script}")
                    self.page.evaluate(script)
            time.sleep(0.1)  # main thread will block here

    def start_overlay_action(self):
        """ Display overlay on page. Will ignore if already exist, or page is None"""
        # random 8-byte alpha-numeric string
        
        if self._canvas_id:     # if exist, skip and return
            return
        if self._top_3_canvas_id:
            return
        if self.page is None:
            return
        # LOGGER.debug("browser Start overlay")
        random_str = ''.join(random.choices('0123456789abcdef', k=8))
        self._canvas_id = "myCanvas"
        random_str = ''.join(random.choices('0123456789abcdef', k=8))
        self._top3_bg_canvas_id = "top3bgCanvas"
        random_str = ''.join(random.choices('0123456789abcdef', k=8))
        self._top_3_canvas_id = "top3Canvas"
        js_code = f"""(async () => {{
            // Create a canvas element and add it to the document body
            const canvas = document.createElement('canvas');
            canvas.id = '{self._canvas_id}';
            canvas.width = {self.width}; // Width of the canvas
            canvas.height = {self.height}; // Height of the canvas
            
            // Set styles to ensure the canvas is on top
            canvas.style.position = 'fixed'; // Use 'fixed' or 'absolute' positioning
            canvas.style.left = '0'; // Position at the top-left corner of the viewport
            canvas.style.top = '0';
            canvas.style.zIndex = '9999997'; // High z-index to ensure it is on top
            canvas.style.pointerEvents = 'none'; // Make the canvas click-through
            document.body.appendChild(canvas);

            // Create a canvas element and add it to the document body
            const top3bgCanvas = document.createElement('canvas');
            top3bgCanvas.id = '{self._top3_bg_canvas_id}';
            top3bgCanvas.width = {self.width}; // Width of the canvas
            top3bgCanvas.height = {self.height}; // Height of the canvas
            top3bgCanvas.style.position = 'fixed'; // Use 'fixed' or 'absolute' positioning
            top3bgCanvas.style.zIndex = '9999998'; // High z-index to ensure it is on top
            top3bgCanvas.style.top = '0'; // Position at the top-left corner of the viewport
            top3bgCanvas.style.left = '0';
            top3bgCanvas.style.pointerEvents = 'none'; // Make the canvas click-through
            document.body.appendChild(top3bgCanvas);

            const top3bgCtx = top3bgCanvas.getContext('2d');
            top3bgCtx.fillStyle = 'rgba(0, 0, 0, 0.5)';
            top3bgCtx.fillRect({self._t3c_x}, {self._t3c_y}, {self._t3c_width}, {self._t3c_height});

            // Create a canvas element and add it to the document body
            const top3Canvas = document.createElement('canvas');
            top3Canvas.id = '{self._top_3_canvas_id}';
            top3Canvas.width = {self.width}; // Width of the canvas
            top3Canvas.height = {self.height}; // Height of the canvas
            top3Canvas.style.position = 'fixed'; // Use 'fixed' or 'absolute' positioning
            top3Canvas.style.zIndex = '9999999'; // High z-index to ensure it is on top
            top3Canvas.style.top = '0'; // Position at the top-left corner of the viewport
            top3Canvas.style.left = '0';
            top3Canvas.style.pointerEvents = 'none'; // Make the canvas click-through
            document.body.appendChild(top3Canvas);
        }})();"""

        self.evaluate_list.append(js_code)
        
    def stop_overlay_action(self):
        """ Remove overlay from page. Will ignore if page is None, or overlay not on"""
        
        if (self._canvas_id is None) or (self.page is None):
            return
        # LOGGER.debug("browser Stop overlay")
        js_code = f"""(() => {{
            const canvas = document.getElementById('{self._canvas_id}');
            if (canvas) {{
                canvas.remove();
            }}
            const top3_canvas = document.getElementById('{self._top_3_canvas_id}');
            if (top3_canvas) {{
                top3_canvas.remove();
            }}
            const top3_bg_canvas = document.getElementById('{self._top3_bg_canvas_id}');
            if (top3_bg_canvas) {{
                top3_bg_canvas.remove();
            }}
            }})()"""
        self.evaluate_list.append(js_code)
        self._canvas_id = None
        self._top_3_canvas_id = None
        self._top3_bg_canvas_id = None
        self._botleft_text = None

    def draw_weight(self, weight):
        if (self._canvas_id is None) or (self.page is None):
            return
        bar_width = int(0.2 * self.scale)
        max_bar_height = int(1 * self.scale)
        tile_half_height = int(0.675 * self.scale)
        js_code = f"""(() => {{
            const canvas = document.getElementById('{self._canvas_id}');
            if (!canvas || !canvas.getContext) {{
                return;
            }}
            const ctx = canvas.getContext('2d');

            // const TILES_LOCATION = [[x, y], ...]; // 示例坐标
            // const weight = [0.5, 0.8, ...]; // 示例权重列表
            TILES_LOCATION = {self.tiles_location_scaled}
            weight = {weight}
            scale = {self.scale}
            
            // 定义长条的宽度和最大长度
            const BAR_WIDTH = {bar_width};
            const MAX_BAR_HEIGHT = {max_bar_height};
            const TSUMO_SPACE = {self.tsumo_space_scaled};

            ctx.clearRect(0, TILES_LOCATION[0][1] - MAX_BAR_HEIGHT - {tile_half_height} - 4, canvas.width, MAX_BAR_HEIGHT + 8);

            // 绘制每个长条
            for (let i = 0; i < weight.length; i++) {{
                const w = weight[i];

                // Tsumohai has a special location
                if (w == -1.0 || i == 13) {{
                    if (weight[13] == -1.0) {{
                        break;
                    }}
                    const barHeight2 = weight[13] * MAX_BAR_HEIGHT;
                    const [x2, y2] = TILES_LOCATION[i];
                    const barX2 = x2 - BAR_WIDTH / 2 + TSUMO_SPACE;
                    const barY2 = y2 - barHeight2 - {tile_half_height};
                    ctx.fillStyle = 'hsl(' + weight[13]*180 + ', 100%, 50%)';
                    ctx.fillRect(barX2, barY2, BAR_WIDTH, barHeight2);
                    break;                    
                }}
                
                // 根据weight值计算长条的实际高度
                const barHeight = w * MAX_BAR_HEIGHT;

                // 获取该长条的中心底部位置
                const [x, y] = TILES_LOCATION[i];

                // 计算长条的左上角坐标，以便以中心底部位置为基准绘制
                const barX = x - BAR_WIDTH / 2;
                const barY = y - barHeight - {tile_half_height};

                // 设置绘制样式
                ctx.fillStyle = 'hsl(' + w*180 + ', 100%, 50%)'; // 设置长条颜色

                // 绘制长条
                ctx.fillRect(barX, barY, BAR_WIDTH, barHeight);
            }}
        }})();"""
        self.evaluate_list.append(js_code)

    def draw_top3(self, top3):
        if (self._canvas_id is None) or (self.page is None):
            return
        top_idx, action, tile, consume1, consume2, weight = top3
        if action in ACTION_2_UNICODE.keys():
            rcmd_msg = f"{ACTION_2_UNICODE[action]}{TILE_2_UNICODE[tile]} {TILE_2_UNICODE[consume1]}{TILE_2_UNICODE[consume2]}"
        else:
            rcmd_msg = f"切{TILE_2_UNICODE[action]}"

        js_code = f"""(async () => {{
            const canvas = document.getElementById('{self._top_3_canvas_id}');
            if (!canvas || !canvas.getContext) {{
                return;
            }}
            const ctx = canvas.getContext('2d');

            ctx.fillStyle = "#FFFFFF";
            ctx.font = 'bold {int(self._t3c_height/4)}px Arial';
            ctx.textAlign = 'left';
            ctx.fillText('{rcmd_msg}', {int(self._t3c_x)}, {int(self._t3c_height/4 + self._t3c_height*top_idx/3 + self._t3c_y)});            
            ctx.textAlign = 'right';
            ctx.fillText('{weight}', {int(self._t3c_width + self._t3c_x)}, {int(self._t3c_height/4 + self._t3c_height*top_idx/3 + self._t3c_y)});
        }})()"""
        self.evaluate_list.append(js_code)

    def clear_top3(self):
        if (self._canvas_id is None) or (self.page is None):
            return
        js_code = f"""(() => {{
            const canvas = document.getElementById('{self._top_3_canvas_id}');
            if (!canvas || !canvas.getContext) {{
                return;
            }}
            const ctx = canvas.getContext('2d');
            ctx.clearRect(0, 0, canvas.width, canvas.height);
        }})()"""
        self.evaluate_list.append(js_code)

    def exit(self):
        for page in self.page.context.pages:
            page.close()
        self.playwrightContextManager.__exit__()


if __name__ == '__main__':
    with open("settings.json", "r") as f:
        settings = json.load(f)
        mitm_port = settings["Port"]["MITM"]
        rpc_port = settings["Port"]["XMLRPC"]
        enable_unlocker = settings["Unlocker"]
        enable_helper = settings["Helper"]
        enable_playwright = settings["Playwright"]["enable"]
        playwright_width = settings["Playwright"]["width"]
        playwright_height = settings["Playwright"]["height"]
        autohu = settings["Autohu"]
        scale = playwright_width / 16
        majsoul_url = settings["MajsoulURL"]

    mitm_host="127.0.0.1"
    rpc_host="127.0.0.1"

    p = OptionParser()
    p.add_option("--mitm-host", default=None)
    p.add_option("--mitm-port", default=None)
    p.add_option("--rpc-host", default=None)
    p.add_option("--rpc-port", default=None)
    p.add_option("--unlocker", default=None)
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

    with open("mhmp.json", "r") as f:
        mhmp = json.load(f)
        mhmp["mitmdump"]["mode"] = [f"regular@{mitm_port}"]
        mhmp["hook"]["enable_skins"] = enable_unlocker
        mhmp["hook"]["enable_aider"] = enable_helper
    with open("mhmp.json", "w") as f:
        json.dump(mhmp, f, indent=4)
    # Create and start the proxy server thread
    proxy_thread = threading.Thread(target=lambda: asyncio.run(start_proxy(mitm_host, mitm_port, enable_unlocker)))
    proxy_thread.start()

    liqiServer = LiqiServer(rpc_host, rpc_port)
    # Create and start the LiqiServer thread
    server_thread = threading.Thread(target=lambda: liqiServer.serve_forever())
    server_thread.start()

    playwright_controller = None
    page = None
    if enable_playwright:
        playwright_controller = PlaywrightController(playwright_width, playwright_height, mitm_port, majsoul_url)

    click_list = []
    evaluate_list = []
    do_autohu = False
    # On Ctrl+C, stop the other threads
    try:
        if enable_playwright:
            playwright_controller.run()
    except KeyboardInterrupt:
        # On Ctrl+C, stop the other threads
        if enable_playwright:
            playwright_controller.exit()
        ctx.master.shutdown()
        liqiServer.server.shutdown()
        exit(0)

