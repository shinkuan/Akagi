import os
os.environ["LOGURU_AUTOINIT"] = "False"
import re
import sys
import json
import time
import atexit
import random
import pathlib
import traceback
import jsonschema
import subprocess
from pathlib import Path
from sys import executable
from threading import Thread
from functools import partial
from datetime import datetime

from rich.text import Text
from textual import on
from textual.app import App, ComposeResult
from textual.containers import Horizontal, ScrollableContainer, Vertical
from textual.css.query import NoMatches
from textual.message import Message
from textual.events import Event, ScreenResume
from textual.screen import Screen
from textual.coordinate import Coordinate
from textual.theme import Theme
from textual.widget import Widget
from textual.widgets import (Button, Checkbox, Footer, Header, Input, Label, Select, Switch,
                             LoadingIndicator, Log, Markdown, Pretty, Rule, Tabs, Tab,
                             Digits, Static, RichLog, DataTable, ContentSwitcher,
                             Markdown, MarkdownViewer)

from .logger import logger
from .misc import TILE_2_UNICODE_ART_RICH, VERTICAL_RULE, EMPTY_VERTICAL_RULE, ADDITIONAL_THEMES
from .libriichi_helper import meta_to_recommend
from mitm.client import Client
from mjai_bot.bot import AkagiBot
from mjai_bot.controller import Controller
from autoplay.autoplay import AutoPlay
from settings import MITMType, Settings, load_settings, get_settings, get_schema, verify_settings, save_settings
from settings.settings import settings

mitm_client: Client = None
mjai_controller: Controller = None
mjai_bot: AkagiBot = None
autoplay: AutoPlay = None

