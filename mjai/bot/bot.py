import json
import time
import sys
import hashlib
import pathlib
import requests

from my_logger import logger

from . import model


class Bot:
    def __init__(self, player_id: int):
        self.player_id = player_id
        model_path = pathlib.Path(__file__).parent / f"mortal.pth"
        self.model = model.load_model(player_id)
        with open(model_path, "rb") as f:
            self.model_hash = hashlib.sha256(f.read()).hexdigest()
        try:
            with open(pathlib.Path(__file__).parent.parent / "online.json", "r") as f:
                online_json = json.load(f)
                self.online = online_json["online"]
                if not self.online:
                    return
                api_key = online_json["api_key"]
                server = online_json["server"]
                headers = {
                    'Authorization': api_key,
                }
                r = requests.post(f"{server}/check", headers=headers)
                r_json = r.json()
                if r_json["result"] == "success":
                    self.model_hash = "online"
        except Exception as e:
            logger.error(e)
            self.online = False

    def react(self, events: str) -> str:
        events = json.loads(events)

        start = time.time()
        return_action = None
        for e in events:
            return_action = self.model.react(json.dumps(e, separators=(",", ":")))
        time_elapsed = time.time() - start

        if return_action is None:
            return json.dumps({"type":"none", "time":time_elapsed}, separators=(",", ":"))
        else:
            raw_data = json.loads(return_action)
            raw_data["time"] = time_elapsed
            if self.online:
                raw_data["online"] = model.online_valid
            return json.dumps(raw_data, separators=(",", ":"))

    def state(self):
        return self.model.state
        

def main():
    player_id = int(sys.argv[1])
    assert player_id in range(4)
    bot = Bot(player_id)

    while True:
        line = sys.stdin.readline().strip()
        resp = bot.react(line)
        sys.stdout.write(resp + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()