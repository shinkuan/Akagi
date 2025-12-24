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
        "dataserver": True,
        "tui": True,
        "autoplay": False,
        "auto_switch_model": True,
        "autoplay_time": {
            "first_tile": 5.0,
            "rand_min": 1.0,
            "rand_max": 3.0,
            "candidate": 0.5,
        },
        "recommendation_temperature": 0.3,
    }


def ensure_default_settings(settings: dict) -> bool:
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
class OTConfig:
    server: str
    online: bool
    api_key: str


@dataclasses.dataclass
class AutoplayTimeConfig:
    first_tile: float
    rand_min: float
    rand_max: float
    candidate: float


@dataclasses.dataclass
class Settings:
    mitm: MITMConfig
    theme: str
    model: str
    ot: OTConfig
    dataserver: bool
    tui: bool
    autoplay: bool
    auto_switch_model: bool
    autoplay_time: AutoplayTimeConfig
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
        self.dataserver = settings.get("dataserver", True)
        self.tui = settings.get("tui", True)
        self.autoplay = settings["autoplay"]
        self.auto_switch_model = settings["auto_switch_model"]
        self.autoplay_time = AutoplayTimeConfig(
            first_tile=settings["autoplay_time"]["first_tile"],
            rand_min=settings["autoplay_time"]["rand_min"],
            rand_max=settings["autoplay_time"]["rand_max"],
            candidate=settings["autoplay_time"]["candidate"],
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
                json.dump(
                    {
                        "server": self.ot.server,
                        "online": self.ot.online,
                        "api_key": self.ot.api_key,
                    },
                    f,
                    indent=4,
                )
            logger.info(f"Updated {ot_setting} with new settings")

        if ot_setting_3p.exists():
            # If the file exists, update it with the new settings
            with open(ot_setting_3p, "w") as f:
                json.dump(
                    {
                        "server": self.ot.server,
                        "online": self.ot.online,
                        "api_key": self.ot.api_key,
                    },
                    f,
                    indent=4,
                )
            logger.info(f"Updated {ot_setting_3p} with new settings")

    def save(self) -> None:
        """
        Save the settings to the settings.json file
        """
        with open(FILE_PATH / "settings.json", "w") as f:
            json.dump(
                {
                    "mitm": {
                        "type": self.mitm.type.value,
                        "host": self.mitm.host,
                        "port": self.mitm.port,
                    },
                    "model": self.model,
                    "theme": self.theme,
                    "ot_server": {
                        "server": self.ot.server,
                        "online": self.ot.online,
                        "api_key": self.ot.api_key,
                    },
                    "dataserver": self.dataserver,
                    "tui": self.tui,
                    "autoplay": self.autoplay,
                    "auto_switch_model": self.auto_switch_model,
                    "autoplay_time": {
                        "first_tile": self.autoplay_time.first_tile,
                        "rand_min": self.autoplay_time.rand_min,
                        "rand_max": self.autoplay_time.rand_max,
                        "candidate": self.autoplay_time.candidate,
                    },
                    "recommendation_temperature": self.recommendation_temperature,
                },
                f,
                indent=4,
            )
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

    settings_path = FILE_PATH / "settings.json"
    schema_path = FILE_PATH / "settings.schema.json"

    if not schema_path.exists():
        raise FileNotFoundError("settings.schema.json not found")

    settings = None

    # Create default settings if missing or empty
    if (not settings_path.exists()) or settings_path.stat().st_size == 0:
        logger.warning("settings.json missing or empty, creating default settings.json")
        default_settings = get_default_settings()
        with open(settings_path, "w") as f:
            json.dump(default_settings, f, indent=4)
        settings = copy.deepcopy(default_settings)

    if settings is None:
        try:
            with open(settings_path, "r") as f:
                settings = json.load(f)
        except json.JSONDecodeError as e:
            logger.error(f"settings.json corrupted: {e}")
            logger.warning("Backup settings.json to settings.json.bak")
            os.rename(settings_path, FILE_PATH / "settings.json.bak")
            logger.warning("Creating new settings.json")
            default_settings = get_default_settings()
            with open(settings_path, "w") as f:
                json.dump(default_settings, f, indent=4)
            logger.info(f"Created new settings.json with default values")
            settings = copy.deepcopy(default_settings)

    defaults_added = ensure_default_settings(settings)

    # Load schema
    with open(schema_path, "r") as f:
        schema = json.load(f)

    # Validate settings
    # This will raise an exception if the settings are invalid
    jsonschema.validate(settings, schema)
    if defaults_added:
        save_settings(settings)
        logger.info(
            f"Updated {FILE_PATH / 'settings.json'} with missing default values"
        )

    # Parse settings
    return Settings(
        mitm=MITMConfig(
            host=settings["mitm"]["host"],
            port=settings["mitm"]["port"],
            type=MITMType(settings["mitm"]["type"]),
        ),
        theme=settings["theme"],
        model=settings["model"],
        ot=OTConfig(
            server=settings["ot_server"]["server"],
            online=settings["ot_server"]["online"],
            api_key=settings["ot_server"]["api_key"],
        ),
        dataserver=settings["dataserver"],
        tui=settings["tui"],
        autoplay=settings["autoplay"],
        auto_switch_model=settings["auto_switch_model"],
        autoplay_time=AutoplayTimeConfig(
            first_tile=settings["autoplay_time"]["first_tile"],
            rand_min=settings["autoplay_time"]["rand_min"],
            rand_max=settings["autoplay_time"]["rand_max"],
            candidate=settings["autoplay_time"]["candidate"],
        ),
        recommendation_temperature=settings["recommendation_temperature"],
    )


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
