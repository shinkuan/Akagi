from gevent import monkey; monkey.patch_socket()

import copy
import pathlib
import tkinter as tk
from PIL import Image, ImageTk
from tkinter import ttk

import sv_ttk
from my_logger import both_logger
from akagi.common import start_message_controller, stop_message_controller, start_mitm, stop_mitm
from akagi.message_controller import MessageController
from akagi.libriichi_helper import meta_to_recommend

ASSETS_PATH = pathlib.Path(__file__).parent / "assets"


class App(tk.Tk):
    def __init__(self):
        super().__init__()

        self.message_controller: MessageController = None

        self.title("Akagi")
        self.geometry("1280x720")
        self.iconbitmap(default=ASSETS_PATH / "icon.ico")

        self.header = Header(self, padding=15)
        self.separator = ttk.Separator(self)
        self.separator.pack(fill="x", pady=10, padx=15)

        self.body = ttk.Frame(self)
        self.body.pack(fill="both", expand=True)
        self.body.columnconfigure(0, weight=1)
        self.body.rowconfigure(0, weight=1)

        sv_ttk.set_theme("dark")


class Header(ttk.Frame):
    def __init__(self, parent: App, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)
        
        self.app: App = parent
        self.pack(side="top", fill="x")

        self.logo_label = ttk.Label(self, text="Akagi", font=('Arial', 24, 'bold'))
        self.logo_label.pack(side="left", padx=(20, 0))

        self.buttons: list[ttk.Button] = []

        self.button_settings = ttk.Button(self, text="Settings", command=self.on_button_settings_pressed)
        self.button_settings.pack(side="right", padx=5)
        self.buttons.append(self.button_settings)

        self.button_logs = ttk.Button(self, text="Logs", command=self.on_button_logs_pressed)
        self.button_logs.pack(side="right", padx=5)
        self.buttons.append(self.button_logs)

        self.button_mitm = ttk.Button(self, text="MITM", command=self.on_button_mitm_pressed)
        self.button_mitm.pack(side="right", padx=5)
        self.buttons.append(self.button_mitm)

        self.button_flow = ttk.Button(self, text="Flow", command=self.on_button_flow_pressed)
        self.button_flow.pack(side="right", padx=5)
        self.buttons.append(self.button_flow)

        self.bind("<Configure>", self.update_button_width)

    def update_button_width(self, event):
        total_width = self.winfo_width()
        button_width = int(total_width * 0.01)

        for button in self.buttons:
            button.config(width=button_width)

    def set_active_button(self, button: ttk.Button):
        for b in self.buttons:
            b.config(style="Accent.TButton" if b == button else "TButton")

    def on_button_settings_pressed(self):
        self.set_active_button(self.button_settings)
        # TODO: Open settings window

    def on_button_logs_pressed(self):
        self.set_active_button(self.button_logs)

    def on_button_mitm_pressed(self):
        self.set_active_button(self.button_mitm)
        self.app.body.destroy()
        self.app.body = MITMWindow(self.app)

    def on_button_flow_pressed(self):
        self.set_active_button(self.button_flow)
        self.app.body.destroy()
        self.app.body = FlowWindow(self.app)


class MITMWindow(ttk.Frame):
    def __init__(self, parent: App, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)

        self.app: App = parent
        self.pack(fill="both", expand=True)
        self.columnconfigure(0, weight=1)
        self.rowconfigure(1, weight=1)

        self.start_button = ttk.Button(self, text="Start MITM", command=self.on_start_pressed, width=25)
        self.start_button.grid(row=0, column=0, padx=15, pady=10, sticky="e")

        self.stop_button = ttk.Button(self, text="Stop MITM", command=self.on_stop_pressed, width=25)
        self.stop_button.grid(row=0, column=1, padx=15, pady=10, sticky="w")

        self.grid_columnconfigure(0, weight=1)
        self.grid_columnconfigure(1, weight=1)

    def on_start_pressed(self):
        start_mitm()
        self.app.message_controller = start_message_controller()
        pass

    def on_stop_pressed(self):
        self.app.message_controller = stop_message_controller()
        stop_mitm()
        pass


