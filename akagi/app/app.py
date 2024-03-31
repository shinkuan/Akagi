import pathlib
import tkinter as tk
from tkinter import ttk
import sv_ttk

from akagi.common import start_message_controller, stop_message_controller, start_mitm, stop_mitm

ASSETS_PATH = pathlib.Path(__file__).parent / "assets"


class App(tk.Tk):
    def __init__(self):
        super().__init__()

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
        
        self.parent: App = parent
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
        self.parent.body.destroy()
        self.parent.body = MITMWindow(self.parent)

    def on_button_flow_pressed(self):
        self.set_active_button(self.button_flow)


class MITMWindow(ttk.Frame):
    def __init__(self, parent, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)

        self.parent = parent
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
        start_message_controller()
        pass

    def on_stop_pressed(self):
        stop_message_controller()
        stop_mitm()
        pass


if __name__ == "__main__":
    app = App()
    app.mainloop()