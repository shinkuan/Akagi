import mitmproxy.addonmanager
import mitmproxy.http
import mitmproxy.log
import mitmproxy.tcp
import mitmproxy.websocket

activated_flows = []    # store all flow.id ([-1] is the recently opened)
messages_dict = dict()  # flow.id -> Queue[flow_msg]


class ClientWebSocket:
    def __init__(self):
        pass

    def websocket_start(self, flow: mitmproxy.http.HTTPFlow):
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows, messages_dict
        
        activated_flows.append(flow.id)
        messages_dict[flow.id] = []

    def websocket_message(self, flow: mitmproxy.http.HTTPFlow):
        assert isinstance(flow.websocket, mitmproxy.websocket.WebSocketData)
        global activated_flows, messages_dict

        messages_dict[flow.id].append(flow.websocket.messages[-1].content)

    def websocket_end(self, flow: mitmproxy.http.HTTPFlow):
        global activated_flows, messages_dict
        activated_flows.remove(flow.id)
        messages_dict.pop(flow.id)