# ============================================= #
#               Settings Screen                 #
# ============================================= #
class SettingsScreen(Screen):
    BINDINGS = [("escape", "app.pop_screen", "back")]
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        pass

    def compose(self) -> ComposeResult:
        # Vertical(
        #     Horizontal(
        #         Label("type: ", classes="settings_key"),
        #         Select.from_values(["amatsuki", "majsoul", "tenhou"], value=settings["mitm"]["type"], id="settings_mitm_type"),
        #         classes="settings_row",
        #     ),
        #     Horizontal(
        #         Label("host: ", classes="settings_key"),
        #         Input(value=settings["mitm"]["host"], id="settings_mitm_host"),
        #         classes="settings_row",
        #     ),
        #     Horizontal(
        #         Label("port: ", classes="settings_key"),
        #         Input(value=str(settings["mitm"]["port"]), id="settings_mitm_port"),
        #         classes="settings_row",
        #     ),
        #     classes="settings_vertical_container",
        #     id="settings_mitm",
        # ),
        # Horizontal(
        #     Label("theme: ", classes="settings_key"),
        #     Input(value=settings["theme"], id="settings_theme"),
        #     classes="settings_row",
        # ),
        # Horizontal(
        #     Label("model: ", classes="settings_key"),
        #     Input(value=settings["model"], id="settings_model"),
        #     classes="settings_row",
        # ),
        schema = get_schema()
        settings = get_settings()
        scrollable_container = ScrollableContainer(
            self.generate_settings_ui(schema, settings, "", "settings", is_outer=True),
            id="settings__scrollable_container",
        )
        scrollable_container.border_title = "Settings"
        yield scrollable_container
        
    def generate_settings_ui(self, schema: dict, settings: dict, previous_names: str, name: str, is_outer=False) -> Widget:
        match schema["type"]:
            case "object":
                containers = []
                for key, value in schema["properties"].items():
                    if key not in settings:
                        raise ValueError(f"Invalid settings: {key} not found")
                    containers.append(self.generate_settings_ui(value, settings[key], f"{previous_names}{name}-", key))
                if is_outer:
                    containers.append(
                        Horizontal(
                            Button("Save", variant="success", id="settings_save_button"),
                            Button("Cancel", variant="error", id="settings_cancel_button"),
                            id="settings_button_container",
                        )
                    )
                    vertical_container = Vertical(
                        *containers,
                        classes="settings__vertical_container_outer",
                        id=f"{previous_names}{name}",
                    )
                    vertical_container.border_title = name
                else:
                    vertical_container = Vertical(
                        *containers,
                        classes="settings__vertical_container",
                        id=f"{previous_names}{name}",
                    )
                    vertical_container.border_title = name
                return vertical_container
            case "string":
                if "enum" in schema:
                    return Horizontal(
                        Label(f"{name}: ", classes="settings__key"),
                        Select.from_values(schema["enum"], value=settings, id=f"{previous_names}{name}", classes="settings__select"),
                        classes="settings__row",
                        id=f"{previous_names}{name}",
                    )
                else:
                    return Horizontal(
                        Label(f"{name}: ", classes="settings__key"),
                        Input(value=settings, id=f"{previous_names}{name}", type="text", classes="settings__input"),
                        classes="settings__row",
                        id=f"{previous_names}{name}",
                    )
            case "number":
                return Horizontal(
                    Label(f"{name}: ", classes="settings__key"),
                    Input(value=str(settings), id=f"{previous_names}{name}", type="number", classes="settings__input"),
                    classes="settings__row",
                    id=f"{previous_names}{name}",
                )
            case "integer":
                return Horizontal(
                    Label(f"{name}: ", classes="settings__key"),
                    Input(value=str(settings), id=f"{previous_names}{name}", type="integer", classes="settings__input"),
                    classes="settings__row",
                    id=f"{previous_names}{name}",
                )
            case "boolean":
                return Horizontal(
                    Label(f"{name}: ", classes="settings__key"),
                    Switch(value=settings, id=f"{previous_names}{name}", classes="settings__switch"),
                    classes="settings__row",
                    id=f"{previous_names}{name}",
                )
            case "array":
                raise ValueError(f"Invalid schema: {schema['type']} is not a valid type")
            case _:
                raise ValueError(f"Invalid schema: {schema['type']} is not a valid type")

    def get_settings(self) -> dict:
        """
        Get settings from the UI.
        """
        settings = {}
        settings_scrollable_container = self.query_one("#settings__scrollable_container")
        for child in settings_scrollable_container.children:
            if isinstance(child, Vertical):
                key = child.id.split("-")[-1]
                settings[key] = self.get_settings_from_vertical(child)
            elif isinstance(child, Horizontal):
                key = child.id.split("-")[-1]
                settings[key] = self.get_settings_from_horizontal(child)
        return settings
    
    def get_settings_from_vertical(self, vertical: Vertical) -> dict:
        settings = {}
        for child in vertical.children:
            if isinstance(child, Vertical):
                key = child.id.split("-")[-1]
                settings[key] = self.get_settings_from_vertical(child)
            elif isinstance(child, Horizontal):
                if child.id == "settings_button_container":
                    continue
                key = child.id.split("-")[-1]
                settings[key] = self.get_settings_from_horizontal(child)
        return settings
    
    def get_settings_from_horizontal(self, horizontal: Horizontal) -> str | int | bool | float | None:
        for child in horizontal.children:
            if isinstance(child, Label):
                continue
            elif isinstance(child, Select):
                return str(child.value)
            elif isinstance(child, Input):
                if child.type == "number":
                    return float(child.value)
                elif child.type == "integer":
                    return int(child.value)
                elif child.type == "string":
                    return str(child.value)
                elif child.type == "text":
                    return str(child.value)
            elif isinstance(child, Switch):
                return bool(child.value)
            else:
                raise ValueError(f"Invalid settings: {child.id} is not a valid type")

    @on(Button.Pressed, "#settings_save_button")
    def settings_save_button_clicked(self) -> None:
        """Handle Button.Pressed message sent by Save button."""
        global settings, mjai_controller, mitm_client, autoplay
        local_settings = self.get_settings()["settings"]
        logger.info(f"Verifying settings: {local_settings}")
        try:
            jsonschema.validate(local_settings, get_schema())
            logger.info("Settings are valid, saving...")
            save_settings(local_settings)
            logger.info("Settings saved")
            notify_restart_mitm = (
                (local_settings["mitm"]["type"] != settings.mitm.type.value) and
                mitm_client.running
            )
            update_model = local_settings["model"] != settings.model
            # Reload settings
            settings.update(get_settings())
            self.app.notify(
                "Settings saved successfully, restart is required to apply changes.",
                title="Settings Saved",
                severity="information",
            )
            if update_model:
                if mjai_controller.choose_bot_name(settings.model):
                    logger.info(f"Selected model: {settings.model}")
                else:
                    logger.error(f"Failed to select model: {settings.model}")
                    self.app.notify(
                        f"Failed to select model: {settings.model}\n"
                        "Please check the model name and try again.",
                        title="Model Error",
                        severity="error",
                    )
            if notify_restart_mitm:
                self.app.notify(
                    "MITM settings changed, you need to restart MITM client for the changes to take effect.",
                    title="MITM Settings Changed",
                    severity="information",
                )
            if settings.autoplay:
                if settings.mitm.type.value in ["tenhou", "unified"]:
                    self.app.notify(
                        f"Autoplay does not support {settings.mitm.type.value} yet, please disable it.",
                        title="Autoplay Warning",
                        severity="warning",
                    )
                else:
                    autoplay.set_autoplay()
                    window = autoplay.auto_select_window()
                    if window is not None:
                        logger.info(f"Autoplay window found: {window.name}")
                        self.app.notify(
                            f"Autoplay window found: {window.name}",
                            title="Autoplay",
                            severity="information",
                        )
                    else:
                        logger.warning("No autoplay window found")
                        self.app.notify(
                            "No autoplay window found, select a target window manually",
                            title="Autoplay",
                            severity="warning",
                        )
            self.app.pop_screen()
        except Exception as e:
            logger.error("Settings are invalid, not saving")
            self.app.notify(
                "Settings are invalid, not saving. \n"
                f"Error: {traceback.format_exc()}",
                title="Settings Error",
                severity="error",
            )
            logger.info(f"Settings: {local_settings} are invalid")
            logger.error(f"Settings error: {e}")

    @on(Button.Pressed, "#settings_cancel_button")
    def settings_cancel_button_clicked(self) -> None:
        """Handle Button.Pressed message sent by Cancel button."""
        self.app.pop_screen()

