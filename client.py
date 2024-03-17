import atexit
import json
import os
import pathlib
import subprocess
import sys
import time
import webbrowser
from pathlib import Path
from sys import executable
from threading import Thread
from typing import Any, Coroutine
from xmlrpc.client import ServerProxy

from my_logger import logger, game_result_log
from textual import on
from textual.app import App, ComposeResult
from textual.containers import Horizontal, ScrollableContainer, Vertical
from textual.css.query import NoMatches
from textual.events import Event, ScreenResume
from textual.screen import Screen
from textual.widgets import (Button, Checkbox, Footer, Header, Input, Label,
                             LoadingIndicator, Log, Markdown, Pretty, Rule,
                             Static)

from action import Action
from liqi import LiqiProto, MsgType
from majsoul2mjai import MajsoulBridge
from libriichi_helper import meta_to_recommend, state_to_tehai
from tileUnicode import TILE_2_UNICODE_ART_RICH, TILE_2_UNICODE, VERTICLE_RULE, HAI_VALUE


submission = 'players/bot.zip'
PORT_NUM = 28680
AUTOPLAY = False
ENABLE_PLAYWRIGHT = False
with open("settings.json", "r") as f:
    settings = json.load(f)
    PORT_NUM = settings["Port"]["MJAI"]
    AUTOPLAY = settings["Autoplay"]
    ENABLE_PLAYWRIGHT = settings["Playwright"]["enable"]


