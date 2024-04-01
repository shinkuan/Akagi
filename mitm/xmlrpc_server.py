from .config import config as mitm_config
from .client_websocket import activated_flows, messages_dict
from xmlrpc.server import SimpleXMLRPCServer

page_evaluate_list: list[str] = []


class XMLRPCServer:
    _rpc_methods_ = ['get_activated_flows', 'get_message', 'reset_message_idx', 'page_evaluate', 'ping']
    def __init__(self):
        self.host = mitm_config.xmlrpc.host
        self.port = mitm_config.xmlrpc.port
        self.server = SimpleXMLRPCServer((self.host, self.port), allow_none=True, logRequests=False)
        for name in self._rpc_methods_:
            self.server.register_function(getattr(self, name))
        self.message_idx = dict() # flow.id -> int

    def get_activated_flows(self):
        global activated_flows
        return activated_flows
    
    def get_message(self, flow_id):
        global activated_flows, messages_dict
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
        global activated_flows
        for flow_id in activated_flows:
            self.message_idx[flow_id] = 0

    def page_evaluate(self, script: str):
        page_evaluate_list.append(script)
        return True

    def ping(self):
        return True

    def serve_forever(self):
        print(f"XMLRPC Server is running on {self.host}:{self.port}")
        self.server.serve_forever()