class FlowWindow(ttk.Frame):
    def __init__(self, parent: App, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)
        
        self.app: App = parent
        self.pack(fill="both", expand=True)
        self.columnconfigure(0, weight=1)
        self.rowconfigure(1, weight=1)

        self.notebook = ttk.Notebook(self)
        self.notebook.pack(expand=True, fill="both")

        self.activated_flows_tabs: dict[str, FlowTab] = {}

        if self.app.message_controller is None:
            # TODO: Show something here
            return

        for flow in self.app.message_controller.activated_flows:
            self.activated_flows_tabs[flow] = FlowTab(self.notebook, flow)
            self.notebook.add(self.activated_flows_tabs[flow], text=f"Flow {flow}")

        self.app.message_controller.bind_observer_flows(
            self.on_activated_flows_changed)

        self.app.message_controller.bind_observer_mjai_messages(
            self.on_mjai_message_received)

    def on_activated_flows_changed(self, activated_flows: list[str]):
        for flow in activated_flows:
            if flow in self.activated_flows_tabs:
                continue
            self.activated_flows_tabs[flow] = FlowTab(self.notebook, flow)
            self.notebook.add(self.activated_flows_tabs[flow], text=f"Flow {flow}")

        self_activated_flows = copy.deepcopy(self.activated_flows_tabs.keys())
        for flow in self_activated_flows:
            if not flow in activated_flows:
                self.notebook.forget(self.activated_flows_tabs[flow])
                del self.activated_flows_tabs[flow]

    def on_mjai_message_received(self, flow_id: str, mjai_msg: dict):
        both_logger.info(f"{mjai_msg}")
        if flow_id in self.activated_flows_tabs.keys():
            bot = self.app.message_controller.bridge[flow_id].mjai_client.bot
            if bot is None:
                return
            self.activated_flows_tabs[flow_id].tehai_viewer.update_tehai(bot.state())
            meta = meta_to_recommend(mjai_msg.get("meta"))
            self.activated_flows_tabs[flow_id].indicator_0.update_content(meta[0][0], meta[0][1])
            if len(meta) == 1:
                assert meta[1][0] == "reach"
                self.activated_flows_tabs[flow_id].indicator_1.update_content(mjai_msg['pai'], 1.0)
            else:
                self.activated_flows_tabs[flow_id].indicator_1.update_content(meta[1][0], meta[1][1])
            if len(meta) > 2:
                self.activated_flows_tabs[flow_id].indicator_2.update_content(meta[2][0], meta[2][1])
            else:
                self.activated_flows_tabs[flow_id].indicator_2.update_content("?", 0.0)            

    def destroy(self) -> None:
        if self.app.message_controller is not None:
            self.app.message_controller._observers_flows = []
        return super().destroy()


