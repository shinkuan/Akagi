import os
import sys
import pathlib
import subprocess

def start_mitm():
    command = [sys.executable, "-m", "mitm"]

    if sys.platform == "win32":
        # Windows特定代码
        mitm_exec = subprocess.Popen(command, creationflags=subprocess.CREATE_NEW_CONSOLE)
    else:
        # macOS和其他Unix-like系统
        mitm_exec = subprocess.Popen(command, preexec_fn=os.setsid)

    return mitm_exec

if __name__ == "__main__":
    mitm_exec = start_mitm()