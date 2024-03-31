import pathlib
import tkinter as tk
from tkinter import ttk
import sv_ttk


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

        sv_ttk.set_theme("dark")


class Header(ttk.Frame):
    def __init__(self, parent, *args, **kwargs):
        super().__init__(parent, *args, **kwargs)
        
        self.parent = parent
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
        pass

    def on_button_logs_pressed(self):
        self.set_active_button(self.button_logs)

    def on_button_mitm_pressed(self):
        self.set_active_button(self.button_mitm)

    def on_button_flow_pressed(self):
        self.set_active_button(self.button_flow)


if __name__ == "__main__":
    app = App()
    app.mainloop()