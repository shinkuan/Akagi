import random

from mhm.addon import MessageProcessor
from mhm.hook import Hook
from mhm.protocol import GameMessageType
from mhm.resource import ResourceManager

POPULATION = ["chara", "skin", "gift"]
# NOTE: Weights of above `POPULATATION`
WEIGHTS = [5, 15, 80]


class EstHook(Hook):
    def __init__(self, resger: ResourceManager) -> None:
        super().__init__()

        @self.bind(GameMessageType.Response, ".lq.Lobby.login")
        @self.bind(GameMessageType.Response, ".lq.Lobby.emailLogin")
        @self.bind(GameMessageType.Response, ".lq.Lobby.oauth2Login")
        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchAccountInfo")
        def _(mp: MessageProcessor):
            mp.data["account"]["platform_diamond"] = [{"id": 100001, "count": 66666}]
            mp.amend()

        @self.bind(GameMessageType.Request, ".lq.Lobby.openChest")
        def _(mp: MessageProcessor):
            chest = resger.chest_map[mp.data["chest_id"]]
            count = mp.data["count"]
            # HACK: Currently UP and NORMAL chests are mixed
            mp.response(
                {
                    "results": [
                        {
                            "reward": {"count": 1, "id": random.choice(chest[k])},
                        }
                        for k in random.choices(POPULATION, WEIGHTS, k=count)
                    ],
                    "total_open_count": count,
                }
            )