class FlowScreen(Screen):

    BINDINGS = [
        ("ctrl+q", "quit", "Quit"),
    ]

    def __init__(self, flow_id, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.flow_id = flow_id
        self.liqi_msg_idx = 0
        self.mjai_msg_idx = 0
        self.consume_ids = []
        self.latest_operation_list = None
        self.syncing = True
        self.action = Action(self.app.rpc_server)
        self.isLiqi = False

    def compose(self) -> ComposeResult:
        """Called to add widgets to the app."""
        liqi_log_container = ScrollableContainer(Pretty(self.app.liqi_msg_dict[self.flow_id], id="liqi_log"), id="liqi_log_container")
        mjai_log_container = ScrollableContainer(Pretty(self.app.mjai_msg_dict[self.flow_id], id="mjai_log"), id="mjai_log_container")
        log_container = Horizontal(liqi_log_container, mjai_log_container, id="log_container")
        liqi_log_container.border_title = "LiqiProto"
        mjai_log_container.border_title = "Mjai"
        tehai_labels = [Label(TILE_2_UNICODE_ART_RICH["?"], id="tehai_"+str(i)) for i in range(13)]
        tehai_value_labels = [Label(HAI_VALUE[40], id="tehai_value_"+str(i)) for i in range(13)]
        tehai_rule = Label(VERTICLE_RULE, id="tehai_rule")
        tsumohai_label = Label(TILE_2_UNICODE_ART_RICH["?"], id="tsumohai")
        tsumohai_value_label = Label(HAI_VALUE[40], id="tsumohai_value")
        tehai_container = Horizontal(id="tehai_container")
        for i in range(13):
            tehai_container.mount(tehai_labels[i])
            tehai_container.mount(tehai_value_labels[i])
        tehai_container.mount(tehai_rule)
        tehai_container.mount(tsumohai_label)
        tehai_container.mount(tsumohai_value_label)
        tehai_container.border_title = "Tehai"
        akagi_action = Button("Akagi", id="akagi_action", variant="default")
        akagi_pai    = Button("Pai", id="akagi_pai", variant="default")
        pai_unicode_art = Label(TILE_2_UNICODE_ART_RICH["?"], id="pai_unicode_art")
        akagi_container = Horizontal(akagi_action, akagi_pai, pai_unicode_art, id="akagi_container")
        akagi_container.border_title = "Akagi"
        loading_indicator = LoadingIndicator(id="loading_indicator")
        loading_indicator.styles.height = "3"
        checkbox_autoplay = Checkbox("Autoplay", id="checkbox_autoplay", classes="short", value=AUTOPLAY)
        checkbox_test_one = Checkbox("test_one", id="checkbox_test_one", classes="short")
        checkbox_test_two = Checkbox("test_two", id="checkbox_test_two", classes="short")
        checkbox_container = Vertical(checkbox_autoplay, checkbox_test_one, id="checkbox_container")
        checkbox_container.border_title = "Options"
        bottom_container = Horizontal(checkbox_container, akagi_container, id="bottom_container")
        yield Header()
        yield Footer()
        yield loading_indicator
        yield log_container
        yield tehai_container
        yield bottom_container

    def on_mount(self) -> None:
        self.liqi_log = self.query_one("#liqi_log")
        self.mjai_log = self.query_one("#mjai_log")
        self.akagi_action = self.query_one("#akagi_action")
        self.akagi_pai = self.query_one("#akagi_pai")
        self.pai_unicode_art = self.query_one("#pai_unicode_art")
        self.akagi_container = self.query_one("#akagi_container")
        self.liqi_log.update(self.app.liqi_msg_dict[self.flow_id])
        self.mjai_log.update(self.app.mjai_msg_dict[self.flow_id])
        self.liqi_log_container = self.query_one("#liqi_log_container")
        self.mjai_log_container = self.query_one("#mjai_log_container")
        self.tehai_labels = [self.query_one("#tehai_"+str(i)) for i in range(13)]
        self.tehai_value_labels = [self.query_one("#tehai_value_"+str(i)) for i in range(13)]
        self.tehai_rule = self.query_one("#tehai_rule")
        self.tsumohai_label = self.query_one("#tsumohai")
        self.tsumohai_value_label = self.query_one("#tsumohai_value")
        self.tehai_container = self.query_one("#tehai_container")
        self.liqi_log_container.scroll_end(animate=False)
        self.mjai_log_container.scroll_end(animate=False)
        self.liqi_msg_idx = len(self.app.liqi_msg_dict[self.flow_id])
        self.mjai_msg_idx = len(self.app.mjai_msg_dict[self.flow_id])
        self.update_log = self.set_interval(0.10, self.refresh_log)
        try:
            self.akagi_action.label = self.app.mjai_msg_dict[self.flow_id][-1]["type"]
            for akagi_action_class in self.akagi_action.classes:
                self.akagi_action.remove_class(akagi_action_class)
            self.akagi_action.add_class("action_"+self.app.mjai_msg_dict[self.flow_id][-1]["type"])
            for akagi_pai_class in self.akagi_pai.classes:
                self.akagi_pai.remove_class(akagi_pai_class)
            self.akagi_pai.add_class("pai_"+self.app.mjai_msg_dict[self.flow_id][-1]["type"])
        except IndexError:
            self.akagi_action.label = "Akagi"

    def refresh_log(self) -> None:
        # Yes I know this is stupid
        try:
            if self.liqi_msg_idx < len(self.app.liqi_msg_dict[self.flow_id]):
                self.liqi_log.update(self.app.liqi_msg_dict[self.flow_id][-1])
                self.liqi_log_container.scroll_end(animate=False)
                self.liqi_msg_idx += 1
                liqi_msg = self.app.liqi_msg_dict[self.flow_id][-1]
                if liqi_msg['type'] == MsgType.Notify:
                    if liqi_msg['method'] == '.lq.ActionPrototype':
                        if 'operation' in liqi_msg['data']['data']:
                            if 'operationList' in liqi_msg['data']['data']['operation']:
                                self.action.latest_operation_list = liqi_msg['data']['data']['operation']['operationList']
                        if liqi_msg['data']['name'] == 'ActionDiscardTile':
                            self.action.isNewRound = False
                            if liqi_msg['data']['data']['isLiqi']:
                                self.isLiqi = True
                            pass
                        if liqi_msg['data']['name'] == 'ActionNewRound':
                            self.action.isNewRound = True
                            self.action.reached = False
                    if liqi_msg['method'] == '.lq.NotifyGameEndResult' or liqi_msg['method'] == '.lq.NotifyGameTerminate':
                        self.action_quit()

            elif self.syncing:
                self.query_one("#loading_indicator").remove()
                self.syncing = False
                if AUTOPLAY and len(self.app.mjai_msg_dict[self.flow_id]) > 0:
                    logger.log("CLICK", self.app.mjai_msg_dict[self.flow_id][-1])
                    self.app.set_timer(2, self.autoplay)
            if self.mjai_msg_idx < len(self.app.mjai_msg_dict[self.flow_id]):
                self.app.mjai_msg_dict[self.flow_id][-1]['meta'] = meta_to_recommend(self.app.mjai_msg_dict[self.flow_id][-1]['meta'])
                latest_mjai_msg = self.app.mjai_msg_dict[self.flow_id][-1]
                # Update tehai
                player_state = self.app.bridge[self.flow_id].mjai_client.bot.state()
                tehai, tsumohai = state_to_tehai(player_state)
                for idx, tehai_label in enumerate(self.tehai_labels):
                    tehai_label.update(TILE_2_UNICODE_ART_RICH[tehai[idx]])
                action_list = [x[0] for x in latest_mjai_msg['meta']]
                for idx, tehai_value_label in enumerate(self.tehai_value_labels):
                    # latest_mjai_msg['meta'] is list of (pai, value)
                    try:
                        pai_value = int(latest_mjai_msg['meta'][action_list.index(tehai[idx])][1] * 40)
                        if pai_value == 40:
                            pai_value = 39
                    except ValueError:
                        pai_value = 40
                    tehai_value_label.update(HAI_VALUE[pai_value])
                self.tsumohai_label.update(TILE_2_UNICODE_ART_RICH[tsumohai])
                if tsumohai in action_list:
                    try:
                        pai_value = int(latest_mjai_msg['meta'][action_list.index(tsumohai)][1] * 40)
                        if pai_value == 40:
                            pai_value = 39
                    except ValueError:
                        pai_value = 40
                    self.tsumohai_value_label.update(HAI_VALUE[pai_value])
                # mjai log
                self.mjai_log.update(self.app.mjai_msg_dict[self.flow_id][-3:])
                self.mjai_log_container.scroll_end(animate=False)
                self.mjai_msg_idx += 1
                self.akagi_action.label = latest_mjai_msg["type"]
                for akagi_action_class in self.akagi_action.classes:
                    self.akagi_action.remove_class(akagi_action_class)
                self.akagi_action.add_class("action_"+latest_mjai_msg["type"])
                for akagi_pai_class in self.akagi_pai.classes:
                    self.akagi_pai.remove_class(akagi_pai_class)
                self.akagi_pai.add_class("pai_"+latest_mjai_msg["type"])
                if "consumed" in latest_mjai_msg:
                    self.akagi_pai.label = str(latest_mjai_msg["consumed"])
                    if "pai" in latest_mjai_msg:
                        self.pai_unicode_art.update(TILE_2_UNICODE_ART_RICH[latest_mjai_msg["pai"]])
                    self.akagi_container.mount(Label(VERTICLE_RULE, id="consumed_rule"))
                    self.consume_ids.append("#"+"consumed_rule")
                    i=0
                    for c in latest_mjai_msg["consumed"]:
                        self.akagi_container.mount(Label(TILE_2_UNICODE_ART_RICH[c], id="consumed_"+c+str(i)))
                        self.consume_ids.append("#"+"consumed_"+c+str(i))
                        i+=1
                elif "pai" in latest_mjai_msg:
                    for consume_id in self.consume_ids:
                        self.query_one(consume_id).remove()
                    self.consume_ids = []
                    self.akagi_pai.label = str(latest_mjai_msg["pai"])
                    self.pai_unicode_art.update(TILE_2_UNICODE_ART_RICH[latest_mjai_msg["pai"]])
                else:
                    self.akagi_pai.label = "None"
                    self.pai_unicode_art.update(TILE_2_UNICODE_ART_RICH["?"])
                # Action
                logger.info(f"Current tehai: {tehai}")
                logger.info(f"Current tsumohai: {tsumohai}")
                self.tehai = tehai
                self.tsumohai = tsumohai
                if not self.syncing and ENABLE_PLAYWRIGHT and AUTOPLAY:
                    logger.log("CLICK", latest_mjai_msg)
                    self.app.set_timer(0.15, self.autoplay)
                    # self.autoplay(tehai, tsumohai)
                    
        except Exception as e:
            logger.error(e)
            pass

    @on(Checkbox.Changed, "#checkbox_autoplay")
    def checkbox_autoplay_changed(self, event: Checkbox.Changed) -> None:
        global AUTOPLAY
        AUTOPLAY = event.value
        pass
        
    def autoplay(self) -> None:
        isliqi = self.isLiqi
        self.action.mjai2action(self.app.mjai_msg_dict[self.flow_id][-1], self.tehai, self.tsumohai, isliqi)
        self.isLiqi = False
        pass

    def action_quit(self) -> None:
        self.app.set_timer(2, self.app.update_flow.resume)
        self.update_log.stop()
        self.app.pop_screen()


class FlowDisplay(Static):

    def __init__(self, flow_id, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.flow_id = flow_id

    def compose(self) -> ComposeResult:
        yield Button(f"Flow {self.flow_id}", id=f"flow_{self.flow_id}_btn", variant="success")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.app.push_screen(FlowScreen(self.flow_id))
        self.app.update_flow.pause()


class HoverLink(Static):
    def __init__(self, text, url, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.renderable = text
        self.url = url
        self.add_class("hover-link")
        self.border_title = self.url
        self.border_subtitle = "Click to open link"

    def on_click(self, event):
        webbrowser.open_new_tab(self.url)
        pass


class SettingsScreen(Static):

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        with open("settings.json", "r") as f:
            settings = json.load(f)
            self.value_port_setting_mitm_input = settings["Port"]["MITM"]
            self.value_port_setting_xmlrpc_input = settings["Port"]["XMLRPC"]
            self.value_unlocker_setting_enable_checkbox = settings["Unlocker"]
            self.value_helper_setting_checkbox = settings["Helper"]
            self.value_autoplay_setting_enable_checkbox = settings["Autoplay"]
            self.value_autoplay_setting_random_time_new_min_input = settings["RandomTime"]["new_min"]
            self.value_autoplay_setting_random_time_new_max_input = settings["RandomTime"]["new_max"]
            self.value_autoplay_setting_random_time_min_input = settings["RandomTime"]["min"]
            self.value_autoplay_setting_random_time_max_input = settings["RandomTime"]["max"]
            self.value_playwright_setting_enable_checkbox = settings["Playwright"]["enable"]
            self.value_playwright_setting_width_input = settings["Playwright"]["width"]
            self.value_playwright_setting_height_input = settings["Playwright"]["height"]

    def compose(self) -> ComposeResult:
        self.port_setting_mitm_label = Label("MITM Port", id="port_setting_mitm_label")
        self.port_setting_mitm_input = Input(placeholder="Port", type="integer", id="port_setting_mitm_input", value=str(self.value_port_setting_mitm_input))
        self.port_setting_mitm_container = Horizontal(self.port_setting_mitm_label, self.port_setting_mitm_input, id="port_setting_mitm_container")
        self.port_setting_xmlrpc_label = Label("XMLRPC Port", id="port_setting_xmlrpc_label")
        self.port_setting_xmlrpc_input = Input(placeholder="Port", type="integer", id="port_setting_xmlrpc_input", value=str(self.value_port_setting_xmlrpc_input))
        self.port_setting_xmlrpc_container = Horizontal(self.port_setting_xmlrpc_label, self.port_setting_xmlrpc_input, id="port_setting_xmlrpc_container")
        self.port_setting_container = Vertical(self.port_setting_mitm_container, self.port_setting_xmlrpc_container, id="port_setting_container")
        self.port_setting_container.border_title = "Port"

        self.unlocker_setting_label = Label("Unlocker", id="unlocker_setting_label")
        self.unlocker_setting_enable_checkbox = Checkbox("Enable", id="unlocker_setting_enable_checkbox", classes="short", value=self.value_unlocker_setting_enable_checkbox)
        self.unlocker_setting_container = Horizontal(self.unlocker_setting_label, self.unlocker_setting_enable_checkbox, id="unlocker_setting_container")
        self.unlocker_setting_container.border_title = "Unlocker"

        self.helper_setting_label = Label("Helper", id="helper_setting_label")
        self.helper_setting_checkbox = Checkbox("Enable", id="helper_setting_checkbox", classes="short", value=self.value_helper_setting_checkbox)
        self.helper_setting_container = Horizontal(self.helper_setting_label, self.helper_setting_checkbox, id="helper_setting_container")
        self.helper_setting_container.border_title = "Helper"

        self.autoplay_setting_enable_label = Label("Enable", id="autoplay_setting_enable_label")
        self.autoplay_setting_enable_checkbox = Checkbox("Enable", id="autoplay_setting_enable_checkbox", classes="short", value=self.value_autoplay_setting_enable_checkbox)
        self.autoplay_setting_enable_container = Horizontal(self.autoplay_setting_enable_label, self.autoplay_setting_enable_checkbox, id="autoplay_setting_enable_container")
        self.autoplay_setting_random_time_new_label = Label("Random New", id="autoplay_setting_random_time_new_label")
        self.autoplay_setting_random_time_new_min_input = Input(placeholder="Min", type="number", id="autoplay_setting_random_time_new_min_input", value=str(self.value_autoplay_setting_random_time_new_min_input))
        self.autoplay_setting_random_time_new_max_input = Input(placeholder="Max", type="number", id="autoplay_setting_random_time_new_max_input", value=str(self.value_autoplay_setting_random_time_new_max_input))
        self.autoplay_setting_random_time_new_container = Horizontal(self.autoplay_setting_random_time_new_label, self.autoplay_setting_random_time_new_min_input, self.autoplay_setting_random_time_new_max_input, id="autoplay_setting_random_time_new_container")
        self.autoplay_setting_random_time_label = Label("Random", id="autoplay_setting_random_time_label")
        self.autoplay_setting_random_time_min_input = Input(placeholder="Min", type="number", id="autoplay_setting_random_time_min_input", value=str(self.value_autoplay_setting_random_time_min_input))
        self.autoplay_setting_random_time_max_input = Input(placeholder="Max", type="number", id="autoplay_setting_random_time_max_input", value=str(self.value_autoplay_setting_random_time_max_input))
        self.autoplay_setting_random_time_container = Horizontal(self.autoplay_setting_random_time_label, self.autoplay_setting_random_time_min_input, self.autoplay_setting_random_time_max_input, id="autoplay_setting_random_time_container")
        self.autoplay_setting_container = Vertical(self.autoplay_setting_enable_container, self.autoplay_setting_random_time_new_container, self.autoplay_setting_random_time_container, id="autoplay_setting_container")
        self.autoplay_setting_container.border_title = "Autoplay"

        self.playwright_setting_enable_label = Label("Enable", id="playwright_setting_enable_label")
        self.playwright_setting_enable_checkbox = Checkbox("Enable", id="playwright_setting_enable_checkbox", classes="short", value=self.value_playwright_setting_enable_checkbox)
        self.playwright_setting_enable_container = Horizontal(self.playwright_setting_enable_label, self.playwright_setting_enable_checkbox, id="playwright_setting_enable_container")
        self.playwright_setting_resolution_label = Label("Resolution", id="playwright_setting_resolution_label")
        self.playwright_setting_width_input = Input(placeholder="Width", type="integer", id="playwright_setting_width_input", value=str(self.value_playwright_setting_width_input))
        self.playwright_setting_height_input = Input(placeholder="Height", type="integer", id="playwright_setting_height_input", value=str(self.value_playwright_setting_height_input))
        self.playwright_setting_resolution_container = Horizontal(self.playwright_setting_resolution_label, self.playwright_setting_width_input, self.playwright_setting_height_input, id="playwright_setting_resolution_container")
        self.playwright_setting_container = Vertical(self.playwright_setting_enable_container, self.playwright_setting_resolution_container, id="playwright_setting_container")
        self.playwright_setting_container.border_title = "Playwright"

        self.setting_save_button = Button("Save", variant="warning", id="setting_save_button")

        self.remove_this_then_you_badluck_for_100years_and_get_hit_by_a_car_then_die = HoverLink("Akagi is Free and Open Sourced on GitHub.\n本程式Akagi在GitHub上完全開源且免費。如果你是付費取得的，你已經被賣家欺騙，請立即舉報、差評、退款。", "https://github.com/shinkuan/Akagi", id="remove_this_you_die")

        self.setting_container = ScrollableContainer(
                                                     self.port_setting_container, 
                                                     self.unlocker_setting_container, 
                                                     self.helper_setting_container,
                                                     self.autoplay_setting_container,
                                                     self.playwright_setting_container,
                                                     self.setting_save_button,
                                                     self.remove_this_then_you_badluck_for_100years_and_get_hit_by_a_car_then_die,
                                                     id="setting_container"
                                                    )

        yield self.setting_container

    @on(Input.Changed, "#port_setting_mitm_input")
    def port_setting_mitm_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_port_setting_mitm_input = int(event.value)
        except:
            pass

    @on(Input.Changed, "#port_setting_xmlrpc_input")
    def port_setting_xmlrpc_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_port_setting_xmlrpc_input = int(event.value)
        except:
            pass

    @on(Checkbox.Changed, "#unlocker_setting_enable_checkbox")
    def unlocker_setting_enable_checkbox_changed(self, event: Checkbox.Changed) -> None:
        self.value_unlocker_setting_enable_checkbox = event.value

    @on(Checkbox.Changed, "#helper_setting_checkbox")
    def helper_setting_checkbox_changed(self, event: Checkbox.Changed) -> None:
        self.value_helper_setting_checkbox = event.value

    @on(Checkbox.Changed, "#autoplay_setting_enable_checkbox")
    def autoplay_setting_enable_checkbox_changed(self, event: Checkbox.Changed) -> None:
        global AUTOPLAY
        AUTOPLAY = event.value
        self.value_autoplay_setting_enable_checkbox = event.value

    @on(Input.Changed, "#autoplay_setting_random_time_new_min_input")
    def autoplay_setting_random_time_new_min_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_autoplay_setting_random_time_new_min_input = float(event.value)
        except:
            pass

    @on(Input.Changed, "#autoplay_setting_random_time_new_max_input")
    def autoplay_setting_random_time_new_max_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_autoplay_setting_random_time_new_max_input = float(event.value)
        except:
            pass

    @on(Input.Changed, "#autoplay_setting_random_time_min_input")
    def autoplay_setting_random_time_min_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_autoplay_setting_random_time_min_input = float(event.value)
        except:
            pass

    @on(Input.Changed, "#autoplay_setting_random_time_max_input")
    def autoplay_setting_random_time_max_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_autoplay_setting_random_time_max_input = float(event.value)
        except:
            pass

    @on(Checkbox.Changed, "#playwright_setting_enable_checkbox")
    def playwright_setting_enable_checkbox_changed(self, event: Checkbox.Changed) -> None:
        self.value_playwright_setting_enable_checkbox = event.value

    @on(Input.Changed, "#playwright_setting_width_input")
    def playwright_setting_width_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_playwright_setting_width_input = int(event.value)
        except:
            pass

    @on(Input.Changed, "#playwright_setting_height_input")
    def playwright_setting_height_input_changed(self, event: Input.Changed) -> None:
        try:
            self.value_playwright_setting_height_input = int(event.value)
        except:
            pass

    @on(Button.Pressed, "#setting_save_button")
    def setting_save_button_pressed(self) -> None:
        with open("settings.json", "r") as f:
            settings = json.load(f)
            settings["Port"]["MITM"] = self.value_port_setting_mitm_input
            settings["Port"]["XMLRPC"] = self.value_port_setting_xmlrpc_input
            settings["Unlocker"] = self.value_unlocker_setting_enable_checkbox
            settings["Helper"] = self.value_helper_setting_checkbox
            settings["Autoplay"] = self.value_autoplay_setting_enable_checkbox
            settings["RandomTime"]["new_min"] = self.value_autoplay_setting_random_time_new_min_input
            settings["RandomTime"]["new_max"] = self.value_autoplay_setting_random_time_new_max_input
            settings["RandomTime"]["min"] = self.value_autoplay_setting_random_time_min_input
            settings["RandomTime"]["max"] = self.value_autoplay_setting_random_time_max_input
            settings["Playwright"]["enable"] = self.value_playwright_setting_enable_checkbox
            settings["Playwright"]["width"] = self.value_playwright_setting_width_input
            settings["Playwright"]["height"] = self.value_playwright_setting_height_input
        with open("settings.json", "w") as f:
            json.dump(settings, f, indent=4)


class Akagi(App):
    CSS_PATH = "client.tcss"

    BINDINGS = [
        ("ctrl+q", "quit", "Quit"),
    ]

    def __init__(self, rpc_server, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.rpc_server = rpc_server
        self.liqi: dict[str, LiqiProto] = {}
        self.bridge: dict[str, MajsoulBridge] = {}
        self.active_flows = []
        self.messages_dict  = dict() # flow.id -> List[flow_msg]
        self.liqi_msg_dict  = dict() # flow.id -> List[liqi_msg]
        self.mjai_msg_dict  = dict() # flow.id -> List[mjai_msg]
        self.akagi_log_dict = dict() # flow.id -> List[akagi_log]
        self.mitm_started = False

    def on_mount(self) -> None:
        self.update_flow = self.set_interval(1, self.refresh_flow)
        self.get_messages_flow = self.set_interval(0.05, self.get_messages)

    def refresh_flow(self) -> None:
        if not self.mitm_started:
            return
        flows = self.rpc_server.get_activated_flows()
        for flow_id in self.active_flows:
            if flow_id not in flows:
                try:
                    self.query_one(f"#flow_{flow_id}").remove()
                except NoMatches:
                    pass
                self.active_flows.remove(flow_id)
                self.messages_dict.pop(flow_id)
                self.liqi_msg_dict.pop(flow_id)
                self.mjai_msg_dict.pop(flow_id)
                self.akagi_log_dict.pop(flow_id)
                self.liqi.pop(flow_id)
                self.bridge.pop(flow_id)
        for flow_id in flows:
            try:
                self.query_one("#FlowContainer")
            except NoMatches:
                continue
            try:
                self.query_one(f"#flow_{flow_id}")
            except NoMatches:
                self.query_one("#FlowContainer").mount(FlowDisplay(flow_id, id=f"flow_{flow_id}"))
                self.active_flows.append(flow_id)
                self.messages_dict[flow_id] = []
                self.liqi_msg_dict[flow_id] = []
                self.mjai_msg_dict[flow_id] = []
                self.akagi_log_dict[flow_id] = []
                self.liqi[flow_id] = LiqiProto()
                self.bridge[flow_id] = MajsoulBridge()

    def get_messages(self):
        if not self.mitm_started:
            return
        for flow_id in self.active_flows:
            messages = self.rpc_server.get_messages(flow_id)
            if messages is not None:
                # Convert xmlrpc.client.Binary to bytes
                messages = messages.data
                assert isinstance(messages, bytes)
                self.messages_dict[flow_id].append(messages)
                liqi_msg = self.liqi[flow_id].parse(messages)
                logger.info(liqi_msg)
                if liqi_msg is not None:
                    self.liqi_msg_dict[flow_id].append(liqi_msg)
                    if liqi_msg['method'] == '.lq.FastTest.authGame' and liqi_msg['type'] == MsgType.Req:
                        self.app.push_screen(FlowScreen(flow_id))
                        pass
                    mjai_msg = self.bridge[flow_id].input(liqi_msg)
                    if mjai_msg is not None:
                        if self.bridge[flow_id].reach and mjai_msg["type"] == "dahai":
                            mjai_msg["type"] = "reach"
                            self.bridge[flow_id].reach = False
                        self.mjai_msg_dict[flow_id].append(mjai_msg)

    def compose(self) -> ComposeResult:
        """Called to add widgets to the app."""
        yield Header()
        yield Button(label="Start MITM", variant="success", id="start_mitm_button")
        yield SettingsScreen(id="settings_screen")
        yield ScrollableContainer(id="FlowContainer")
        yield Footer()

    def on_event(self, event: Event) -> Coroutine[Any, Any, None]:
        return super().on_event(event)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "start_mitm_button":
            self.query_one("#settings_screen").remove()
            start_mitm()
            event.button.variant = "default"
            event.button.disabled = True
            self.set_timer(5, self.mitm_connected)
        pass

    def mitm_connected(self):
        try:
            self.rpc_server.ping()
            self.mitm_started = True
        except:
            self.set_timer(2, self.mitm_connected)

    def action_quit(self) -> None:
        self.update_flow.stop()
        self.get_messages_flow.stop()
        self.exit()


def exit_handler():
    global mitm_exec
    try:
        mitm_exec.kill()
        logger.info("Stop Akagi")
    except:
        pass
    pass


def start_mitm():
    global mitm_exec

    command = [sys.executable, pathlib.Path(__file__).parent / "mitm.py"]

    if sys.platform == "win32":
        # Windows特定代码
        mitm_exec = subprocess.Popen(command, creationflags=subprocess.CREATE_NEW_CONSOLE)
    else:
        # macOS和其他Unix-like系统
        mitm_exec = subprocess.Popen(command, preexec_fn=os.setsid)


if __name__ == '__main__':
    with open("settings.json", "r") as f:
        settings = json.load(f)
        rpc_port = settings["Port"]["XMLRPC"]
    rpc_host = "127.0.0.1"
    s = ServerProxy(f"http://{rpc_host}:{rpc_port}", allow_none=True)
    app = Akagi(rpc_server=s)
    atexit.register(exit_handler)
    try:
        logger.info("Start Akagi")
        app.run()
    except Exception as e:
        exit_handler()
        raise e