# ============================================= #
#                Models Screen                  #
# ============================================= #
class ModelsScreen(Screen):
    BINDINGS = [("escape", "app.pop_screen", "back")]
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.windows: list = []

    def on_mount(self) -> None:
        pass

    def compose(self) -> ComposeResult:
        global mjai_controller, autoplay, settings
        self.windows = autoplay.get_windows()
        windows_names = [window.name for window in self.windows]
        yield ScrollableContainer(
            Static("Models", id="models_label"),
            Select.from_values(mjai_controller.available_bots_names, id="models_select"),
            Static("Warning: This will restart the bot, do not change model during a match!", id="models_warning"),
            Static("Warning: To play 3P Mahjong, a 3P model is needed.", id="models_warning2"),
            Static("Autoplay Window", id="autoplay_window_label"),
            Select.from_values(windows_names, id="models_window_select"),
            Button("Select", variant="primary", id="models_select_button"),
            id="models_select_container",
        )

    @on(Button.Pressed, "#models_select_button")
    def models_select_button_clicked(self) -> None:
        """Handle Button.Pressed message sent by Select button."""
        global mjai_controller, settings, autoplay

        models_select: Select = self.query_one("#models_select")
        selected_model = models_select.value
        if selected_model != Select.BLANK:
            selected_model_index = mjai_controller.available_bots_names.index(selected_model)
            if mjai_controller.choose_bot_index(selected_model_index):
                logger.info(f"Selected model: {selected_model}")
                settings.model = selected_model
                settings.save()
            else:
                logger.error(f"Failed to select model: {selected_model}")
        
        selected_window: Select = self.query_one("#models_window_select")
        selected_window_name = selected_window.value
        if selected_window_name != Select.BLANK:
            for i, window in enumerate(self.windows):
                if window.name == selected_window_name:
                    autoplay.select_window(window.hwnd)
                    break

        self.app.pop_screen()

# ============================================= #
#                 Logs Screen                   #
# ============================================= #
class LogsScreen(Screen):
    BINDINGS = [("escape", "app.pop_screen", "back")]
    def __init__(self, *args, **kwargs):
        self.log_names: set[str] = set()
        self.log_paths: dict[str, tuple[datetime, Path]] = {}
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        pass

    def compose(self) -> ComposeResult:
        self.update_files()
        tabs = [
            Tab(label=name, id=f"logs_tab_{name}", disabled=False) for name in sorted(self.log_names)
        ]
        yield Header(name="Logs")
        yield Tabs(*tabs, id="logs_tabs")
        yield RichLog(
            max_lines=1000, min_width=80, wrap=False,
            highlight=True, markup=True, auto_scroll=True,
            id="logs_log"
        )
        yield Button("Refresh", variant="primary", id="logs_refresh_button")
        yield Footer()

    def update_files(self, log_dir: str = "./logs") -> None:
        """
        Update the list of log files.
        """
        if not os.path.isdir(log_dir):
            logger.error(f"Log directory {log_dir} does not exist")
            return

        pattern = re.compile(r"(.+)_(\d{8})_(\d{6})\.log$")

        for file_name in os.listdir(log_dir):
            if not file_name.endswith(".log"):
                continue

            match = pattern.match(file_name)
            if not match:
                continue

            name = match.group(1)
            date_str = match.group(2)
            time_str = match.group(3)
            timestamp = datetime.strptime(f"{date_str}_{time_str}", "%Y%m%d_%H%M%S")
            self.log_names.add(name)

            if name not in self.log_paths or timestamp > self.log_paths[name][0]:
                file_path = Path(log_dir) / file_name
                self.log_paths[name] = (timestamp, file_path)

    def on_tabs_tab_activated(self, event: Tabs.TabActivated) -> None:
        """Handle TabActivated message sent by Tabs."""
        rich_log: RichLog = self.query_one("#logs_log")
        tab_name: str = event.tab.label_text
        rich_log.clear()
        with open(self.log_paths[tab_name][1], "rb") as f:
            try:
                content = f.read().decode("utf-8")
            except UnicodeDecodeError:
                content = f.read().decode("ascii", errors="replace")
        rich_log.write(content)

    @on(Button.Pressed, "#logs_refresh_button")
    def logs_refresh_button_clicked(self) -> None:
        """Handle Button.Pressed message sent by Refresh button."""
        original_log_names = self.log_names.copy()
        self.update_files()
        logs_tabs: Tabs = self.query_one("#logs_tabs")
        for name in self.log_names:
            if name not in original_log_names:
                logs_tabs.add_tab(Tab(label=name, id=f"logs_tab_{name}", disabled=False))

# ============================================= #
#                 Help Screen                   #
# ============================================= #
class HelpScreen(Screen):
    BINDINGS = [("escape", "app.pop_screen", "back")]
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        pass

    def compose(self) -> ComposeResult:
        with open(Path(__file__).parent / "HELP.md", "r") as f:
            help_text = f.read()
        yield Header()
        yield Markdown(help_text, open_links=True, id="help_markdown")
        yield Footer()

class HelpScreenZH(Screen):
    BINDINGS = [("escape", "app.pop_screen", "back")]
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        pass

    def compose(self) -> ComposeResult:
        with open(Path(__file__).parent / "HELP_ZH.md", "r", encoding="utf-8") as f:
            help_text = f.read()
        yield Header()
        yield Markdown(help_text, open_links=True, id="help_markdown_zh")
        yield Footer()

