import threading
from .app import App
from my_logger import both_logger

app = None
app_thread = None


def set_window_destory_handler(callback: callable):
    global app, app_thread

    if app is not None:
        app.protocol("WM_DELETE_WINDOW", callback)
        both_logger.info("Window destroy handler set")

def start_app():
    global app, app_thread

    if app is not None:
        both_logger.info("App already running")
        return
    
    app = App()
    app_thread = threading.Thread(target=app.mainloop)

def stop_app():
    global app, app_thread

    if app is None:
        both_logger.info("App is not running")
        return
    
    app.destroy()
    app_thread.join(timeout=0.2)
    both_logger.info("App stopped")
    app = None
    app_thread = None