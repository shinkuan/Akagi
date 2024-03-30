import socket

import requests
from urllib3 import disable_warnings
from urllib3.exceptions import InsecureRequestWarning

from mhm import console
from mhm.addon import MessageProcessor
from mhm.hook import Hook
from mhm.protocol import GameMessageType


class DerHook(Hook):
    def __init__(self) -> None:
        self.pool = {}
        disable_warnings(InsecureRequestWarning)

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(1)
        if sock.connect_ex(("127.0.0.1", 12121)) == 0:  # HACK
            console.log("[green]Aider Detected")
            self.open = True
        else:
            console.log("[red]No Aider Detected")
            self.open = False

    def run(self, mp: MessageProcessor):
        if self.open and mp.kind != GameMessageType.Request:
            self.send(mp)

    def send(self, mp: MessageProcessor):
        # TODO: modify the helper source code to be compatible with SHA256
        if mp.name == ".lq.ActionPrototype":
            if mp.data["name"] == "ActionNewRound":
                mp.data["data"]["md5"] = mp.data["data"]["sha256"][:32]
            send_msg = mp.data["data"]
        elif mp.name == ".lq.FastTest.syncGame":
            for action in mp.data["game_restore"]["actions"]:
                if action["name"] == "ActionNewRound":
                    action["data"]["md5"] = action["data"]["sha256"][:32]
            send_msg = {"sync_game_actions": mp.data["game_restore"]["actions"]}
        else:
            send_msg = mp.data
        # TODO: This URL should be included in the configuration file
        # TODO: It's preferable to perform POST requests asynchronously
        #       to avoid blocking and unnecessary checks
        requests.post("https://127.0.0.1:12121", json=send_msg, verify=0, timeout=1)
