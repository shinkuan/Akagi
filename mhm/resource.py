import random
from collections import defaultdict
from typing import Literal

import requests
from google.protobuf.json_format import MessageToDict
from google.protobuf.message import Message

from .config import ROOT, config
from .proto import config_pb2, sheets_pb2

HOST = "https://game.maj-soul.com/1"

LQBIN_RKEY = "res/config/lqc.lqbin"
LQBIN_VTXT = ROOT / "lqc.txt"
LQBIN_PATH = ROOT / "lqc.lqbin"

SheetNames = Literal[
    "item_definition_loading_image",
    "item_definition_character",
    "item_definition_title",
    "item_definition_skin",
    "item_definition_item",
    "character_emoji",
    "chest_preview",
    "spot_rewards",
]
# NOTE: Message names in `SheetNames` will be resolved,
# while unspecified messages will not be resolved to achieve optimal compatibility


class ResourceManager:
    RENAME_SCROLL = 302013
    VIEW_CATEGORY = 5

    def __init__(self, lqbin: bytes, version: str) -> None:
        sheets_table: dict[SheetNames, list] = defaultdict(list)

        config_table = config_pb2.ConfigTables()
        config_table.ParseFromString(lqbin)

        for data in config_table.datas:
            klass_key = f"{data.table}_{data.sheet}"
            klass_words = klass_key.split("_")

            if klass_key in SheetNames.__args__:
                klass_name = "".join(n.capitalize() for n in klass_words)
                klass: Message = getattr(sheets_pb2, klass_name)

                for buffer in data.data:
                    message: Message = klass()
                    message.ParseFromString(buffer)

                    message_dict = MessageToDict(
                        message,
                        including_default_value_fields=True,
                        preserving_proto_field_name=True,
                    )  # IDEA: It seems that there is no need to convert msg into a dict

                    sheets_table[klass_key].append(message_dict)

        self.sheets_table = sheets_table
        self.version = version

    def build(self) -> "ResourceManager":
        self.skin_map = defaultdict(list)
        for row in self.sheets_table["item_definition_skin"]:
            self.skin_map[row["character_id"]].append(row["id"])
        self.skin_rows = [n for m in self.skin_map.values() for n in m]

        self.extra_emoji_map = defaultdict(list)
        for row in self.sheets_table["character_emoji"]:
            self.extra_emoji_map[row["charid"]].append(row["sub_id"])
        if config.base.no_cheering_emotes:
            exclude = set(range(13, 19))
            for emotes in self.extra_emoji_map.values():
                emotes[:] = sorted(set(emotes) - exclude)

        self.character_map = {
            m["id"]: {
                "charid": m["id"],
                "skin": m["init_skin"],
                "extra_emoji": self.extra_emoji_map[m["id"]],
                "level": 5,
                "exp": 1,
                "is_upgraded": True,
                "rewarded_level": [1, 2, 3, 4, 5],
                "views": [],
            }
            for m in self.sheets_table["item_definition_character"]
        }
        self.character_rows = list(self.character_map.values())

        self.item_rows = [ResourceManager.RENAME_SCROLL]
        for row in self.sheets_table["item_definition_item"]:
            if row["category"] == ResourceManager.VIEW_CATEGORY:
                self.item_rows.append(row["id"])
        for row in self.sheets_table["item_definition_loading_image"]:
            self.item_rows.append(row["unlock_items"][0])
        self.bag_rows = [{"item_id": m, "stack": 1} for m in self.item_rows]

        self.chest_map = defaultdict(lambda: defaultdict(list))
        for row in self.sheets_table["chest_preview"]:
            self.chest_map[row["chest_id"]][row["type"]].append(row["item_id"])

        self.title_rows = [m["id"] for m in self.sheets_table["item_definition_title"]]
        self.spot_rewards = [m["id"] for m in self.sheets_table["spot_rewards"]]
        return self


def load_resource() -> ResourceManager:
    rand_a: int = random.randint(0, int(1e9))
    rand_b: int = random.randint(0, int(1e9))

    # use requests.Session() instead of open/close the connection multiple times.
    with requests.Session() as s:
        ver_url = f"{HOST}/version.json?randv={rand_a}{rand_b}"
        response = s.get(ver_url, proxies={"https": None})
        response.raise_for_status()
        version: str = response.json()["version"]

        res_url = f"{HOST}/resversion{version}.json"
        response = s.get(res_url, proxies={"https": None}, stream=True)
        response.raise_for_status()
        bin_version: str = response.json()["res"][LQBIN_RKEY]["prefix"]

        # Using Cache
        if LQBIN_VTXT.exists():
            with LQBIN_VTXT.open("r") as txt:
                if txt.read() == bin_version:
                    with LQBIN_PATH.open("rb") as bin:
                        return ResourceManager(bin.read(), bin_version).build()

        bin_url = f"{HOST}/{bin_version}/{LQBIN_RKEY}"
        response = s.get(bin_url, proxies={"https": None}, stream=True)
        response.raise_for_status()

        content = b"".join(response.iter_content(chunk_size=8192))
        with LQBIN_PATH.open("wb") as bin, LQBIN_VTXT.open("w") as txt:
            bin.write(content)
            txt.write(bin_version)
        return ResourceManager(content, bin_version).build()