# ============================================= #
#                    Akagi                      #
# ============================================= #
class TableRecord(Vertical):
    """
    Table record widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def compose(self) -> ComposeResult:
        # TODO: Implement table record
        for i in range(4):
            yield Label(f"Player {i}", id=f"table_record_player_{i}")

class Tehai(Horizontal):
    """
    Tehai widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def compose(self) -> ComposeResult:
        for i in range(13):
            yield Label(TILE_2_UNICODE_ART_RICH["?"], id=f"tehai_{i}")
        yield Label(VERTICAL_RULE, id="tehai_rule")
        yield Label(TILE_2_UNICODE_ART_RICH["?"], id="tehai_13") # Tsumo
        self.border_title = "Tehai"

    def update_tehai(self) -> None:
        global mjai_bot

        tehai: list[str] = mjai_bot.tehai_mjai
        tsumo: str = mjai_bot.last_self_tsumo
        if len(tehai) in [14, 11, 8, 5, 2]:
            if tsumo in tehai:
                tehai.remove(tsumo)
            else:
                tsumo = "?"
        else:
            tsumo = "?"

        for i in range(13):
            tehai_label: Label = self.query_one(f"#tehai_{i}")
            if i < len(tehai):
                tehai_label.update(TILE_2_UNICODE_ART_RICH[tehai[i]])
            else:
                tehai_label.update(TILE_2_UNICODE_ART_RICH["?"])
        tehai_label: Label = self.query_one("#tehai_13")
        tehai_label.update(TILE_2_UNICODE_ART_RICH[tsumo])

    def clear_tehai(self) -> None:
        for i in range(13):
            tehai_label: Label = self.query_one(f"#tehai_{i}")
            tehai_label.update(TILE_2_UNICODE_ART_RICH["?"])

