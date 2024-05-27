import os
import json
import random

from mhm import pRoot, conf, resver
from mhm.hook import Hook
from mhm.proto import MsgManager, MsgType


def _skin(charid: int) -> int:
    return charid % 1000 * 100 + 400001


def _skins(charid_set):
    for m in charid_set:
        i = _skin(m)
        for n in range(i, i + 9):
            yield n


def _characters(charid_set):
    for m in charid_set:
        yield {
            "charid": m,
            "level": 5,
            "exp": 1,
            "skin": _skin(m),
            "extra_emoji": resver.emotes.get(str(m)),
            "is_upgraded": True,
            "rewarded_level": [],
            "views": [],
        }


class SkinInfo:
    def __init__(self) -> None:
        inFrameSet = {305529, 305537, 305542, 305545, 305551, 305552} | set(
            range(305520, 305524)
        )

        exItemSet = {305214, 305314, 305526, 305725} | set(
            range(305501, 305556)
        ).difference(inFrameSet)

        exTitleSet = {
            600030,
            600043,
            # 600017,
            # 600024,
            # 600025,
            # 600029,
            # 600041,
            # 600044,
        } | set(range(600057, 600064))

        aItemList = sorted(set(range(305001, 309000)).difference(exItemSet))
        aTitleList = sorted(set(range(600002, 600082)).difference(exTitleSet))

        self.titleList = aTitleList
        self.itemList = [{"item_id": i, "stack": 1} for i in aItemList]


