import ctypes
import json
import locale
import os
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Self

import jsonschema
from jsonschema.exceptions import ValidationError

from akagi_ng.core.paths import ensure_dir, get_assets_dir, get_settings_dir
from akagi_ng.schema.constants import DEFAULT_GAME_URLS, Platform
from akagi_ng.settings.logger import logger

CONFIG_DIR: Path = ensure_dir(get_settings_dir())
SETTINGS_JSON_PATH: Path = CONFIG_DIR / "settings.json"

SCHEMA_PATH: Path = get_assets_dir() / "settings.schema.json"

# Windows 语言区域 ID（LCID）
LCID_ZH_CN = 2052  # 简体中文 (0x0804)
LCID_ZH_TW = 1028  # 繁体中文-台湾 (0x0404)
LCID_ZH_HK = 3076  # 繁体中文-香港 (0x0C04)
LCID_ZH_MO = 5124  # 繁体中文-澳门 (0x1404)
LCID_JA_JP = 1041  # 日文 (0x0411)


@dataclass(slots=True)
class OTConfig:
    online: bool
    server: str = ""
    api_key: str = ""


@dataclass(slots=True)
class MITMConfig:
    enabled: bool
    host: str
    port: int
    upstream: str


@dataclass(slots=True)
class ServerConfig:
    host: str
    port: int


@dataclass(slots=True)
class ModelConfig:
    temperature: float
    rule_based_agari_guard: bool
    model_4p: str = "mortal.pth"
    model_3p: str = "mortal3p.pth"


@dataclass(slots=True)
class Settings:
    log_level: str
    locale: str
    game_url: str
    platform: Platform
    mitm: MITMConfig
    server: ServerConfig
    ot: OTConfig
    model_config: ModelConfig

    def update(self, data: dict):
        """从字典更新设置"""
        _update_settings(self, data)
        self._validate_game_url()

    def __post_init__(self):
        self._validate_game_url()

    def _validate_game_url(self):
        """验证并修正 game_url"""
        is_mismatch = (self.platform == Platform.TENHOU and "maj-soul.com" in self.game_url) or (
            self.platform == Platform.MAJSOUL and "tenhou.net" in self.game_url
        )

        if not self.game_url or is_mismatch:
            self.game_url = DEFAULT_GAME_URLS.get(self.platform, DEFAULT_GAME_URLS[Platform.MAJSOUL])

    def save(self):
        """保存设置到 settings.json 文件"""
        _save_settings(asdict(self))
        logger.info(f"Saved settings to {SETTINGS_JSON_PATH}")

    @classmethod
    def from_dict(cls, data: dict) -> Self:
        """从字典创建 Settings 对象"""
        mitm_data = data.get("mitm", {})
        server_data = data.get("server", {})
        model_config_data = data.get("model_config", {})
        ot_data = data.get("ot", {})
        game_url = data.get("game_url", "")

        platform_val = data.get("platform")
        platform = Platform(platform_val) if platform_val else Platform.MAJSOUL

        return cls(
            log_level=data.get("log_level", "INFO"),
            locale=data.get("locale", "zh-CN"),
            game_url=game_url,
            platform=platform,
            mitm=MITMConfig(
                enabled=mitm_data.get("enabled", False),
                host=mitm_data.get("host", "127.0.0.1"),
                port=mitm_data.get("port", 6789),
                upstream=mitm_data.get("upstream", ""),
            ),
            server=ServerConfig(
                host=server_data.get("host", "127.0.0.1"),
                port=server_data.get("port", 8765),
            ),
            ot=OTConfig(
                online=ot_data.get("online", False),
                server=ot_data.get("server", ""),
                api_key=ot_data.get("api_key", ""),
            ),
            model_config=ModelConfig(
                model_4p=model_config_data.get("model_4p", "mortal.pth"),
                model_3p=model_config_data.get("model_3p", "mortal3p.pth"),
                temperature=model_config_data.get("temperature", 0.3),
                rule_based_agari_guard=model_config_data.get("rule_based_agari_guard", True),
            ),
        )


def _detect_locale_windows() -> str | None:
    """通过 Windows API 检测语言环境"""
    try:
        windll = ctypes.windll.kernel32
        lcid = windll.GetUserDefaultUILanguage()
        if lcid == LCID_ZH_CN:
            return "zh-CN"
        if lcid in (LCID_ZH_TW, LCID_ZH_HK, LCID_ZH_MO):
            return "zh-TW"
        if lcid == LCID_JA_JP:
            return "ja-JP"
    except (AttributeError, OSError) as e:
        logger.debug(f"Failed to detect locale via Windows API: {e}")
    return None


def _detect_locale_python() -> str | None:
    """通过 Python locale 模块检测语言环境"""
    try:
        sys_locale = locale.getlocale()[0]
        if sys_locale:
            if sys_locale.startswith("zh_CN"):
                return "zh-CN"
            if sys_locale.startswith(("zh_TW", "zh_HK")):
                return "zh-TW"
            if sys_locale.startswith("ja"):
                return "ja-JP"
    except Exception as e:
        logger.debug(f"Failed to detect locale via python locale: {e}")
    return None


