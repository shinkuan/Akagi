import os
import socket

import requests
from urllib3 import disable_warnings
from urllib3.exceptions import InsecureRequestWarning

from mhm import console
from mhm.addon import MessageProcessor
from mhm.hook import Hook
from mhm.protocol import GameMessageType
from mhm.config import ROOT

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
            console.log("[green]Starting Aider")
            cmd = f'start cmd /c "title Console · 🀄 && {ROOT / "common/endless/mahjong-helper"} -majsoul -p 12121"'
            os.system(cmd)
            self.open = True

    def run(self, mp: MessageProcessor):
        if self.open and mp.msg.kind != GameMessageType.Request:
            self.send(mp)

    def send(self, mp: MessageProcessor):
        # TODO: modify the helper source code to be compatible with SHA256
        if mp.msg.name == ".lq.ActionPrototype":
            if mp.msg.data["name"] == "ActionNewRound":
                mp.msg.data["data"]["md5"] = mp.msg.data["data"]["sha256"][:32]
            send_msg = mp.msg.data["data"]
        elif mp.msg.name == ".lq.FastTest.syncGame":
            for action in mp.msg.data["game_restore"]["actions"]:
                if action["name"] == "ActionNewRound":
                    action["data"]["md5"] = action["data"]["sha256"][:32]
            send_msg = {"sync_game_actions": mp.msg.data["game_restore"]["actions"]}
        else:
            send_msg = mp.msg.data
        # TODO: This URL should be included in the configuration file
        # TODO: It's preferable to perform POST requests asynchronously
        #       to avoid blocking and unnecessary checks
        requests.post("https://127.0.0.1:12121", json=send_msg, verify=0, timeout=1)