class FlowTab(ttk.Frame):
    def __init__(self, parent: ttk.Notebook, flow_id: str, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)

        self.app: App = parent.master.app  # Notebook -> FlowWindow -> App
        # self.grid_columnconfigure(0, weight=1)  # 使列0可以扩展，这样中间的部分可以使用额外的空间
        # self.grid_rowconfigure(0, weight=0)  # 使行1可以扩展
        # self.grid_rowconfigure(1, weight=0)  # 使行1可以扩展

        # self.indicator_0 = Indicator(self)
        # self.indicator_0.grid(row=0, column=0)

        # self.indicator_1 = Indicator(self)
        # self.indicator_1.grid(row=0, column=1)

        # self.indicator_2 = Indicator(self)
        # self.indicator_2.grid(row=0, column=2)

        # self.separator = ttk.Separator(self)
        # self.separator.grid(row=1, column=0, columnspan=3, sticky="ew", pady=10)

        # self.tehai_viewer_frame_container = ttk.Frame(self)
        # self.tehai_viewer_frame_container.grid(row=1, column=0)  # 放置在中间的行

        # self.tehai_viewer: TehaiViewer = TehaiViewer(self.tehai_viewer_frame_container)
        # self.tehai_viewer.pack()  # TehaiViewer内部的布局管理器不变

        self.indicator_0 = Indicator(self)
        self.indicator_0.pack(side="top", padx=10, pady=10)

        self.indicator_1 = Indicator(self)
        self.indicator_1.pack(side="top", padx=10, pady=10)

        self.indicator_2 = Indicator(self)
        self.indicator_2.pack(side="top", padx=10, pady=10)

        self.separator = ttk.Separator(self, orient="horizontal")
        self.separator.pack(fill="x", padx=10, pady=10)

        self.tehai_viewer: TehaiViewer = TehaiViewer(self)
        self.tehai_viewer.pack(side="top", padx=10, pady=10)

        self.flow_id = flow_id

        if self.app.message_controller is not None:
            bot = self.app.message_controller.bridge[self.flow_id].mjai_client.bot
            if bot is not None:
                self.tehai_viewer.update_tehai(bot.state())


PAI_STR = [
    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
    "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
    "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
    "E",  "S",  "W",  "N",  "P",  "F",  "C",  "?",
    "5mr", "5pr", "5sr"
]


class Indicator(ttk.Frame):
    def __init__(self, parent: FlowTab, *args, **kwargs):
        height = 200
        super().__init__(parent, height=height, *args, **kwargs)

        self.app: App = parent.app
        self.pack(side="top", fill="x")
        
        action = "?"
        weight = 0.0
        self.action_frame = Pai(self, action, width=200)
        self.action_frame.pack(side="left", padx=10, pady=10)

        self.separator = ttk.Separator(self, orient="vertical")
        self.separator.pack(side="left", fill="y", padx=10, pady=10)

        self.weight_label = ttk.Label(self, text=f"Weight: {weight*100:.2f}%", font=('Arial', 24, 'bold'))        
        self.weight_label.pack(side="left", padx=10, pady=10)

    def update_content(self, action: str, weight: float):
        self.action_frame.update_content(action)
        self.weight_label.configure(text=f"Weight: {weight*100:.2f}%")


class TehaiViewer(ttk.Frame):
    def __init__(self, parent: FlowTab, libriichi_state = None, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)

        self.app: App = parent.app
        self.pack(fill="x")
        
        self.libriichi_state = libriichi_state
        if self.libriichi_state is not None:
            tile_list, tsumohai = self.state_to_tehai()
        else:
            tile_list = ["?"]*13
            tsumohai = "?"

        self.pai_frame_list: list[Pai] = []
        self.tsumohai_frame = None
        for tile in tile_list:
            pai = Pai(self, tile)
            pai.pack(side="left", padx=(1,0), pady=5)
            self.pai_frame_list.append(pai)
        pai = Pai(self, tsumohai)
        pai.pack(side="left", padx=(20,0), pady=5)
        self.tsumohai_frame = pai

    def update_tehai(self, libriichi_state):
        self.libriichi_state = libriichi_state
        tile_list, tsumohai = self.state_to_tehai()
        for idx, tile in enumerate(tile_list):
            self.pai_frame_list[idx].update_content(tile)
        self.tsumohai_frame.update_content(tsumohai)

    def state_to_tehai(self) -> tuple[list[str], str]:
        tehai34 = self.libriichi_state.tehai # with tsumohai, no aka marked
        akas = self.libriichi_state.akas_in_hand
        tsumohai = self.libriichi_state.last_self_tsumo()
        return self._state_to_tehai(tehai34, akas, tsumohai)

    def _state_to_tehai(self, tile34: int, aka: list[bool], tsumohai: str|None) -> tuple[list[str], str]:
        pai_str = [
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
            "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
            "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
            "E",  "S",  "W",  "N",  "P",  "F",  "C",  "?"
        ]
        aka_str = [
            "5mr", "5pr", "5sr"
        ]
        tile_list = []
        for tile_id, tile_count in enumerate(tile34):
            for _ in range(tile_count):
                tile_list.append(pai_str[tile_id])
        for idx, aka in enumerate(aka):
            if aka:
                tile_list[tile_list.index("5" + ["m", "p", "s"][idx])] = aka_str[idx]
        if len(tile_list)%3 == 2 and tsumohai is not None:
            tile_list.remove(tsumohai)
        else:
            tsumohai = "?"
        len_tile_list = len(tile_list)
        if len_tile_list < 13:
            tile_list += ["?"]*(13-len_tile_list)

        return (tile_list, tsumohai)


