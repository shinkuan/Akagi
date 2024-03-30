import eel
from mhm import protocol
from my_logger import both_logger
from mitm.config import config as mitm_config

from .config import config as akagi_config
from .xmlrpc_client import XMLRPCClient
from .majsoul2mjai import MajsoulBridge


class MessageController:
    def __init__(self):
        self.client = XMLRPCClient()
        self.stop_me = False

        self.activated_flows = []
        self.bridge: dict[str, MajsoulBridge] = {}
        self.messages_dict  = dict() # flow.id -> List[flow_msg]
        self.liqi_msg_dict  = dict() # flow.id -> List[liqi_msg]
        self.mjai_msg_dict  = dict() # flow.id -> List[mjai_msg]

    def get_activated_flows(self):
        old_activated_flows = self.activated_flows
        new_activated_flows = self.client.get_activated_flows()
        self.activated_flows = new_activated_flows
        for new_flow in new_activated_flows:
            if new_flow not in self.messages_dict:
                self.bridge[new_flow] = MajsoulBridge()
                self.messages_dict[new_flow] = []
                self.liqi_msg_dict[new_flow] = []
                self.mjai_msg_dict[new_flow] = []
        for old_flow in old_activated_flows:
            if old_flow not in new_activated_flows:
                self.bridge.pop(old_flow)
                self.messages_dict.pop(old_flow)
                self.liqi_msg_dict.pop(old_flow)
                self.mjai_msg_dict.pop(old_flow)
        return self.activated_flows
    
    #  TODO: Use GameMessage class instead of dict
    def game_message_to_dict(self, game_message: protocol.GameMessage):
        return {
            "id": game_message.idx,
            "type": game_message.kind,
            "method": game_message.name,
            "data": game_message.data,
        }

    def get_message(self, flow_id):
        message = self.client.get_message(flow_id)
        if message is None:
            return
        
        message = message.data
        assert isinstance(message, bytes)
        self.messages_dict[flow_id].append(message)
        liqi_msg = protocol.parse(flow_id, message)
        liqi_msg = self.game_message_to_dict(liqi_msg)
        if liqi_msg is None:
            return
        
        both_logger.debug(f"Flow {flow_id} received: {liqi_msg}")
        self.liqi_msg_dict[flow_id].append(liqi_msg)
        mjai_msg = self.bridge[flow_id].input(liqi_msg)
        if mjai_msg is None:
            return
        
        self.mjai_msg_dict[flow_id].append(mjai_msg)
        eel.set_mjai_msg(str(mjai_msg))
        return

    def start(self):
        both_logger.info("Starting Message Client Controller")
        server_alive = False
        ping_count = 0
        while not server_alive:
            try:
                server_alive = self.client.ping()
            except Exception as e:
                if ping_count > akagi_config.xmlrpc_client.max_ping_count:
                    both_logger.error(f"XMLRPC Server is not running: {e}")
                    raise e
                ping_count += 1
                eel.sleep(1)
        both_logger.info("XMLRPC Client connected.")
        both_logger.info("XMLRPC Server is running.")
        while not self.stop_me:
            self.get_activated_flows()
            for flow in self.activated_flows:
                while self.get_message(flow) is not None:
                    pass
            eel.sleep(akagi_config.xmlrpc_client.refresh_interval)

    def stop(self):
        both_logger.info("Stopping Message Controller")
        self.stop_me = True
        pass