class Consume(Horizontal):
    """
    Consume widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def compose(self) -> ComposeResult:
        for i in range(3):
            yield Label(TILE_2_UNICODE_ART_RICH["?"], id=f"consume_{i}")

    def update_consume(self, consume: list[str]) -> None:
        for i in range(3):
            consume_label: Label = self.query_one(f"#consume_{i}")
            if i < len(consume):
                consume_label.update(TILE_2_UNICODE_ART_RICH[consume[i]])
            else:
                consume_label.update(TILE_2_UNICODE_ART_RICH["?"])

    def clear_consume(self) -> None:
        for i in range(3):
            consume_label: Label = self.query_one(f"#consume_{i}")
            consume_label.update(TILE_2_UNICODE_ART_RICH["?"])

class Recommendation(Horizontal):
    """
    Recommendation widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def compose(self) -> ComposeResult:
        yield Button("Akagi", variant="default", id="recommendation_button")
        yield Label(TILE_2_UNICODE_ART_RICH["?"], id="recommendation_tile")
        yield Label(EMPTY_VERTICAL_RULE, id="recommendation_rule")
        yield Consume(id="recommendation_consume")
        yield Digits(value="00.00", id="recommendation_score")

    def update_recommendation(self, recommend: tuple[str, float]) -> None:
        global mjai_bot
        action_name: dict[str, str] = {
            "reach": "Reach",
            "chi_low": "Chi",
            "chi_mid": "Chi",
            "chi_high": "Chi",
            "pon": "Pon",
            "kan_select": "Kan",
            "hora": "Hora",
            "ryukyoku": "Ryukyoku",
            "none": "None",
            "nukidora": "Nukidora",
        }
        if (recommend[0] in action_name):
            action = action_name[recommend[0]]
        else:
            action = "Dahai"

        recommendation_button: Button = self.query_one("#recommendation_button")
        recommendation_tile: Label = self.query_one("#recommendation_tile")
        recommendation_rule: Label = self.query_one("#recommendation_rule")
        recommendation_consume: Consume = self.query_one("#recommendation_consume")        
        recommendation_score: Digits = self.query_one("#recommendation_score")

        recommendation_button.label = action
        recommendation_button.set_classes([action])
        if recommend[0] in ("reach"):
            # We don't know the tile to reach, so we use "?"
            # This is because MJAI protocol doesn't provide the tile to reach
            assert mjai_bot.can_riichi
            recommendation_tile.update(TILE_2_UNICODE_ART_RICH["?"])
            recommendation_rule.update(EMPTY_VERTICAL_RULE)
            recommendation_consume.clear_consume()
        elif recommend[0] in ("chi_low", "chi_mid", "chi_high"):
            chi_candidates = mjai_bot.find_chi_candidates_simple()
            if recommend[0] == "chi_low":
                assert mjai_bot.can_chi_low
                assert chi_candidates.chi_low_meld is not None
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH[chi_candidates.chi_low_meld[0]])
                recommendation_rule.update(VERTICAL_RULE)
                recommendation_consume.update_consume(chi_candidates.chi_low_meld[1])
            elif recommend[0] == "chi_mid":
                assert mjai_bot.can_chi_mid
                assert chi_candidates.chi_mid_meld is not None
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH[chi_candidates.chi_mid_meld[0]])
                recommendation_rule.update(VERTICAL_RULE)
                recommendation_consume.update_consume(chi_candidates.chi_mid_meld[1])
            elif recommend[0] == "chi_high":
                assert mjai_bot.can_chi_high
                assert chi_candidates.chi_high_meld is not None
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH[chi_candidates.chi_high_meld[0]])
                recommendation_rule.update(VERTICAL_RULE)
                recommendation_consume.update_consume(chi_candidates.chi_high_meld[1])
        elif recommend[0] in ("pon"):
            assert mjai_bot.can_pon
            recommendation_tile.update(TILE_2_UNICODE_ART_RICH[mjai_bot.last_kawa_tile])
            recommendation_rule.update(VERTICAL_RULE)
            recommendation_consume.update_consume([mjai_bot.last_kawa_tile[:2], mjai_bot.last_kawa_tile[:2]])
        elif recommend[0] in ("kan_select"):
            assert mjai_bot.can_kan
            if mjai_bot.can_daiminkan:
                # When we can daiminkan, this is the only way to kan.
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH[mjai_bot.last_kawa_tile])
                recommendation_rule.update(VERTICAL_RULE)
                recommendation_consume.update_consume([mjai_bot.last_kawa_tile[:2]]*3)
            else:
                # We don't know the tile to kan, so we use "?"
                # At some rare cases, we can have multiple kan options
                # but we don't know which one to choose, so we use "?"
                # This is because of Mortal model's limitation.
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH["?"])
                recommendation_rule.update(EMPTY_VERTICAL_RULE)
                recommendation_consume.clear_consume()
        elif recommend[0] in ("hora"):
            assert mjai_bot.can_agari
            if mjai_bot.can_ron_agari:
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH[mjai_bot.last_kawa_tile])
                recommendation_rule.update(EMPTY_VERTICAL_RULE)
                recommendation_consume.clear_consume()
            elif mjai_bot.can_tsumo_agari:
                recommendation_tile.update(TILE_2_UNICODE_ART_RICH[mjai_bot.last_self_tsumo])
                recommendation_rule.update(EMPTY_VERTICAL_RULE)
                recommendation_consume.clear_consume()
        elif recommend[0] in ("ryukyoku"):
            assert mjai_bot.can_ryukyoku
            recommendation_tile.update(TILE_2_UNICODE_ART_RICH["?"])
            recommendation_rule.update(EMPTY_VERTICAL_RULE)
            recommendation_consume.clear_consume()
        elif recommend[0] in ("none"):
            recommendation_tile.update(TILE_2_UNICODE_ART_RICH["?"])
            recommendation_rule.update(EMPTY_VERTICAL_RULE)
            recommendation_consume.clear_consume()
        elif recommend[0] in ("nukidora"):
            recommendation_tile.update(TILE_2_UNICODE_ART_RICH["N"])
            recommendation_rule.update(EMPTY_VERTICAL_RULE)
            recommendation_consume.clear_consume()
        else:
            assert mjai_bot.can_discard
            recommendation_tile.update(TILE_2_UNICODE_ART_RICH[recommend[0]])
            recommendation_rule.update(EMPTY_VERTICAL_RULE)
            recommendation_consume.clear_consume()
        recommendation_score.update(f"{recommend[1]*100:.2f}")

    def clear_recommendation(self) -> None:
        recommendation_button: Button = self.query_one("#recommendation_button")
        recommendation_button.label = "Akagi"
        recommendation_button.set_classes([])
        recommendation_tile: Label = self.query_one("#recommendation_tile")
        recommendation_tile.update(TILE_2_UNICODE_ART_RICH["?"])
        recommendation_rule: Label = self.query_one("#recommendation_rule")
        recommendation_rule.update(EMPTY_VERTICAL_RULE)
        recommendation_consume: Consume = self.query_one("#recommendation_consume")
        recommendation_consume.clear_consume()
        recommendation_score: Digits = self.query_one("#recommendation_score")
        recommendation_score.update("00.00")

    @on(Button.Pressed, "#recommendation_button")
    def recommendation_button_clicked(self) -> None:
        # When clicked, check if the action is reach or kan_select
        # 1. Reach:
        #   - send reach action back to MJAI
        # 2. Kan_select:
        #   - TODO
        global mitm_client
        recommendation_button: Button = self.query_one("#recommendation_button")
        if recommendation_button.label == "Reach":
            # I think Mortal can tolerate getting multiple reach
            # https://github.com/Equim-chan/Mortal/blob/3ec7a80f0f34446e9fd51c5df4a2940706874fe7/libriichi/src/state/update.rs#L665
            mitm_client.messages.put({
                "type": "reach",
                "actor": mjai_controller.bot.player_id,
            })
        elif recommendation_button.label == "Kan":
            # TODO
            pass

class Recommendations(Vertical):
    """
    Recommendations widget.
    """
    RECOMMENDATION_COUNT = 3

    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def compose(self) -> ComposeResult:
        for i in range(self.RECOMMENDATION_COUNT):
            yield Recommendation(id=f"recommendation_{i}")
        self.border_title = "Top Recommendations"

    def update_recommendation(self, mjai_msg: dict) -> None:
        global mjai_bot
        if "meta" not in mjai_msg:
            return
        if "q_values" not in mjai_msg["meta"]:
            return
        meta = mjai_msg["meta"]
        recommends: list[tuple[str, float]] = meta_to_recommend(meta, mjai_bot.is_3p)
        for i in range(self.RECOMMENDATION_COUNT):
            recommend: Recommendation = self.query_one(f"#recommendation_{i}")
            if i < len(recommends):
                recommend.update_recommendation(recommends[i])
            else:
                recommend.clear_recommendation()

