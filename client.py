import os
os.environ["LOGURU_AUTOINIT"] = "False"

from typing import Any, Coroutine
from xmlrpc.client import ServerProxy
import docker
import json
from loguru import logger

from textual import on  
from textual.app import App, ComposeResult
from textual.containers import ScrollableContainer, Horizontal, Vertical
from textual.events import Event, ScreenResume
from textual.widgets import Button, Footer, Header, Static, Log, Pretty, Label, Rule, LoadingIndicator, Checkbox, Input, Markdown
from textual.css.query import NoMatches
from textual.screen import Screen

from liqi import LiqiProto, MsgType
from mjai.player import MjaiPlayerClient
from majsoul2mjai import MajsoulBridge
from tileUnicode import TILE_2_UNICODE_ART_RICH, TILE_2_UNICODE, VERTICLE_RULE
from action import Action
from concurrent.futures import ThreadPoolExecutor
from threading import Thread

submission = 'players/bot.zip'
PORT_NUM = 28680
AUTOPLAY = False
with open("settings.json", "r") as f:
    settings = json.load(f)
    PORT_NUM = settings["Port"]["MJAI"]
    AUTOPLAY = settings["Autoplay"]

def get_container_ports():
    client = docker.from_env()
    containers = client.containers.list()
    used_port_list = []
    for container in containers:
        ports = container.ports
        for _, bindings in ports.items():
            if bindings is not None:
                used_port_list.append(bindings[0]['HostPort'])
    used_port_list = [int(p) for p in used_port_list]
    return used_port_list


