import json
import sys
import time


class Bot:
    def __init__(self, actor_id):
        self.actor_id = actor_id
        self.index = 0

    def react(self, events: str) -> str:
        events = json.loads(events)
        assert len(events) > 0

        # しばらく正常で途中からタイムアウト
        self.index += 1
        if self.index > 500:
            sys.stderr.write("sleep(10)!")
            time.sleep(10.0)

        # 最後のイベントの `type` によって分岐
        if events[-1]["type"] == "tsumo":  # type: ignore
            # 最後にツモった牌を捨てる
            return json.dumps({
                "type": "dahai",
                "pai": events[-1]["pai"],  # type: ignore
                "actor": self.actor_id,
                "tsumogiri": True,
            }, separators=(",", ":"))
        else:
            # それ以外
            # 他人の打牌に対して常にアクションしない
            return json.dumps({
                "type": "none",
            }, separators=(",", ":"))


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