class BestAction(Horizontal):
    # TODO: When action is None, fails to update
    """
    Best action widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def compose(self) -> ComposeResult:
        yield Button("Action", id="best_action_button_action", variant="default")
        yield Label(TILE_2_UNICODE_ART_RICH["?"], id="best_action_tile")
        yield Label(EMPTY_VERTICAL_RULE, id="best_action_rule")
        yield Consume(id="best_action_consume")
        self.border_title = "Suggestion"

    def update_best_action(self, mjai_msg: dict) -> None:
        global mjai_bot
        action_name: dict[str, str] = {
            "dahai": "Dahai",
            "chi": "Chi",
            "pon": "Pon",
            "ankan": "Kan",
            "daiminkan": "Kan",
            "kakan": "Kan",
            "reach": "Reach",
            "hora": "Hora",
            "ryukyoku": "Ryukyoku",
            "none": "None",
            "nukidora": "Nukidora",
        }
        if (mjai_msg["type"] in action_name):
            action = action_name[mjai_msg["type"]]
        else:
            action = "Unknown"
            logger.error(f"Unknown action: {mjai_msg['type']}")

        best_action_button_action: Button = self.query_one("#best_action_button_action")
        best_action_tile: Label = self.query_one("#best_action_tile")
        best_action_rule: Label = self.query_one("#best_action_rule")
        best_action_consume: Consume = self.query_one("#best_action_consume")

        best_action_button_action.set_classes([action])
        best_action_button_action.label = action
        match mjai_msg["type"]:
            case "none":
                best_action_tile.update(TILE_2_UNICODE_ART_RICH["?"])
                best_action_rule.update(EMPTY_VERTICAL_RULE)
                best_action_consume.clear_consume()
            case "dahai" | "nukidora":
                best_action_tile.update(TILE_2_UNICODE_ART_RICH[mjai_msg["pai"]])
                best_action_rule.update(EMPTY_VERTICAL_RULE)
                best_action_consume.clear_consume()
            case "chi" | "pon" | "daiminkan" | "kakan":
                best_action_tile.update(TILE_2_UNICODE_ART_RICH[mjai_msg["pai"]])
                best_action_rule.update(VERTICAL_RULE)
                best_action_consume.update_consume(mjai_msg["consumed"])
            case "ankan":
                best_action_tile.update(TILE_2_UNICODE_ART_RICH[mjai_msg["consumed"][0]])
                best_action_rule.update(VERTICAL_RULE)
                best_action_consume.update_consume(mjai_msg["consumed"][1:])
            case "reach" | "hora" | "ryukyoku":
                best_action_tile.update(TILE_2_UNICODE_ART_RICH["?"])
                best_action_rule.update(EMPTY_VERTICAL_RULE)
                best_action_consume.clear_consume()
            case _:
                logger.error(f"Unknown action: {mjai_msg['type']}")
                pass

    @on(Button.Pressed, "#best_action_button_action")
    def best_action_button_clicked(self) -> None:
        # When clicked, check if the action is reach or kan_select
        # 1. Reach:
        #   - send reach action back to MJAI
        # 2. Kan_select:
        #   - TODO
        global mitm_client
        best_action_button_action: Button = self.query_one("#best_action_button_action")
        if best_action_button_action.label == "Reach":
            # I think Mortal can tolerate getting multiple reach
            # https://github.com/Equim-chan/Mortal/blob/3ec7a80f0f34446e9fd51c5df4a2940706874fe7/libriichi/src/state/update.rs#L665
            mitm_client.messages.put({
                "type": "reach",
                "actor": mjai_controller.bot.player_id,
            })
        elif best_action_button_action.label == "Kan":
            # TODO
            pass

class BotStatus(Vertical):
    """
    Bot status widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(**kwargs)

    def on_mount(self) -> None:
        """
        Called when the widget is mounted.
        """
        mjai_bot_status: DataTable = self.query_one("#mjai_bot_status")
        COLUMN_NAMES = [
            "type",
            "value",
            "note",
        ]
        ROW_TYPE = [
            ("player_id",       None, ""),
            ("target_actor",    None, ""),
            ("target_actor_rel",None, ""),
            ("can_act",         None, ""),
            ("can_discard",     None, ""),
            ("can_riichi",      None, ""),
            ("can_chi",         None, ""),
            ("can_chi_low",     None, ""),
            ("can_chi_mid",     None, ""),
            ("can_chi_high",    None, ""),
            ("can_pon",         None, ""),
            ("can_kan",         None, ""),
            ("can_daiminkan",   None, ""),
            ("can_ankan",       None, ""),
            ("can_kakan",       None, ""),
            ("can_agari",       None, ""),
            ("can_ron_agari",   None, ""),
            ("can_tsumo_agari", None, ""),
            ("can_ryukyoku",    None, ""),
            ("can_pass",        None, ""),
        ]
        mjai_bot_status.add_columns(*COLUMN_NAMES)
        mjai_bot_status.add_rows(ROW_TYPE)

    def compose(self) -> ComposeResult:
        yield DataTable(
            show_cursor=False,
            zebra_stripes=True,
            id="mjai_bot_status",
        )
        self.border_title = "Bot Status"

    def update_bot_status(self) -> None:
        """
        Update the bot status table with the current bot status.
        """
        global mjai_bot

        mjai_bot_status: DataTable = self.query_one("#mjai_bot_status")
        attributes = [
            mjai_bot.player_id, mjai_bot.target_actor, mjai_bot.target_actor_rel, mjai_bot.can_act,
            mjai_bot.can_discard, mjai_bot.can_riichi, mjai_bot.can_chi, mjai_bot.can_chi_low,
            mjai_bot.can_chi_mid, mjai_bot.can_chi_high, mjai_bot.can_pon, mjai_bot.can_kan,
            mjai_bot.can_daiminkan, mjai_bot.can_ankan, mjai_bot.can_kakan, mjai_bot.can_agari,
            mjai_bot.can_ron_agari, mjai_bot.can_tsumo_agari, mjai_bot.can_ryukyoku, mjai_bot.can_pass
        ]
        for row, attribute in enumerate(attributes):
            if isinstance(attribute, bool):
                new_attribute = Text(str(attribute))
                new_attribute.stylize("green" if attribute else "red")
                mjai_bot_status.update_cell_at(Coordinate(row=row, column=1), new_attribute)
            else:
                mjai_bot_status.update_cell_at(Coordinate(row=row, column=1), attribute)

