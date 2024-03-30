# TODO: This part needs to be refactored
import json
import os
import random
from pathlib import Path

from mhm.addon import MessageProcessor
from mhm.config import config
from mhm.hook import Hook
from mhm.protocol import GameMessageType
from mhm.resource import ResourceManager


class KinHook(Hook):
    def __init__(self, resger: ResourceManager) -> None:  # noqa: C901
        super().__init__()

        self.path = Path("./account")

        if not os.path.exists(self.path):
            os.mkdir(self.path)

        self.mapSkin: dict[int, Skin] = {}
        self.mapGame: dict[str, dict] = {}

        # Response

        @self.bind(GameMessageType.Response, ".lq.Lobby.login")
        @self.bind(GameMessageType.Response, ".lq.Lobby.emailLogin")
        @self.bind(GameMessageType.Response, ".lq.Lobby.oauth2Login")  # login
        def _(mp: MessageProcessor):
            self.mapSkin[mp.member] = Skin(self.path, mp.member, mp.data, resger)
            self.mapSkin[mp.member].update_player(mp.data.get("account"))
            mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.joinRoom")
        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchRoom")
        @self.bind(GameMessageType.Response, ".lq.Lobby.createRoom")  # room
        def _(mp: MessageProcessor):
            # 在加入、获取、创建房间时修改己方头衔、立绘、角色
            if "room" not in mp.data:
                return True
            for person in mp.data["room"]["persons"]:
                if mSkin := self.mapSkin.get(person["account_id"]):
                    mSkin.update_player(person)
                    mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchInfo")
        def _(mp: MessageProcessor):
            # 替换信息
            if mSkin := self.mapSkin.get(mp.member):
                mp.data["bag_info"]["bag"]["items"].extend(resger.bag_rows)
                mp.data["title_list"]["title_list"] = resger.title_rows
                mp.data["all_common_views"] = mSkin.commonviews
                mp.data["character_info"] = mSkin.characterinfo
                mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchBagInfo")
        def _(mp: MessageProcessor):
            # 添加物品
            if self.mapSkin.get(mp.member):
                mp.data["bag"]["items"].extend(resger.bag_rows)
                mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchTitleList")
        def _(mp: MessageProcessor):
            # 添加头衔
            if self.mapSkin.get(mp.member):
                mp.data["title_list"] = resger.title_rows
                mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchAllCommonViews")
        def _(mp: MessageProcessor):
            # 装扮本地数据替换
            if mSkin := self.mapSkin.get(mp.member):
                mp.data = mSkin.commonviews
                mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchCharacterInfo")
        def _(mp: MessageProcessor):
            # 全角色数据替换
            if mSkin := self.mapSkin.get(mp.member):
                mp.data = mSkin.characterinfo
                mp.amend()

        @self.bind(GameMessageType.Response, ".lq.Lobby.fetchAccountInfo")
        def _(mp: MessageProcessor):
            # 修改状态面板立绘、头衔
            if mSkin := self.mapSkin.get(mp.data["account"]["account_id"]):
                mSkin.update_player(mp.data["account"], "loading_image")
                mp.amend()

        @self.bind(GameMessageType.Response, ".lq.FastTest.authGame")
        def _(mp: MessageProcessor):
            # 进入对局时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.seat_list = mp.data["seat_list"]

                if mGame := self.mapGame.get(mSkin.game_uuid):
                    mp.data["players"] = mGame
                else:
                    for player in mp.data["players"]:
                        if pSkin := self.mapSkin.get(player["account_id"]):
                            # 替换对局头像，角色、头衔
                            pSkin.update_player(player)
                            if config.base.random_star_char:
                                nChar, nSkin = mSkin.random_star_character_and_skin
                                player["character"], player["avatar_id"] = nChar, nSkin
                        else:
                            # 其他玩家报菜名，对机器人无效
                            player["character"].update(
                                {"level": 5, "exp": 1, "is_upgraded": True}
                            )
                    self.mapGame[mSkin.game_uuid] = mp.data["players"]
                mp.amend()

        # Request

        @self.bind(GameMessageType.Request, ".lq.FastTest.authGame")
        def _(mp: MessageProcessor):
            # 记录当前对局 UUID
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.game_uuid = mp.data["game_uuid"]

        @self.bind(GameMessageType.Request, ".lq.FastTest.broadcastInGame")
        def _(mp: MessageProcessor):
            # 发送未持有的表情时
            emo = json.loads(mp.data["content"])["emo"]
            if emo > 8 and (mSkin := self.mapSkin.get(mp.member)):
                seat = mSkin.seat_list.index(mp.member)
                mp.broadcast(
                    channel="MaTCH",
                    members=mSkin.seat_list,
                    name=".lq.NotifyGameBroadcast",
                    data={"seat": seat, "content": json.dumps({"emo": emo})},
                )
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.changeMainCharacter")
        def _(mp: MessageProcessor):
            # 修改主角色时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.main_character_id = mp.data["character_id"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.changeCharacterSkin")
        def _(mp: MessageProcessor):
            # 修改角色皮肤时
            if mSkin := self.mapSkin.get(mp.member):
                character = mSkin.character_of(mp.data["character_id"])
                character["skin"] = mp.data["skin"]
                mSkin.save()
                mp.notify(
                    name=".lq.NotifyAccountUpdate",
                    data={"update": {"character": {"characters": [character]}}},
                )
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.updateCharacterSort")
        def _(mp: MessageProcessor):
            # 修改星标角色时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.characterinfo["character_sort"] = mp.data["sort"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.useTitle")
        def _(mp: MessageProcessor):
            # 选择头衔时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.title = mp.data["title"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.modifyNickname")
        def _(mp: MessageProcessor):
            # 修改昵称时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.nickname = mp.data["nickname"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.setLoadingImage")
        def _(mp: MessageProcessor):
            # 选择加载图时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.loading_image = mp.data["images"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.useCommonView")
        def _(mp: MessageProcessor):
            # 选择装扮时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.use = mp.data["index"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.saveCommonViews")
        def _(mp: MessageProcessor):
            # 修改装扮时
            if mSkin := self.mapSkin.get(mp.member):
                sIndex = mp.data["save_index"]
                mSkin.commonviews["views"][sIndex]["values"] = mp.data["views"]
                mSkin.save()
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.setHiddenCharacter")
        def _(mp: MessageProcessor):
            # 隐藏角色时
            if mSkin := self.mapSkin.get(mp.member):
                mSkin.characterinfo["hidden_characters"] = mp.data["chara_list"]
                mSkin.save()
                mp.response({"hidden_characters": mp.data["chara_list"]})

        @self.bind(GameMessageType.Request, ".lq.Lobby.addFinishedEnding")
        def _(mp: MessageProcessor):
            # 屏蔽传记完成请求
            if self.mapSkin.get(mp.member):
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.receiveEndingReward")
        def _(mp: MessageProcessor):
            # 屏蔽传记奖励请求
            if self.mapSkin.get(mp.member):
                mp.response()

        @self.bind(GameMessageType.Request, ".lq.Lobby.receiveCharacterRewards")
        def _(mp: MessageProcessor):
            # 屏蔽角色奖励请求
            if self.mapSkin.get(mp.member):
                mp.response()

        # Notify

        @self.bind(GameMessageType.Notify, ".lq.NotifyRoomPlayerUpdate")
        def _(mp: MessageProcessor):
            # 房间中添加、减少玩家时修改立绘、头衔
            for player in mp.data["player_list"]:
                if mSkin := self.mapSkin.get(player["account_id"]):
                    mSkin.update_player(player)
                    mp.amend()

        @self.bind(GameMessageType.Notify, ".lq.NotifyGameFinishRewardV2")
        def _(mp: MessageProcessor):
            # 终局结算时，不播放羁绊动画
            if self.mapSkin.get(mp.member):
                mp.data["main_character"] = {"exp": 1, "add": 0, "level": 5}
                mp.amend()


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

    def __init__(
        self, root: str, member: int, data: dict, resger: ResourceManager
    ) -> None:
        self.path = root / f"{member}.json"

        # base attributes
        self.keys = ["title", "nickname", "loading_image"]
        self.title: int = None
        self.nickname: str = None
        self.loading_image: list = None

        # temp attributes
        self.seat_list: list = None
        self.game_uuid: str = None

        self.update_self(data.get("account"))

        if os.path.exists(self.path):
            self.load(resger)
        else:
            self.init(resger)

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

    def load(self, resger: ResourceManager):
        with open(self.path, encoding="utf-8") as f:
            data: dict = json.load(f)

            base = data.get("base", data.get("account"))
            self.commonviews = data.get("commonviews")
            self.characterinfo = data.get("characterinfo", data.get("characters"))

            for key in self.keys:
                setattr(self, key, base[key])

            self.update_characterinfo(resger)

    def init(self, resger: ResourceManager):
        # commonviews
        self.commonviews = {
            "views": [{"values": [], "index": i} for i in range(0, 10)],
            "use": 0,
        }

        # characterinfo
        self.characterinfo = {
            "characters": resger.character_rows,
            "skins": resger.skin_rows,
            "main_character_id": 200001,
            "send_gift_limit": 2,
            "character_sort": [],
            "finished_endings": [],
            "hidden_characters": [],
            "rewarded_endings": [],
            "send_gift_count": 0,
        }

        # save
        self.save()

    def update_characterinfo(self, resger: ResourceManager):
        characters: list[dict] = self.characterinfo["characters"]

        now_charid_set = {m["charid"] for m in characters}
        res_charid_set = {m["charid"] for m in resger.character_rows}

        for m in characters:
            m["extra_emoji"] = resger.extra_emoji_map[m["charid"]]

        if remove_chars := sorted(now_charid_set - res_charid_set):
            characters[:] = {m for m in characters if m["charid"] not in remove_chars}

        if extend_chars := sorted(res_charid_set - now_charid_set):
            characters.extend([resger.character_map[c] for c in extend_chars])

        self.characterinfo["skins"] = resger.skin_rows
