import sys
from my_logger import both_logger
from .common import start_message_controller, stop_message_controller, start_mitm, stop_mitm
from .app import App
from .config import config as akagi_config

app = App()

def stop():
    global app
    stop_message_controller()
    stop_mitm()
    app.destroy()
    sys.exit(0)

def main():
    global app
    both_logger.info("Starting Akagi")
    app.protocol("WM_DELETE_WINDOW", stop)    

    while True:
        try:
            app.mainloop()
        except KeyboardInterrupt:
            stop()
        pass


if __name__ == "__main__":
    main()