class ContentSwitcherCustom(ContentSwitcher):
    """
    Custom content switcher widget.
    When clicked, it switches to the next child widget.
    """
    def __init__(self, *args, **kwargs):
        self.children_ids = []
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        """
        Called when the widget is mounted.
        """
        self.children_ids = [child.id for child in self.children]

    def on_click(self) -> None:
        """
        Switch to the next child widget when clicked.
        """
        for i, child_id in enumerate(self.children_ids):
            if child_id == self.current:
                self.current = self.children_ids[(i + 1) % len(self.children_ids)]
                break

class MJAIInLog(RichLog):
    """
    MJAI In Log widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        self.border_title = "MJAI In"

class MJAIOutLog(RichLog):
    """
    MJAI Out Log widget.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def on_mount(self) -> None:
        self.border_title = "MJAI Out"

class AkagiApp(App):
    """
    Main application class for Akagi.
    """
    CSS_PATH = "client.tcss"

    BINDINGS = [
        ("t", "random_theme", "Random Theme"),
        ("h", "help_screen", "Help"),
        ("z", "help_screen_zh", "Help (中文)"),
    ]

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.autoplay_mjai_msg: dict | None = None

    def on_mount(self) -> None:
        global settings, mjai_controller
        # ============================================= #
        #                   Main Loop                   #
        # ============================================= #
        self.main_loop_timer = self.set_interval(1 / 20, self.main_loop)

        # ============================================= #
        #                  Screens                      #
        # ============================================= #

        # ============================================= #
        #                  Widgets                      #
        # ============================================= #

        # ============================================= #
        #                   Themes                      #
        # ============================================= #
        for theme in ADDITIONAL_THEMES.values():
            self.register_theme(theme)
        if settings.theme in list(self.available_themes.keys()):
            self.theme = settings.theme
        else:
            logger.warning(f"Theme {settings.theme} not found, using default theme")
            self.theme = "textual-dark"
            self.notify(
                f"Theme {settings.theme} not found, using default theme",
                title="Theme Error",
                severity="warning",
            )
        # ============================================= #
        #                   Models                      #
        # ============================================= #
        if mjai_controller.bot is None:
            self.notify(
                "No bot selected, please make sure you have bots installed in ./mjai_bot directory",
                title="Bot Error",
                severity="error",
            )
            logger.error("No bot selected, please make sure you have bots installed in ./mjai_bot directory")

        # ============================================= #
        #                    MITM                       #
        # ============================================= #
        if not mitm_client.running:
            self.mitm_start_button_clicked()

    def compose(self) -> ComposeResult:
        """
        Create child widgets for the app.
        """
        yield Header()
        yield Horizontal(
            Recommendations(id="recommendation"),
            ContentSwitcherCustom(
                BotStatus(id="bot_status"),
                MJAIOutLog(
                    max_lines=1000, min_width=80, wrap=False,
                    highlight=True, markup=True, auto_scroll=True,
                    id="mjai_out_log"
                ),
                MJAIInLog(
                    max_lines=1000, min_width=80, wrap=False,
                    highlight=True, markup=True, auto_scroll=True,
                    id="mjai_in_log"
                ),
                id="content_switcher",
                initial="mjai_out_log",
            ),
            id="top_container",
        )
        yield Tehai(id="tehai")
        # yield BestAction(id="best_action")
        yield Horizontal(
            Horizontal(
                Button("MITM\n\nStopped", id="option1_button", variant="default"),
                Button("Settings", id="option2_button"),
                Button("Model\n&\nAutoplay", id="option3_button"),
                Button("Logs", id="option4_button"),
                id="option_container",
            ),
            BestAction(id="best_action"),
            id="bottom_container",
        )
        yield Footer()

    def main_loop(self) -> None:
        """
        Main loop for the application.
        """
        try:
            global mitm_client, mjai_controller, mjai_bot, settings
            if not mitm_client.running:
                return
            mjai_msgs = mitm_client.dump_messages()
            if mjai_msgs:
                # ============================================= #
                #                React to MJAI                  #
                # ============================================= #
                mjai_in_log: RichLog = self.query_one("#mjai_in_log")
                for mjai_msg in mjai_msgs:
                    logger.debug(f"-> {mjai_msg}")
                    mjai_in_log.write(mjai_msg)
                mjai_response = mjai_controller.react(mjai_msgs)
                logger.debug(f"<- {mjai_response}")
                mjai_bot.react(input_list=mjai_msgs)
                mjai_out_log: RichLog = self.query_one("#mjai_out_log")
                if (
                    ((mjai_response["type"] != "none" or mjai_bot.can_act   ) and (not mjai_bot.is_3p)) or
                    ((mjai_response["type"] != "none" or mjai_bot.can_act_3p) and (    mjai_bot.is_3p))
                ):
                    mjai_out_log.write(mjai_response)
                # ============================================= #
                #             Update Widgets and UI             #
                # ============================================= #
                bot_status: BotStatus = self.query_one("#bot_status")
                bot_status.update_bot_status()
                tehai: Tehai = self.query_one("#tehai")
                tehai.update_tehai()
                best_action: BestAction = self.query_one("#best_action")
                best_action.update_best_action(mjai_response)
                recommendation: Recommendations = self.query_one("#recommendation")
                recommendation.update_recommendation(mjai_response)
                # ============================================= #
                #             Autoplay and Actions              #
                # ============================================= #
                if settings.autoplay:
                    for mjai_msg in mjai_msgs:
                        if mjai_msg.get("type") == "end_game":
                            logger.info("AutoPlay: end_game detected, scheduling lobby navigation")
                            self.set_timer(0.5, autoplay.join_next_game)
                            break
                if (
                    ((mjai_response["type"] != "none" or mjai_bot.can_act   ) and (not mjai_bot.is_3p)) or
                    ((mjai_response["type"] != "none" or mjai_bot.can_act_3p) and (    mjai_bot.is_3p))
                ):
                    if settings.autoplay:
                        self.set_timer(0.1, partial(self.autoplay, mjai_response))
        except Exception as e:
            logger.error(f"Error in main loop: {traceback.format_exc()}")

    def find_autoplay_window(self) -> None:
        global settings, autoplay, mitm_client
        if settings.autoplay:
            window = autoplay.auto_select_window()
            if window is not None:
                logger.info(f"Autoplay window found: {window.name}")
                self.app.notify(
                    f"Autoplay window found: {window.name}",
                    title="Autoplay",
                    severity="information",
                )
            else:
                logger.warning("No autoplay window found")
                self.app.notify(
                    "No autoplay window found, select a target window manually",
                    title="Autoplay",
                    severity="warning",
                )

    def autoplay(self, mjai_response: dict) -> None:
        """
        Autoplay function to handle MJAI messages.
        """
        global autoplay, mitm_client, mjai_controller
        if settings.mitm.type.value in ["tenhou", "unified"]:
            # Riichi City, Tenhou do not support autoplay
            logger.warning("Autoplay is not supported for this MJAI type")
            return

        if (not autoplay.check_window()):
            self.find_autoplay_window()
        try:
            act_result = autoplay.act(mjai_response)
            if not act_result:
                logger.warning("Action not preformed.")
                self.app.notify(
                    "Action not preformed, please check the logs.",
                    title="Autoplay Warning",
                    severity="warning",
                )
        except Exception as e:
            logger.error(f"Error in autoplay: {traceback.format_exc()}")
            self.app.notify(
                f"Error in autoplay: {e}",
                title="Autoplay Error",
                severity="error",
            )
            return
        if mjai_response["type"] == "reach":
            mitm_client.messages.put({
                "type": "reach",
                "actor": mjai_controller.bot.player_id,
            })

    def action_random_theme(self) -> None:
        """
        Randomly select a theme from the available themes.
        """
        theme: str = random.choice(list(self.available_themes.keys()))
        self.theme = theme
        logger.info(f"Theme changed to {theme}")
    
    def action_help_screen(self) -> None:
        """
        Show the help screen.
        """
        self.push_screen(HelpScreen())

    def action_help_screen_zh(self) -> None:
        """
        Show the help screen in Chinese.
        """
        self.push_screen(HelpScreenZH())

    @on(Button.Pressed, "#option1_button")
    def mitm_start_button_clicked(self) -> None:
        global mitm_client, autoplay, settings

        option1_button: Button = self.query_one("#option1_button")
        if mitm_client.running:
            mitm_client.stop()
            option1_button.label = "MITM\n\nStopped"
            option1_button.variant = "default"
        else:
            mitm_client.start()
            option1_button.label = "MITM\n\nRunning"
            option1_button.variant = "success"

    @on(Button.Pressed, "#option2_button")
    def settings_button_clicked(self) -> None:
        """
        Open the settings screen.
        """
        self.push_screen(SettingsScreen())
   
    @on(Button.Pressed, "#option3_button")
    def model_button_clicked(self) -> None:
        """
        Open the model screen.
        """
        self.push_screen(ModelsScreen())

    @on(Button.Pressed, "#option4_button")
    def logs_button_clicked(self) -> None:
        """
        Open the logs screen.
        """
        self.push_screen(LogsScreen())

def main():
    """
    Main entry point for Akagi.
    """
    global mitm_client, mjai_controller, mjai_bot, settings, autoplay

    logger.info("Starting Akagi...")
    logger.info(f"MITM Proxy: {settings.mitm.host}:{settings.mitm.port} ({settings.mitm.type})")
    mitm_client = Client()
    logger.info(f"Starting MJAI controller")
    mjai_controller = Controller()
    mjai_bot = AkagiBot()
    autoplay = AutoPlay()
    autoplay.set_bot(mjai_bot)
    autoplay.set_autoplay()

    logger.info("Starting App...")
    app = AkagiApp()
    try:
        app.run()
    except KeyboardInterrupt:
        logger.info("Stopping Akagi...")
    mitm_client.stop()
    logger.info("Akagi stopped")
    sys.exit(0)
