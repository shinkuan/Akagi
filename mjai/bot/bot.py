import json
import sys

from loguru import logger

from . import model


class Bot:
    def __init__(self, player_id: int):
        self.player_id = player_id
        self.model = model.load_model(player_id)

    def react(self, events: str) -> str:
        events = json.loads(events)

        # logger.info("hi")
        return_action = None
        for e in events:
            return_action = self.model.react(json.dumps(e, separators=(",", ":")))

        if return_action is None:
            return json.dumps({"type":"none"}, separators=(",", ":"))
        else:
            raw_data = json.loads(return_action)
            del raw_data["meta"]
            return json.dumps(raw_data, separators=(",", ":"))


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
    # debug()
    main()