MJAI_2_ASSETS = {
    "1m": "Man1",
    "2m": "Man2",
    "3m": "Man3",
    "4m": "Man4",
    "5m": "Man5",
    "6m": "Man6",
    "7m": "Man7",
    "8m": "Man8",
    "9m": "Man9",
    "1p": "Pin1",
    "2p": "Pin2",
    "3p": "Pin3",
    "4p": "Pin4",
    "5p": "Pin5",
    "6p": "Pin6",
    "7p": "Pin7",
    "8p": "Pin8",
    "9p": "Pin9",
    "1s": "Sou1",
    "2s": "Sou2",
    "3s": "Sou3",
    "4s": "Sou4",
    "5s": "Sou5",
    "6s": "Sou6",
    "7s": "Sou7",
    "8s": "Sou8",
    "9s": "Sou9",
    "E": "Ton",
    "S": "Nan",
    "W": "Shaa",
    "N": "Pei",
    "P": "Haku",
    "F": "Hatsu",
    "C": "Chun",
    "?": "Blank",
    "5mr": "Man5-Dora",
    "5pr": "Pin5-Dora",
    "5sr": "Sou5-Dora"
}


class Pai(ttk.Frame):
    def __init__(self, parent: TehaiViewer | Indicator, pai: str, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)

        self.app: App = parent.app
        self.pack()
        
        self.label = None
        self.pai = pai
        if pai in PAI_STR:
            width = 80
        else:
            width = 200
        self.display_image(pai, width)

        # self.bind("<Configure>", self.on_resize)

    def combine_images(self, pai: str):
        if pai in PAI_STR:
            front_image = ASSETS_PATH / "Black" / f"Outline.png"
            pai_image = ASSETS_PATH / "Black" / f"{MJAI_2_ASSETS[pai]}.png"
            image1 = Image.open(front_image).convert("RGBA")
            image2 = Image.open(pai_image).convert("RGBA")
            
            combined = Image.alpha_composite(image1, image2)
        else:
            combined = Image.open(ASSETS_PATH / "Action" / f"{pai}.png").convert("RGBA")

        return combined

    def get_resized_image(self, pai: str, width: int):
        combined_image = self.combine_images(pai)

        height = int((width / combined_image.width) * combined_image.height)
        resized_image = combined_image.resize((width, height))

        return resized_image

    def display_image(self, pai: str, width: int):
        resized_image = self.get_resized_image(pai, width)

        tk_image = ImageTk.PhotoImage(resized_image)
        
        self.label = ttk.Label(self, image=tk_image)
        self.label.image = tk_image  # 防止圖像被垃圾回收
        self.label.pack()

    def on_resize(self, event):
        self.display_image(self.pai, event.width)

    def update_content(self, pai: str):
        self.pai = pai
        if pai in PAI_STR:
            width = 80
        else:
            width = 200
        resized_image = self.get_resized_image(pai, width)
        tk_image = ImageTk.PhotoImage(resized_image)
        self.label.configure(image=tk_image)
        self.label.image = tk_image # 防止圖像被垃圾回收


if __name__ == "__main__":
    app = App()
    app.mainloop()