class KinHook(Hook):
    def __init__(self) -> None:
        super().__init__()

        self.info = SkinInfo()
        self.path = pRoot / "account"

        if not os.path.exists(self.path):
            os.mkdir(self.path)

        self.mapSkin: dict[int, Skin] = dict()
        self.mapGame: dict[str, dict] = dict()

        # Response

        @self.bind(MsgType.Res, ".lq.Lobby.login")
        @self.bind(MsgType.Res, ".lq.Lobby.emailLogin")
        @self.bind(MsgType.Res, ".lq.Lobby.oauth2Login")  # login
        def _(mger: MsgManager):
            self.mapSkin[mger.member] = Skin(self.path, mger)
            self.mapSkin[mger.member].update_player(mger.data.get("account"))
            mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.joinRoom")
        @self.bind(MsgType.Res, ".lq.Lobby.fetchRoom")
        @self.bind(MsgType.Res, ".lq.Lobby.createRoom")  # room
        def _(mger: MsgManager):
            # 在加入、获取、创建房间时修改己方头衔、立绘、角色
            if "room" not in mger.data:
                return True
            for person in mger.data["room"]["persons"]:
                if mSkin := self.mapSkin.get(person["account_id"]):
                    mSkin.update_player(person)
                    mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.fetchInfo")
        def _(mger: MsgManager):
            # 替换信息
            if mSkin := self.mapSkin.get(mger.member):
                mger.data["bag_info"]["bag"]["items"].extend(self.info.itemList)
                mger.data["title_list"]["title_list"] = self.info.titleList
                mger.data["all_common_views"] = mSkin.commonviews
                mger.data["character_info"] = mSkin.characterinfo
                mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.fetchBagInfo")
        def _(mger: MsgManager):
            # 添加物品
            if mSkin := self.mapSkin.get(mger.member):
                mger.data["bag"]["items"].extend(self.info.itemList)
                mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.fetchTitleList")
        def _(mger: MsgManager):
            # 添加头衔
            if mSkin := self.mapSkin.get(mger.member):
                mger.data["title_list"] = self.info.titleList
                mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.fetchAllCommonViews")
        def _(mger: MsgManager):
            # 装扮本地数据替换
            if mSkin := self.mapSkin.get(mger.member):
                mger.data = mSkin.commonviews
                mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.fetchCharacterInfo")
        def _(mger: MsgManager):
            # 全角色数据替换
            if mSkin := self.mapSkin.get(mger.member):
                mger.data = mSkin.characterinfo
                mger.amend()

        @self.bind(MsgType.Res, ".lq.Lobby.fetchAccountInfo")
        def _(mger: MsgManager):
            # 修改状态面板立绘、头衔
            if mSkin := self.mapSkin.get(mger.data["account"]["account_id"]):
                mSkin.update_player(mger.data["account"], "loading_image")
                mger.amend()

        @self.bind(MsgType.Res, ".lq.FastTest.authGame")
        def _(mger: MsgManager):
            # 进入对局时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.seat_list = mger.data["seat_list"]

                if mGame := self.mapGame.get(mSkin.game_uuid):
                    mger.data["players"] = mGame
                else:
                    for player in mger.data["players"]:
                        if pSkin := self.mapSkin.get(player["account_id"]):
                            # 替换对局头像，角色、头衔
                            pSkin.update_player(player)
                            if conf.hook.random_star_char:
                                nChar, nSkin = mSkin.random_star_character_and_skin
                                player["character"], player["avatar_id"] = nChar, nSkin
                        else:
                            # 其他玩家报菜名，对机器人无效
                            player["character"].update(
                                {"level": 5, "exp": 1, "is_upgraded": True}
                            )
                    self.mapGame[mSkin.game_uuid] = mger.data["players"]
                mger.amend()

        # Request

        @self.bind(MsgType.Req, ".lq.FastTest.authGame")
        def _(mger: MsgManager):
            # 记录当前对局 UUID
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.game_uuid = mger.data["game_uuid"]

        @self.bind(MsgType.Req, ".lq.FastTest.broadcastInGame")
        def _(mger: MsgManager):
            # 发送未持有的表情时
            emo = json.loads(mger.data["content"])["emo"]
            if emo > 8 and (mSkin := self.mapSkin.get(mger.member)):
                seat = mSkin.seat_list.index(mger.member)
                mger.notify_match(
                    ids=mSkin.seat_list,
                    method=".lq.NotifyGameBroadcast",
                    data={"seat": seat, "content": json.dumps({"emo": emo})},
                )
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.changeMainCharacter")
        def _(mger: MsgManager):
            # 修改主角色时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.main_character_id = mger.data["character_id"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.changeCharacterSkin")
        def _(mger: MsgManager):
            # 修改角色皮肤时
            if mSkin := self.mapSkin.get(mger.member):
                character = mSkin.character_of(mger.data["character_id"])
                character["skin"] = mger.data["skin"]
                mSkin.save()
                mger.notify(
                    method=".lq.NotifyAccountUpdate",
                    data={"update": {"character": {"characters": [character]}}},
                )
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.updateCharacterSort")
        def _(mger: MsgManager):
            # 修改星标角色时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.characterinfo["character_sort"] = mger.data["sort"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.useTitle")
        def _(mger: MsgManager):
            # 选择头衔时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.title = mger.data["title"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.modifyNickname")
        def _(mger: MsgManager):
            # 修改昵称时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.nickname = mger.data["nickname"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.setLoadingImage")
        def _(mger: MsgManager):
            # 选择加载图时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.loading_image = mger.data["images"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.useCommonView")
        def _(mger: MsgManager):
            # 选择装扮时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.use = mger.data["index"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.saveCommonViews")
        def _(mger: MsgManager):
            # 修改装扮时
            if mSkin := self.mapSkin.get(mger.member):
                sIndex = mger.data["save_index"]
                mSkin.commonviews["views"][sIndex]["values"] = mger.data["views"]
                mSkin.save()
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.setHiddenCharacter")
        def _(mger: MsgManager):
            # 隐藏角色时
            if mSkin := self.mapSkin.get(mger.member):
                mSkin.characterinfo["hidden_characters"] = mger.data["chara_list"]
                mSkin.save()
                mger.respond({"hidden_characters": mger.data["chara_list"]})

        @self.bind(MsgType.Req, ".lq.Lobby.addFinishedEnding")
        def _(mger: MsgManager):
            # 屏蔽传记完成请求
            if mSkin := self.mapSkin.get(mger.member):
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.receiveEndingReward")
        def _(mger: MsgManager):
            # 屏蔽传记奖励请求
            if mSkin := self.mapSkin.get(mger.member):
                mger.respond()

        @self.bind(MsgType.Req, ".lq.Lobby.receiveCharacterRewards")
        def _(mger: MsgManager):
            # 屏蔽角色奖励请求
            if mSkin := self.mapSkin.get(mger.member):
                mger.respond()

        # Notify

        @self.bind(MsgType.Notify, ".lq.NotifyRoomPlayerUpdate")
        def _(mger: MsgManager):
            # 房间中添加、减少玩家时修改立绘、头衔
            for player in mger.data["player_list"]:
                if mSkin := self.mapSkin.get(player["account_id"]):
                    mSkin.update_player(player)
                    mger.amend()

        @self.bind(MsgType.Notify, ".lq.NotifyGameFinishRewardV2")
        def _(mger: MsgManager):
            # 终局结算时，不播放羁绊动画
            if mSkin := self.mapSkin.get(mger.member):
                mger.data["main_character"] = {"exp": 1, "add": 0, "level": 5}
                mger.amend()


