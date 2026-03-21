import threading
import traceback
import asyncio
import queue
import time
import mitmproxy.http
import mitmproxy.log
import mitmproxy.tcp
import mitmproxy.websocket
from mitmproxy import proxy, options, ctx
from mitmproxy.tools.dump import DumpMaster
from .bridge import MajsoulBridge
from .mitm_abc import ClientWebSocketABC
from .logger import logger

# Because in Majsouls, every flow's message has an id, we need to use one bridge for each flow
activated_flows: list[str] = [] # store all flow.id ([-1] is the recently opened)
majsoul_bridges: dict[str, MajsoulBridge] = {} # store all flow.id -> MajsoulBridge
mjai_messages: queue.Queue[dict] = queue.Queue() # store all messages


class ClientWebSocket(ClientWebSocketABC):
    def __init__(self):
        self.bridge_lock = threading.Lock()
        pass

    def websocket_start(self, flow: mitmproxy.http.HTTPFlow):
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows, majsoul_bridges
        logger.info(f"WebSocket connection opened: {flow.id}")
        
        activated_flows.append(flow.id)
        majsoul_bridges[flow.id] = MajsoulBridge()

    def websocket_message(self, flow: mitmproxy.http.HTTPFlow):
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows, majsoul_bridges
        try:
            if flow.id in activated_flows:
                msg = flow.websocket.messages[-1]
                if msg.from_client:
                    logger.debug(f"<- Message: {msg.content}")
                else: # from server
                    logger.debug(f"-> Message: {msg.content}")
                self.bridge_lock.acquire()
                bridge = majsoul_bridges[flow.id]
                msgs = bridge.parse(msg.content)
                self.bridge_lock.release()
                if msgs is None:
                    return
                for m in msgs:
                    mjai_messages.put(m)
            else:
                logger.error(f"WebSocket message received from unactivated flow: {flow.id}")
        except Exception as e:
            # Release the lock if it is locked
            if self.bridge_lock.locked():
                self.bridge_lock.release()
            logger.error(f"Error: {traceback.format_exc()}")
            logger.error(f"Error: {str(e)}")
            logger.error(f"Error: {e.__traceback__.tb_lineno}")

    def websocket_end(self, flow: mitmproxy.http.HTTPFlow):
        global activated_flows, majsoul_bridges
        if flow.id in activated_flows:
            logger.info(f"WebSocket connection closed: {flow.id}")
            activated_flows.remove(flow.id)
            del majsoul_bridges[flow.id]
        else:
            logger.error(f"WebSocket connection closed from unactivated flow: {flow.id}")

async def start_proxy(host, port):
    opts = options.Options(listen_host=host, listen_port=port, ssl_insecure=True)
    master = DumpMaster(
        opts,
        with_termlog=False,
        with_dumper=False,
    )
    master.addons.add(ClientWebSocket())
    logger.info(f"Starting MITM proxy server at {host}:{port}")
    await master.run()
    logger.info("MITM proxy server stopped")
    return master

def stop_proxy():
    ctx.master.shutdown()
