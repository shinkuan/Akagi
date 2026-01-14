import os
import json
import copy
import jsonschema
from jsonschema.exceptions import ValidationError
import dataclasses
from enum import Enum
from pathlib import Path
from .logger import logger

FILE_PATH = Path(__file__).resolve().parent


def get_default_settings() -> dict:
    """
    Default settings template. A fresh copy should be used each time to avoid mutation.
    """
    return {
        "mitm": {"type": "majsoul", "host": "127.0.0.1", "port": 7880},
        "model": "mortal",
        "theme": "textual-dark",
        "ot_server": {
            "server": "http://127.0.0.1:5000",
            "online": False,
            "api_key": "your_api_key",
        },
        "dataserver": {"enable": True, "host": "0.0.0.0", "port": 7881},
        "autoplay": False,
        "auto_switch_model": True,
        "autoplay_thinker": "default",
        "default_thinker": {
            "max_delay": 8.0,
            "min_delay": 1.0,
            "add_random_delay": True,
            "add_random_delay_min": 1.0,
            "add_random_delay_max": 3.0,
            "first_tile_discard": 4.0,
            "can_riichi_think": 2.0,
            "close_q_threshold": 0.005,
            "two_close_q_think": 2.0,
            "confident_q_threshold": 0.995,
            "confident_q_max_delay": 2.0,
            "kan_think": 0.5,
        },
        "recommendation_temperature": 0.1,
    }


def fill_defaults(settings: dict) -> bool:
    """
    Ensure required settings exist, filling missing entries with defaults.

    Args:
        settings (dict): Settings dictionary to patch.

    Returns:
        bool: True if any defaults were added.
    """
    default_settings = get_default_settings()
    updated = False

    for key, default_value in default_settings.items():
        if key not in settings or settings.get(key) is None:
            settings[key] = copy.deepcopy(default_value)
            updated = True
            continue

        if isinstance(default_value, dict):
            if not isinstance(settings[key], dict):
                settings[key] = copy.deepcopy(default_value)
                updated = True
                continue

            for sub_key, sub_value in default_value.items():
                if sub_key not in settings[key] or settings[key].get(sub_key) is None:
                    settings[key][sub_key] = copy.deepcopy(sub_value)
                    updated = True

    return updated


class MITMType(Enum):
    AMATSUKI = "amatsuki"
    MAJSOUL = "majsoul"
    RIICHI_CITY = "riichi_city"
    TENHOU = "tenhou"
    UNIFIED = "unified"

@dataclasses.dataclass
class ServiceConfig:
    host: str
    port: int

@dataclasses.dataclass
class MITMConfig(ServiceConfig):
    type: MITMType

@dataclasses.dataclass
class DataServerConfig:
    enable: bool
    host: str
    port: int

@dataclasses.dataclass
class OTConfig:
    server: str
    online: bool
    api_key: str

@dataclasses.dataclass
class DefaultThinkerConfig:
    max_delay: float
    min_delay: float
    add_random_delay: bool
    add_random_delay_min: float
    add_random_delay_max: float
    first_tile_discard: float
    can_riichi_think: float
    close_q_threshold: float
    two_close_q_think: float
    confident_q_threshold: float
    confident_q_max_delay: float
    kan_think: float

