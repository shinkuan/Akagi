import json
from dataclasses import asdict, dataclass, field, fields, is_dataclass
from pathlib import Path


ROOT = Path(".")

CONFIG_PATH = ROOT / "configs" / "mitm.json"


@dataclass
class Config:
    @dataclass
    class XMLRPC:
        host: str = "127.0.0.1"
        port: int = 7879

    @dataclass
    class Playwright:
        enable: bool = False
        width: int = 1280
        height: int = 720

    xmlrpc: XMLRPC = field(default_factory=lambda: Config.XMLRPC())
    playwright: Playwright = field(default_factory=lambda: Config.Playwright())

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
    json.dump(asdict(config), f, indent=2, ensure_ascii=False)