class Skin:
    @property
    def use(self) -> int:
        return self.commonviews["use"]

    @use.setter
    def use(self, value: int):
        self.commonviews["use"] = value

    @property
    def main_character_id(self) -> int:
        return self.characterinfo["main_character_id"]

    @main_character_id.setter
    def main_character_id(self, value: int):
        self.characterinfo["main_character_id"] = value

    @property
    def slots(self) -> list[dict]:
        return self.commonviews["views"][self.use].get("values", [])

    @property
    def views(self) -> dict:
        return [
            {
                "type": 0,
                "slot": slot["slot"],
                "item_id_list": [],
                "item_id": random.choice(slot["item_id_list"])
                if slot["type"]
                else slot["item_id"],
            }
            for slot in self.slots
        ]

    @property
    def avatar_frame(self) -> int:
        for slot in self.slots:
            if slot["slot"] == 5:
                return slot["item_id"]
        return 0

    @property
    def character(self) -> dict:
        return self.character_of(self.main_character_id)

    @property
    def random_star_character_and_skin(self) -> tuple[dict, int]:
        if self.characterinfo["character_sort"]:
            cIndex = random.choice(self.characterinfo["character_sort"])
            character = self.character_of(cIndex)
        else:
            character = self.character
        return character, character.get("skin")

    @property
    def avatar_id(self) -> int:
        return self.character["skin"]

    def __init__(self, root: str, mger: MsgManager) -> None:
        self.path = root / "{}.json".format(mger.member)

        # base attributes
        self.keys = ["title", "nickname", "loading_image"]
        self.title: int = None
        self.nickname: str = None
        self.loading_image: list = None

        # temp attributes
        self.seat_list: list = None
        self.game_uuid: str = None

        self.update_self(mger.data.get("account"))

        if os.path.exists(self.path):
            self.load()
        else:
            self.init()

    def character_of(self, charid: int) -> dict:
        for character in self.characterinfo["characters"]:
            if charid == character.get("charid"):
                return character

    def update_self(self, player: dict):
        for key in player:
            if key in self.keys:
                setattr(self, key, player[key])

    def update_player(self, player: dict, *exclude: str):
        for key in player:
            if key not in exclude and hasattr(self, key):
                player[key] = getattr(self, key)

    def save(self):
        with open(self.path, "w", encoding="utf-8") as f:
            data = {
                "base": {k: getattr(self, k) for k in self.keys},
                "commonviews": self.commonviews,
                "characterinfo": self.characterinfo,
            }

            json.dump(data, f, ensure_ascii=False)

    def load(self):
        with open(self.path, "r", encoding="utf-8") as f:
            data: dict = json.load(f)

            base = data.get("base", data.get("account"))
            self.commonviews = data.get("commonviews")
            self.characterinfo = data.get("characterinfo", data.get("characters"))

            for key in self.keys:
                setattr(self, key, base[key])

            self.update_characterinfo()

    def init(self):
        # commonviews
        self.commonviews = {
            "views": [{"values": [], "index": i} for i in range(0, 10)],
            "use": 0,
        }

        # characterinfo
        res_charid_set = set(map(int, resver.emotes.keys()))

        main_character_id = 200001
        skins = list(_skins(res_charid_set))
        characters = list(_characters(res_charid_set))

        self.characterinfo = {
            "characters": characters,
            "skins": skins,
            "main_character_id": main_character_id,
            "send_gift_limit": 2,
            "character_sort": [],
            "finished_endings": [],
            "hidden_characters": [],
            "rewarded_endings": [],
            "send_gift_count": 0,
        }

        # save
        self.save()

    def update_characterinfo(self):
        charid_set = {i.get("charid") for i in self.characterinfo["characters"]}
        res_charid_set = set(map(int, resver.emotes.keys()))

        skins: list[int] = self.characterinfo.get("skins")
        characters: list[dict] = self.characterinfo.get("characters")

        for m in characters:
            m["extra_emoji"] = resver.emotes.get(str(m["charid"]))

        if remove_m := sorted(charid_set - res_charid_set):
            skins[:] = [m for m in skins if m not in _skins(remove_m)]
            characters[:] = [m for m in characters if m.get("charid") not in remove_m]

        if extend_m := sorted(res_charid_set - charid_set):
            skins.extend(_skins(extend_m))
            characters.extend(_characters(extend_m))