@dataclasses.dataclass
class Settings:
    mitm: MITMConfig
    theme: str
    model: str
    ot: OTConfig
    dataserver: DataServerConfig
    autoplay: bool
    auto_switch_model: bool
    autoplay_thinker: str
    default_thinker: DefaultThinkerConfig
    recommendation_temperature: float
    def update(self, settings: dict) -> None:
        """
        Update settings from a dictionary

        Args:
            settings (dict): Dictionary with settings to update
        """
        self.mitm.host = settings["mitm"]["host"]
        self.mitm.port = settings["mitm"]["port"]
        self.mitm.type = MITMType(settings["mitm"]["type"])
        self.theme = settings["theme"]
        self.model = settings["model"]
        self.ot.server = settings["ot_server"]["server"]
        self.ot.online = settings["ot_server"]["online"]
        self.ot.api_key = settings["ot_server"]["api_key"]
        self.dataserver.enable = settings["dataserver"]["enable"]
        self.dataserver.host = settings["dataserver"]["host"]
        self.dataserver.port = int(settings["dataserver"]["port"])
        self.autoplay = settings["autoplay"]
        self.auto_switch_model = settings["auto_switch_model"]
        self.autoplay_thinker = settings["autoplay_thinker"]
        self.default_thinker = DefaultThinkerConfig(
            max_delay=settings["default_thinker"]["max_delay"],
            min_delay=settings["default_thinker"]["min_delay"],
            add_random_delay=settings["default_thinker"]["add_random_delay"],
            add_random_delay_min=settings["default_thinker"]["add_random_delay_min"],
            add_random_delay_max=settings["default_thinker"]["add_random_delay_max"],
            first_tile_discard=settings["default_thinker"]["first_tile_discard"],
            can_riichi_think=settings["default_thinker"]["can_riichi_think"],
            close_q_threshold=settings["default_thinker"]["close_q_threshold"],
            two_close_q_think=settings["default_thinker"]["two_close_q_think"],
            confident_q_threshold=settings["default_thinker"]["confident_q_threshold"],
            confident_q_max_delay=settings["default_thinker"]["confident_q_max_delay"],
            kan_think=settings["default_thinker"]["kan_think"]
        )
        self.recommendation_temperature = settings["recommendation_temperature"]
        self.save_ot_settings()

    def save_ot_settings(self) -> None:
        """
        Save the OT settings to the ot_settings.json file in the mjai_bot directory
        """
        # Find the ot_settings.json file
        ot_setting = Path.cwd() / "mjai_bot" / "mortal" / "ot_settings.json"
        ot_setting_3p = Path.cwd() / "mjai_bot" / "mortal3p" / "ot_settings.json"

        if ot_setting.exists():
            # If the file exists, update it with the new settings
            with open(ot_setting, "w") as f:
                json.dump({
                    "server": self.ot.server,
                    "online": self.ot.online,
                    "api_key": self.ot.api_key
                }, f, indent=4)
            logger.info(f"Updated {ot_setting} with new settings")

        if ot_setting_3p.exists():
            # If the file exists, update it with the new settings
            with open(ot_setting_3p, "w") as f:
                json.dump({
                    "server": self.ot.server,
                    "online": self.ot.online,
                    "api_key": self.ot.api_key
                }, f, indent=4)
            logger.info(f"Updated {ot_setting_3p} with new settings")

    def save(self) -> None:
        """
        Save the settings to the settings.json file
        """
        with open(FILE_PATH / "settings.json", "w") as f:
            json.dump({
                "mitm": {
                    "type": self.mitm.type.value,
                    "host": self.mitm.host,
                    "port": self.mitm.port
                },
                "model": self.model,
                "theme": self.theme,
                "ot_server": {
                    "server": self.ot.server,
                    "online": self.ot.online,
                    "api_key": self.ot.api_key
                },
                "dataserver": {
                    "enable": self.dataserver.enable,
                    "host": self.dataserver.host,
                    "port": self.dataserver.port
                },
                "autoplay": self.autoplay,
                "auto_switch_model": self.auto_switch_model,
                "autoplay_thinker": self.autoplay_thinker,
                "default_thinker": {
                    "max_delay": self.default_thinker.max_delay,
                    "min_delay": self.default_thinker.min_delay,
                    "add_random_delay": self.default_thinker.add_random_delay,
                    "add_random_delay_min": self.default_thinker.add_random_delay_min,
                    "add_random_delay_max": self.default_thinker.add_random_delay_max,
                    "first_tile_discard": self.default_thinker.first_tile_discard,
                    "can_riichi_think": self.default_thinker.can_riichi_think,
                    "close_q_threshold": self.default_thinker.close_q_threshold,
                    "two_close_q_think": self.default_thinker.two_close_q_think,
                    "confident_q_threshold": self.default_thinker.confident_q_threshold,
                    "confident_q_max_delay": self.default_thinker.confident_q_max_delay,
                    "kan_think": self.default_thinker.kan_think
                },
                "recommendation_temperature": self.recommendation_temperature
            }, f, indent=4)
        # Save the settings to the file
        logger.info(f"Saved settings to {FILE_PATH / 'settings.json'}")
        logger.info(f"Updated {FILE_PATH / 'settings.json'} with new settings")

