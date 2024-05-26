# TODO: Consider compatibility with extensions from the majsoul-plus.
from collections import defaultdict

from mhm.addon import MessageProcessor
from mhm.protocol import GameMessageType


class Hook:
    def __init__(self) -> None:
        self.mapping = defaultdict(list)

    def run(self, mp: MessageProcessor):
        key = mp.msg.key
        if key in self.mapping:
            [func(mp) for func in self.mapping[key]]

    def bind(self, kind: GameMessageType, name: str):
        def decorator(func):
            key = (kind, name)
            self.mapping[key].append(func)
            return func

        return decorator
