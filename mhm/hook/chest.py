import random

from mhm import resver
from mhm.hook import Hook
from mhm.proto import MsgManager, MsgType


def rewards(mapChest: dict, count: int, id: int):
    rewards = []

    if id not in mapChest:
        id = -999
    for i in range(0, count):
        aRandom, bRandom = random.random(), random.random()
        for (aPb, aPool), (bPb, bPool) in mapChest[id]:
            if aRandom < aPb:
                rewards.append(random.choice(bPool if bRandom < bPb else aPool))
                break

    return [{"reward": {"id": id, "count": 1}} for id in rewards]


def chest(mapChest: dict, count: int, id: int):
    return {
        "results": rewards(mapChest, count, id),
        "total_open_count": count,
    }


class OstHook(Hook):
    def __init__(self) -> None:
        super().__init__()

        aChars = [int(m) for m in resver.emotes]
        nViews = sorted(set(range(305001, 305056)).difference({305043, 305047}))
        gGifts = sorted(range(303012, 303090, 10))
        bGifts = sorted(range(303013, 303090, 10))

        self.mapChest = {
            1005: [
                [(0.05, aChars), (0.2, [200076])],
                [(0.2, nViews), (0, [])],
                [(1, gGifts), (0.0625, bGifts)],
            ],
            -999: [
                [(0.05, aChars), (0, [])],
                [(0.2, nViews), (0, [])],
                [(1, gGifts), (0.0625, bGifts)],
            ],
        }

        @self.bind(MsgType.Res, ".lq.Lobby.login")
        @self.bind(MsgType.Res, ".lq.Lobby.emailLogin")
        @self.bind(MsgType.Res, ".lq.Lobby.oauth2Login")  # login
        @self.bind(MsgType.Res, ".lq.Lobby.fetchAccountInfo")  # lobby refresh
        def _(mger: MsgManager):
            mger.data["account"]["platform_diamond"] = [{"id": 100001, "count": 66666}]
            mger.amend()

        @self.bind(MsgType.Req, ".lq.Lobby.openChest")
        def _(mger: MsgManager):
            nData = chest(self.mapChest, mger.data["count"], mger.data["chest_id"])
            mger.respond(nData)