def detect_system_locale() -> str:
    """
    检测系统语言环境,返回支持的语言之一:
    zh-CN, zh-TW, ja-JP, en-US。
    检测失败或不支持的语言默认返回 en-US。
    """
    # 优先使用 Windows API (如果在 Windows 上)
    if os.name == "nt":
        windows_locale = _detect_locale_windows()
        if windows_locale:
            return windows_locale

    # 回退到 Python locale
    python_locale = _detect_locale_python()
    if python_locale:
        return python_locale

    # 默认返回英语
    return "en-US"


def get_default_settings_dict() -> dict:
    return {
        "log_level": "INFO",
        "locale": detect_system_locale(),
        "game_url": DEFAULT_GAME_URLS[Platform.MAJSOUL],
        "platform": Platform.MAJSOUL.value,
        "mitm": {
            "enabled": False,
            "host": "127.0.0.1",
            "port": 6789,
            "upstream": "",
        },
        "server": {"host": "127.0.0.1", "port": 8765},
        "ot": {"online": False, "server": "http://127.0.0.1:5000", "api_key": "<YOUR_API_KEY>"},
        "model_config": {
            "model_4p": "mortal.pth",
            "model_3p": "mortal3p.pth",
            "temperature": 0.3,
            "rule_based_agari_guard": True,
        },
    }


def get_settings_dict() -> dict:
    """返回当前设置"""
    return asdict(local_settings)


def verify_settings(data: dict) -> bool:
    """根据 schema 验证设置"""
    try:
        jsonschema.validate(data, _get_schema())
        return True
    except ValidationError as e:
        logger.error(f"Settings validation error: {e.message}")
        return False


def _load_settings() -> Settings:
    """
    加载并验证设置。
    - 检查 schema 文件是否存在
    - 从 CONFIG_DIR 读取 settings.json
    - 如果 settings.json 损坏，备份并重建默认设置

    Raises:
        FileNotFoundError: schema 不存在
    """
    # 验证 schema 文件存在
    schema = _get_schema()

    if not SETTINGS_JSON_PATH.exists():
        logger.warning(f"{SETTINGS_JSON_PATH} not found. Creating a default {SETTINGS_JSON_PATH}.")
        with open(SETTINGS_JSON_PATH, "w", encoding="utf-8") as f:
            json.dump(get_default_settings_dict(), f, indent=4, ensure_ascii=False)

    try:
        with open(SETTINGS_JSON_PATH, encoding="utf-8") as f:
            loaded_settings = json.load(f)
        jsonschema.validate(loaded_settings, schema)
    except json.JSONDecodeError as e:
        loaded_settings = _backup_and_reset_settings(f"settings.json corrupted: {e}")
    except ValidationError as e:
        loaded_settings = _backup_and_reset_settings(f"settings.json validation failed: {e.message}")

    return Settings.from_dict(loaded_settings)


def _get_schema() -> dict:
    """获取 settings.json 的 schema"""
    if not SCHEMA_PATH.exists():
        raise FileNotFoundError(f"settings.schema.json not found at {SCHEMA_PATH}")
    with open(SCHEMA_PATH, encoding="utf-8") as f:
        return json.load(f)


def _update_settings(settings: Settings, data: dict):
    """从字典更新 Settings 对象"""
    settings.log_level = data.get("log_level", "INFO")
    settings.locale = data.get("locale", "zh-CN")
    settings.game_url = data.get("game_url", "")
    settings.platform = Platform(data.get("platform", Platform.MAJSOUL))

    mitm_data = data.get("mitm", {})
    settings.mitm.enabled = mitm_data.get("enabled", False)
    settings.mitm.host = mitm_data.get("host", "127.0.0.1")
    settings.mitm.port = mitm_data.get("port", 6789)
    settings.mitm.upstream = mitm_data.get("upstream", "")

    server_data = data.get("server", {})
    settings.server.host = server_data.get("host", "127.0.0.1")
    settings.server.port = server_data.get("port", 8765)

    model_config_data = data.get("model_config", {})
    settings.model_config.model_4p = model_config_data.get("model_4p", "mortal.pth")
    settings.model_config.model_3p = model_config_data.get("model_3p", "mortal3p.pth")
    settings.model_config.temperature = model_config_data.get("temperature", 0.3)
    settings.model_config.rule_based_agari_guard = model_config_data.get("rule_based_agari_guard", True)

    ot_data = data.get("ot", {})
    settings.ot.online = ot_data.get("online", False)
    settings.ot.server = ot_data.get("server", "")
    settings.ot.api_key = ot_data.get("api_key", "")


def _save_settings(data: dict):
    """保存 settings.json"""
    with open(SETTINGS_JSON_PATH, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=4, ensure_ascii=False)


def _backup_and_reset_settings(reason: str) -> dict:
    """
    备份当前设置文件并重建默认值。
    返回默认设置字典。
    """
    logger.error(reason)
    bak_path = SETTINGS_JSON_PATH.with_suffix(".json.bak")
    logger.warning(f"Backup settings.json to {bak_path}")

    if SETTINGS_JSON_PATH.exists():
        os.replace(SETTINGS_JSON_PATH, bak_path)

    logger.warning("Creating new settings.json with default values")
    default_settings = get_default_settings_dict()
    with open(SETTINGS_JSON_PATH, "w", encoding="utf-8") as f:
        json.dump(default_settings, f, indent=4, ensure_ascii=False)

    return default_settings


local_settings: Settings = _load_settings()
