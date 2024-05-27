from rich.console import Console
from rich.logging import RichHandler
from collections import defaultdict
from dataclasses import dataclass, asdict, field
from os.path import exists
from os import environ
from json import load, dump
from logging import getLogger
from pathlib import Path

pRoot = Path(".")

pathConf = pRoot / "mhmp.json"
pathResVer = pRoot / "resver.json"


@dataclass
class ResVer:
    version: str = None
    emotes: dict[str, list] = None

    @classmethod
    def fromdict(cls, data: dict):
        # purge
        if "max_charid" in data:
            data.pop("max_charid")
        if "emos" in data:
            data["emotes"] = data.pop("emos")
        return cls(**data)


@dataclass
class Conf:
    @dataclass
    class Base:
        log_level: str = "info"
        pure_python_protobuf: bool = False

    @dataclass
    class Hook:
        enable_skins: bool = True
        enable_aider: bool = False
        enable_chest: bool = False
        random_star_char: bool = False
        no_cheering_emotes: bool = False

    mhm: Base = None
    hook: Hook = None
    dump: dict = None
    mitmdump: dict = None
    proxinject: dict = None

    @classmethod
    def default(cls):
        return cls(
            mhm=cls.Base(),
            hook=cls.Hook(),
            dump={"with_dumper": False, "with_termlog": True},
            mitmdump={"http2": False, "mode": ["socks5@127.0.0.1:7070"]},
            proxinject={"name": "jantama_mahjongsoul", "set-proxy": "127.0.0.1:7070"},
        )

    @classmethod
    def fromdict(cls, data: dict):
        # purge
        if "server" in data:
            data.pop("server")
        if "plugin" in data:
            data["hook"] = data.pop("plugin")
        # to dataclass
        for key, struct in [("mhm", cls.Base), ("hook", cls.Hook)]:
            if key in data:
                data[key] = struct(**data[key])
        return cls(**data)


if exists(pathConf):
    conf = Conf.fromdict(load(open(pathConf, "r")))
else:
    conf = Conf.default()

if exists(pathResVer):
    resver = ResVer.fromdict(load(open(pathResVer, "r")))
else:
    resver = ResVer()


def fetch_resver():
    """Fetch the latest character id and emojis"""
    import requests
    import random
    import re

    rand_a: int = random.randint(0, int(1e9))
    rand_b: int = random.randint(0, int(1e9))

    ver_url = f"https://game.maj-soul.com/1/version.json?randv={rand_a}{rand_b}"
    response = requests.get(ver_url, proxies={"https": None})
    response.raise_for_status()
    version: str = response.json().get("version")

    if resver.version == version:
        return

    res_url = f"https://game.maj-soul.com/1/resversion{version}.json"
    response = requests.get(res_url, proxies={"https": None})
    response.raise_for_status()
    res_data: dict = response.json()

    emotes: defaultdict[str, list[int]] = defaultdict(list)
    pattern = rf"en\/extendRes\/emo\/e(\d+)\/(\d+)\.png"

    for text in res_data.get("res"):
        matches = re.search(pattern, text)

        if matches:
            charid = matches.group(1)
            emo = int(matches.group(2))

            if emo == 13:
                continue
            emotes[charid].append(emo)
    for value in emotes.values():
        value.sort()

    resver.version = version
    resver.emotes = {key: value[9:] for key, value in sorted(emotes.items())}

    with open(pathResVer, "w") as f:
        dump(asdict(resver), f)


def no_cheering_emotes():
    exclude = set(range(13, 19))
    for emo in resver.emotes.values():
        emo[:] = sorted(set(emo) - exclude)


def init():
    with console.status("[magenta]Fetch the latest server version") as status:
        fetch_resver()
    if conf.hook.no_cheering_emotes:
        no_cheering_emotes()
    if conf.mhm.pure_python_protobuf:
        environ["PROTOCOL_BUFFERS_PYTHON_IMPLEMENTATION"] = "python"

    with open(pathConf, "w") as f:
        dump(asdict(conf), f, indent=2)


# console
console = Console()


# logger
logger = getLogger(__name__)
logger.propagate = False
logger.setLevel(conf.mhm.log_level.upper())
logger.addHandler(RichHandler(markup=True, rich_tracebacks=True))