class FlowScreen(Screen):

    BINDINGS = [
        # ("d", "toggle_dark", "Toggle dark mode"),
        # ("a", "add_stopwatch", "Add"),
        # ("r", "remove_stopwatch", "Remove"),
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
        self.action = Action()

    def compose(self) -> ComposeResult:
        """Called to add widgets to the app."""
        liqi_log_container = ScrollableContainer(Pretty(self.app.liqi_msg_dict[self.flow_id], id="liqi_log"), id="liqi_log_container")
        mjai_log_container = ScrollableContainer(Pretty(self.app.mjai_msg_dict[self.flow_id], id="mjai_log"), id="mjai_log_container")
        log_container = Horizontal(liqi_log_container, mjai_log_container, id="log_container")
        liqi_log_container.border_title = "LiqiProto"
        mjai_log_container.border_title = "Mjai"
        tehai_labels = [Label(TILE_2_UNICODE_ART_RICH["?"], id="tehai_"+str(i)) for i in range(13)]
        tehai_rule = Label(VERTICLE_RULE, id="tehai_rule")
        tsumohai_label = Label(TILE_2_UNICODE_ART_RICH["?"], id="tsumohai")
        tehai_container = Horizontal(id="tehai_container")
        for tehai_label in tehai_labels:
            tehai_container.mount(tehai_label)
        tehai_container.mount(tehai_rule)
        tehai_container.mount(tsumohai_label)
        tehai_container.border_title = "Tehai"
        akagi_action = Button("Akagi", id="akagi_action", variant="default")
        akagi_pai    = Button("Pai", id="akagi_pai", variant="default")
        pai_unicode_art = Label(TILE_2_UNICODE_ART_RICH["?"], id="pai_unicode_art")
        akagi_container = Horizontal(akagi_action, akagi_pai, pai_unicode_art, id="akagi_container")
        akagi_container.border_title = "Akagi"
        loading_indicator = LoadingIndicator(id="loading_indicator")
        loading_indicator.styles.height = "3"
        checkbox_autoplay = Checkbox("Autoplay", id="checkbox_autoplay", classes="short")
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
        self.tehai_rule = self.query_one("#tehai_rule")
        self.tsumohai_label = self.query_one("#tsumohai")
        self.tehai_container = self.query_one("#tehai_container")
        self.liqi_log_container.scroll_end()
        self.mjai_log_container.scroll_end()
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
                self.liqi_log_container.scroll_end()
                self.liqi_msg_idx += 1
                for idx, tehai_label in enumerate(self.tehai_labels):
                    tehai_label.update(TILE_2_UNICODE_ART_RICH[self.app.bridge[self.flow_id].my_tehais[idx]])
                self.tsumohai_label.update(TILE_2_UNICODE_ART_RICH[self.app.bridge[self.flow_id].my_tsumohai])
                liqi_msg = self.app.liqi_msg_dict[self.flow_id][-1]
                if liqi_msg['type'] == MsgType.Notify:
                    if liqi_msg['method'] == '.lq.ActionPrototype':
                        if liqi_msg['data']['name'] == 'ActionDiscardTile':
                            self.action.isNewRound = False
                            if 'operation' in liqi_msg['data']['data']:
                                if 'operationList' in liqi_msg['data']['data']['operation']:
                                    self.action.latest_operation_list = liqi_msg['data']['data']['operation']['operationList']
                        if liqi_msg['data']['name'] == 'ActionNewRound':
                            self.action.isNewRound = True
                            self.action.reached = False
                    if liqi_msg['method'] == '.lq.NotifyGameEndResult' or liqi_msg['method'] == '.lq.NotifyGameTerminate':
                        self.action_quit()
                    
            elif self.syncing:
                self.query_one("#loading_indicator").remove()
                self.syncing = False
                if AUTOPLAY:
                    logger.log("CLICK", self.app.mjai_msg_dict[self.flow_id][-1])
                    self.app.set_timer(2, self.autoplay)
            if self.mjai_msg_idx < len(self.app.mjai_msg_dict[self.flow_id]):
                self.mjai_log.update(self.app.mjai_msg_dict[self.flow_id])
                self.mjai_log_container.scroll_end()
                self.mjai_msg_idx += 1
                self.akagi_action.label = self.app.mjai_msg_dict[self.flow_id][-1]["type"]
                for akagi_action_class in self.akagi_action.classes:
                    self.akagi_action.remove_class(akagi_action_class)
                self.akagi_action.add_class("action_"+self.app.mjai_msg_dict[self.flow_id][-1]["type"])
                for akagi_pai_class in self.akagi_pai.classes:
                    self.akagi_pai.remove_class(akagi_pai_class)
                self.akagi_pai.add_class("pai_"+self.app.mjai_msg_dict[self.flow_id][-1]["type"])
                if "consumed" in self.app.mjai_msg_dict[self.flow_id][-1]:
                    self.akagi_pai.label = str(self.app.mjai_msg_dict[self.flow_id][-1]["consumed"])
                    if "pai" in self.app.mjai_msg_dict[self.flow_id][-1]:
                        self.pai_unicode_art.update(TILE_2_UNICODE_ART_RICH[self.app.mjai_msg_dict[self.flow_id][-1]["pai"]])
                    self.akagi_container.mount(Label(VERTICLE_RULE, id="consumed_rule"))
                    self.consume_ids.append("#"+"consumed_rule")
                    i=0
                    for c in self.app.mjai_msg_dict[self.flow_id][-1]["consumed"]:
                        self.akagi_container.mount(Label(TILE_2_UNICODE_ART_RICH[c], id="consumed_"+c+str(i)))
                        self.consume_ids.append("#"+"consumed_"+c+str(i))
                        i+=1
                elif "pai" in self.app.mjai_msg_dict[self.flow_id][-1]:
                    for consume_id in self.consume_ids:
                        self.query_one(consume_id).remove()
                    self.consume_ids = []
                    self.akagi_pai.label = str(self.app.mjai_msg_dict[self.flow_id][-1]["pai"])
                    self.pai_unicode_art.update(TILE_2_UNICODE_ART_RICH[self.app.mjai_msg_dict[self.flow_id][-1]["pai"]])
                else:
                    self.akagi_pai.label = "None"
                    self.pai_unicode_art.update(TILE_2_UNICODE_ART_RICH["?"])
                # Action
                if not self.syncing and AUTOPLAY:
                    logger.log("CLICK", self.app.mjai_msg_dict[self.flow_id][-1])
                    self.app.set_timer(0.1, self.autoplay)
                    
        except Exception as e:
            logger.error(e)
            pass
        
    def autoplay(self) -> None:
        self.action.mjai2action(self.app.mjai_msg_dict[self.flow_id][-1], self.app.bridge[self.flow_id].my_tehais, self.app.bridge[self.flow_id].my_tsumohai)
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


class SettingsScreen(Screen):
    
    BINDINGS = [
        ("ctrl+q", "quit_setting", "Quit Settings"),
        ("ctrl+s", "quit_setting", "Quit Settings"),
    ]

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        with open("settings.json", "r") as f:
            settings = json.load(f)
            self.value_port_setting_mitm_input = settings["Port"]["MITM"]
            self.value_port_setting_xmlrpc_input = settings["Port"]["XMLRPC"]
            self.value_unlocker_setting_enable_checkbox = settings["Unlocker"]
            self.value_unlocker_setting_v10_checkbox = settings["v10"]
            self.value_autoplay_setting_enable_checkbox = settings["Autoplay"]
            # self.value_autoplay_setting_random_time_min_input = settings["Autoplay"]["Random Time"]["Min"]
            # self.value_autoplay_setting_random_time_max_input = settings["Autoplay"]["Random Time"]["Max"]

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
        self.unlocker_setting_v10_checkbox = Checkbox("v10", id="unlocker_setting_v10_checkbox", classes="short", value=self.value_unlocker_setting_v10_checkbox)
        self.unlocker_setting_container = Horizontal(self.unlocker_setting_label, self.unlocker_setting_enable_checkbox, self.unlocker_setting_v10_checkbox, id="unlocker_setting_container")
        self.unlocker_setting_container.border_title = "Unlocker"

        self.autoplay_setting_enable_label = Label("Enable", id="autoplay_setting_enable_label")
        self.autoplay_setting_enable_checkbox = Checkbox("Enable", id="autoplay_setting_enable_checkbox", classes="short", value=self.value_autoplay_setting_enable_checkbox)
        self.autoplay_setting_enable_container = Horizontal(self.autoplay_setting_enable_label, self.autoplay_setting_enable_checkbox, id="autoplay_setting_enable_container")
        self.autoplay_setting_random_time_label = Label("Random Time", id="autoplay_setting_random_time_label")
        self.autoplay_setting_random_time_min_input = Input(placeholder="Min", type="number", id="autoplay_setting_random_time_min_input")
        self.autoplay_setting_random_time_max_input = Input(placeholder="Max", type="number", id="autoplay_setting_random_time_max_input")
        self.autoplay_setting_random_time_container = Horizontal(self.autoplay_setting_random_time_label, self.autoplay_setting_random_time_min_input, self.autoplay_setting_random_time_max_input, id="autoplay_setting_random_time_container")
        self.autoplay_setting_container = Vertical(self.autoplay_setting_enable_container, self.autoplay_setting_random_time_container, id="autoplay_setting_container")
        self.autoplay_setting_container.border_title = "Autoplay"

        yield Header()
        yield Markdown("# Settings")
        yield self.port_setting_container
        yield self.unlocker_setting_container
        yield self.autoplay_setting_container
        yield Footer()

    @on(Input.Changed, "#port_setting_mitm_input")
    def port_setting_mitm_input_changed(self, event: Input.Changed) -> None:
        self.value_port_setting_mitm_input = event.value

    @on(Input.Changed, "#port_setting_xmlrpc_input")
    def port_setting_xmlrpc_input_changed(self, event: Input.Changed) -> None:
        self.value_port_setting_xmlrpc_input = event.value

    @on(Checkbox.Changed, "#unlocker_setting_enable_checkbox")
    def unlocker_setting_enable_checkbox_changed(self, event: Checkbox.Changed) -> None:
        self.value_unlocker_setting_enable_checkbox = event.value

    @on(Checkbox.Changed, "#unlocker_setting_v10_checkbox")
    def unlocker_setting_v10_checkbox_changed(self, event: Checkbox.Changed) -> None:
        self.value_unlocker_setting_v10_checkbox = event.value

    @on(Checkbox.Changed, "#autoplay_setting_enable_checkbox")
    def autoplay_setting_enable_checkbox_changed(self, event: Checkbox.Changed) -> None:
        self.value_autoplay_setting_enable_checkbox = event.value

    @on(Input.Changed, "#autoplay_setting_random_time_min_input")
    def autoplay_setting_random_time_min_input_changed(self, event: Input.Changed) -> None:
        # self.value_autoplay_setting_random_time_min_input = event.value
        pass

    @on(Input.Changed, "#autoplay_setting_random_time_max_input")
    def autoplay_setting_random_time_max_input_changed(self, event: Input.Changed) -> None:
        # self.value_autoplay_setting_random_time_max_input = event.value
        pass

    def action_quit_setting(self) -> None:
        with open("settings.json", "r") as f:
            settings = json.load(f)
            settings["Port"]["MITM"] = int(self.value_port_setting_mitm_input)
            settings["Port"]["XMLRPC"] = int(self.value_port_setting_xmlrpc_input)
            settings["Unlocker"] = self.value_unlocker_setting_enable_checkbox
            settings["v10"] = self.value_unlocker_setting_v10_checkbox
            settings["Autoplay"] = self.value_autoplay_setting_enable_checkbox
            # settings["Autoplay"]["Random Time"]["Min"] = self.value_autoplay_setting_random_time_min_input
            # settings["Autoplay"]["Random Time"]["Max"] = self.value_autoplay_setting_random_time_max_input
        with open("settings.json", "w") as f:
            json.dump(settings, f, indent=4)
        self.app.pop_screen()


class Akagi(App):
    CSS_PATH = "client.tcss"

    BINDINGS = [
        # ("d", "toggle_dark", "Toggle dark mode"),
        # ("a", "add_stopwatch", "Add"),
        # ("r", "remove_stopwatch", "Remove"),
        ("ctrl+q", "quit", "Quit"),
        ("ctrl+s", "settings", "Settings")
    ]

    def __init__(self, rpc_server, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.rpc_server = rpc_server
        self.mjai_client: dict[str, MjaiPlayerClient]={}
        self.liqi: dict[str, LiqiProto]={}
        self.bridge: dict[str, MajsoulBridge]={}
        self.active_flows = []
        self.messages_dict = dict() # flow.id -> List[flow_msg]
        self.liqi_msg_dict = dict() # flow.id -> List[liqi_msg]
        self.mjai_msg_dict = dict() # flow.id -> List[mjai_msg]
        self.akagi_log_dict= dict() # flow.id -> List[akagi_log]
        self.loguru_log = [] # List[loguru_log]

        self.four_mjai_client = []
        used_port = get_container_ports()
        port_num = PORT_NUM
        four_port_num = []
        for i in range(4):
            while port_num in used_port:
                port_num+=1
            four_port_num.append(port_num)
            used_port.append(port_num)
            
        with ThreadPoolExecutor(max_workers=4) as executor:
            futures = {executor.submit(self.launch_client, i, four_port_num[i], submission) for i in range(4)}
            self.four_mjai_client = [future.result() for future in futures]
        
        self.four_mjai_client.sort(key=lambda x: x.player_id)
        
    def launch_client(self, i, port_num, submission):
        client = MjaiPlayerClient(submission, timeout=15, port_num=port_num)
        client.launch_container(i)
        return client

    def on_mount(self) -> None:
        self.update_flow = self.set_interval(1, self.refresh_flow)
        self.get_messages_flow = self.set_interval(0.05, self.get_messages)
        
    # on_event
    # def on_event(self, event: Event) -> Coroutine[Any, Any, None]:
    #     if isinstance(event, ScreenResume):
    #         self.update_flow.resume()
    #         self.get_messages_flow.resume()

    def refresh_flow(self) -> None:
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
                self.mjai_client[flow_id].delete_container()
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
                used_port = get_container_ports()
                port_num = PORT_NUM
                while port_num in used_port:
                    port_num+=1
                self.mjai_client[flow_id] = MjaiPlayerClient(
                    submission,
                    timeout=15,
                    port_num=port_num
                )
                self.liqi[flow_id] = LiqiProto()
                self.bridge[flow_id] = MajsoulBridge()

    def get_messages(self):
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
                    mjai_msg = self.bridge[flow_id].input(self.four_mjai_client, liqi_msg)
                    if mjai_msg is not None:
                        if self.bridge[flow_id].reach and mjai_msg["type"] == "dahai":
                            mjai_msg["type"] = "reach"
                            self.bridge[flow_id].reach = False
                        self.mjai_msg_dict[flow_id].append(mjai_msg)

    def compose(self) -> ComposeResult:
        """Called to add widgets to the app."""
        yield Header()
        yield Footer()
        yield ScrollableContainer(id="FlowContainer")

    def on_event(self, event: Event) -> Coroutine[Any, Any, None]:
        return super().on_event(event)
    
    def my_sink(self, message) -> None:
        record = message.record
        self.loguru_log.append(f"{record['time'].strftime('%H:%M:%S')} | {record['level'].name}\t | {record['message']}")

    def action_quit(self) -> None:
        self.update_flow.stop()
        self.get_messages_flow.stop()
        self.rpc_server.reset_message_idx()
        for flow_id in self.active_flows:
            self.mjai_client[flow_id].delete_container()
        for mjai_client in self.four_mjai_client:
            mjai_client.delete_container()
        self.exit()

    def action_settings(self) -> None:
        self.push_screen(SettingsScreen())
        pass


if __name__ == '__main__':
    with open("settings.json", "r") as f:
        settings = json.load(f)
        rpc_port = settings["Port"]["XMLRPC"] 
    rpc_host = "127.0.0.1"
    s = ServerProxy(f"http://{rpc_host}:{rpc_port}", allow_none=True)
    s.reset_message_idx()
    app = Akagi(rpc_server=s)
    logger.add(app.my_sink)
    logger.add("akagi.log")
    logger.level("CLICK", no=10, icon="CLICK")
    try:
        app.run()
    except Exception as e:
        containers = docker.from_env().containers.list()
        for container in containers:
            if container.image.tags[0] == 'smly/mjai-client:v3':
                container.stop()
                container.remove()
        raise e
    