def load_settings() -> Settings:
    """
    Load settings from settings.json and validate them against settings.schema.json

    Raises:
        FileNotFoundError: settings.json or settings.schema.json not found
        jsonschema.exceptions.ValidationError: settings.json does not match settings.schema.json

    Returns:
        Settings: Parsed settings
    """

    # Check file exists
    if not (FILE_PATH / "settings.json").exists():
        raise FileNotFoundError("settings.json not found")
    if not (FILE_PATH / "settings.schema.json").exists():
        raise FileNotFoundError("settings.schema.json not found")
    
    defaults_added = False
    try:
        # Load settings
        with open(FILE_PATH / "settings.json", "r") as f:
            settings = json.load(f)
    except json.JSONDecodeError as e:
        logger.error(f"settings.json corrupted: {e}")
        logger.warning("Backup settings.json to settings.json.bak")
        os.rename(FILE_PATH / "settings.json", FILE_PATH / "settings.json.bak")
        logger.warning("Creating new settings.json")
        default_settings = get_default_settings()
        with open(FILE_PATH / "settings.json", "w") as f:
            json.dump(default_settings, f, indent=4)
        logger.info(f"Created new settings.json with default values")
        settings = copy.deepcopy(default_settings)

    defaults_added = fill_defaults(settings)

    # Load schema
    with open(FILE_PATH / "settings.schema.json", "r") as f:
        schema = json.load(f)

    # Validate settings
    # This will raise an exception if the settings are invalid
    jsonschema.validate(settings, schema)
    if defaults_added:
        save_settings(settings)
        logger.info(f"Updated {FILE_PATH / 'settings.json'} with missing default values")

    # Parse settings
    settings_obj = Settings(
        mitm=MITMConfig(
            host=settings["mitm"]["host"],
            port=settings["mitm"]["port"],
            type=MITMType(settings["mitm"]["type"])
        ),
        theme=settings["theme"],
        model=settings["model"],
        ot=OTConfig(
            server=settings["ot_server"]["server"],
            online=settings["ot_server"]["online"],
            api_key=settings["ot_server"]["api_key"]
        ),
        dataserver=DataServerConfig(
            enable=settings["dataserver"]["enable"],
            host=settings["dataserver"]["host"],
            port=int(settings["dataserver"]["port"]),
        ),
        autoplay=settings["autoplay"],
        auto_switch_model=settings["auto_switch_model"],
        autoplay_thinker=settings["autoplay_thinker"],
        default_thinker=DefaultThinkerConfig(
            max_delay=settings["default_thinker"]["max_delay"],
            min_delay=settings["default_thinker"]["min_delay"],
            add_random_delay=settings["default_thinker"]["add_random_delay"],
            add_random_delay_min=settings["default_thinker"]["add_random_delay_min"],
            add_random_delay_max=settings["default_thinker"]["add_random_delay_max"],
            first_tile_discard=settings["default_thinker"]["first_tile_discard"],
            can_riichi_think=settings["default_thinker"]["can_riichi_think"],
            close_q_threshold=settings["default_thinker"]["close_q_threshold"],
            two_close_q_think=settings["default_thinker"]["two_close_q_think"],
            confident_q_threshold=settings["default_thinker"]["confident_q_threshold"],
            confident_q_max_delay=settings["default_thinker"]["confident_q_max_delay"],
            kan_think=settings["default_thinker"]["kan_think"]
        ),
        recommendation_temperature=settings["recommendation_temperature"]
    )
    settings_obj.save_ot_settings()
    return settings_obj

def get_schema() -> dict:
    """
    Get the schema for settings.json

    Returns:
        dict: Schema for settings.json
    """
    with open(FILE_PATH / "settings.schema.json", "r") as f:
        return json.load(f)
    
def get_settings() -> dict:
    """
    Get the settings.json

    Returns:
        dict: settings.json
    """
    with open(FILE_PATH / "settings.json", "r") as f:
        return json.load(f)
    
def save_settings(settings: dict) -> None:
    """
    Save the settings.json

    Args:
        settings (dict): settings.json
    """
    with open(FILE_PATH / "settings.json", "w") as f:
        json.dump(settings, f, indent=4)

def verify_settings(settings: dict) -> bool:
    """
    Verify the settings.json against the schema

    Args:
        settings (dict): settings.json

    Returns:
        bool: True if valid, False otherwise
    """
    try:
        jsonschema.validate(settings, get_schema())
        return True
    except ValidationError as e:
        logger.error(f"Settings validation error: {e.message}")
        return False

settings: Settings = load_settings()
