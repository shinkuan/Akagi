from xmlrpc.client import ServerProxy
from mitm.config import config as mitm_config


class XMLRPCClient:
    def __init__(self):
        self.host = mitm_config.xmlrpc.host
        self.port = mitm_config.xmlrpc.port
        self.server = ServerProxy(f"http://{self.host}:{self.port}", allow_none=True)

    def get_activated_flows(self):
        return self.server.get_activated_flows()

    def get_message(self, flow_id):
        return self.server.get_message(flow_id)

    def reset_message_idx(self):
        return self.server.reset_message_idx()

    def page_evaluate(self, script: str):
        return self.server.page_evaluate(script)

    def ping(self):
        return self.server.ping()

    def __del__(self):
        del self.server
        pass
    pass
