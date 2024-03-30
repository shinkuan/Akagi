# TODO: Consider compatibility with extensions from the majsoul-plus.

from mhm.addon import MessageProcessor
from mhm.protocol import GameMessageType


class Hook:
    def __init__(self) -> None:
        self.mapping = {}

    def run(self, mp: MessageProcessor):
        if mp.key in self.mapping:
            self.mapping[mp.key](mp)

    def bind(self, kind: GameMessageType, name: str):
        def decorator(func):
            key = (kind, name)
            self.mapping[key] = func
            return func

        return decorator
