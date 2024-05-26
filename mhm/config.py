import json
from dataclasses import asdict, dataclass, field, fields, is_dataclass
from pathlib import Path

ROOT = Path(".")

CONFIG_PATH = ROOT / "mhmp.json"
PROXIN_PATH = ROOT / "proxinject/proxinjector-cli.exe"


@dataclass
class Config:
    @dataclass
    class Base:
        skins: bool = True
        aider: bool = False
        chest: bool = False
        yongchang_mode: bool = False
        random_star_char: bool = False
        no_cheering_emotes: bool = True

    @dataclass
    class Mitmdump:
        dump: dict = field(
            default_factory=lambda: {"with_dumper": False, "with_termlog": True}
        )
        args: dict = field(
            default_factory=lambda: {"http2": False, "mode": ["socks5@127.0.0.1:7070"]}
        )

    @dataclass
    class Proxinject:
        path: str = field(
            default_factory=lambda: str(PROXIN_PATH) if PROXIN_PATH.exists() else None
        )
        args: dict = field(
            default_factory=lambda: {
                "name": "jantama_mahjongsoul",
                "set-proxy": "127.0.0.1:7070",
            }
        )

    base: Base = field(default_factory=lambda: Config.Base())
    mitmdump: Mitmdump = field(default_factory=lambda: Config.Mitmdump())
    proxinject: Proxinject = field(default_factory=lambda: Config.Proxinject())

    @classmethod
    def fromdict(cls, data: dict):
        try:
            for field in fields(cls):
                if is_dataclass(field.type) and field.name in data:
                    data[field.name] = field.type(**data[field.name])
            return cls(**data)
        except (TypeError, KeyError):
            print("Configuration file is outdated, please delete it manually")
            raise


if CONFIG_PATH.exists():
    with CONFIG_PATH.open("r", encoding="utf-8") as f:
        config = Config.fromdict(json.load(f))
else:
    config = Config()
with CONFIG_PATH.open("w", encoding="utf-8") as f:
    json.dump(asdict(config), f, indent=2)
