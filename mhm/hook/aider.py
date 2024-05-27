import os
import requests

from urllib3 import disable_warnings
from urllib3.exceptions import InsecureRequestWarning
from socket import socket, AF_INET, SOCK_STREAM

from mhm import pRoot
from mhm.proto import MsgManager
from mhm.hook import Hook


class DerHook(Hook):
    def __init__(self) -> None:
        self.pool = dict()

        disable_warnings(InsecureRequestWarning)

        self.path = pRoot / "common/endless/mahjong-helper"

    def hook(self, mger: MsgManager):
        if mger.m.isReq():
            return

        if mger.member not in self.pool:
            self.pool[mger.member] = Aider(self.path)

        if mger.m.method == ".lq.ActionPrototype":
            if mger.data["name"] == "ActionNewRound":
                mger.data["data"]["md5"] = mger.data["data"]["sha256"][:32]
            send_msg = mger.data["data"]
        elif mger.m.method == ".lq.FastTest.syncGame":
            for action in mger.data["game_restore"]["actions"]:
                if action["name"] == "ActionNewRound":
                    action["data"]["md5"] = action["data"]["sha256"][:32]
            send_msg = {"sync_game_actions": mger.data["game_restore"]["actions"]}
        else:
            send_msg = mger.data

        requests.post(self.pool[mger.member].api, json=send_msg, verify=0)


class Aider:
    PORT = 43410

    def __init__(self, path: str) -> None:
        with socket(AF_INET, SOCK_STREAM) as s:
            s.settimeout(0.2)
            if s.connect_ex(("127.0.0.1", Aider.PORT)) != 0:
                cmd = f'start cmd /c "title Console · 🀄 && {path} -majsoul -p {Aider.PORT}"'
                os.system(cmd)

        self.api = f"https://127.0.0.1:{Aider.PORT}"

        Aider.PORT += 1
