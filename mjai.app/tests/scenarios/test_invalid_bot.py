import json
import os
import tempfile
from pathlib import Path

from mjai.game import Simulator


def test_invalid_msg():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
                "examples/invalidbot.zip",
            ]
            game = Simulator(submissions, dirpath)
            next_state = game._internal_run(0)

            for player_idx in range(4):
                assert (
                    Path(dirpath) / f"0.player{player_idx}.stderr.log"
                ).exists()
                assert (
                    Path(dirpath) / f"0.player{player_idx}.stdout.log"
                ).exists()

            assert (Path(dirpath) / "0.mjai.jsonl").exists()
            with (Path(dirpath) / "0.mjai.jsonl").open("r") as f:
                # Find last ryukyoku event
                json_lines = [json.loads(line) for line in f]
                for line in json_lines:
                    print(line)
                assert json_lines[-1]["type"] == "end_game"
                assert json_lines[-2]["type"] == "end_kyoku"
                assert json_lines[-3]["type"] == "ryukyoku"
                assert json_lines[-3]["reason"] == "error"
                assert json_lines[-3]["deltas"][0] == 4000
                assert json_lines[-3]["deltas"][1] == 2000
                assert json_lines[-3]["deltas"][2] == 2000
                assert json_lines[-3]["deltas"][3] == -8000

            error_report = json.load(
                (Path(dirpath) / "0.game.error.json").open("r")
            )
            assert error_report["error_type"] == "invalid"
            assert error_report["player_id"] == 3
            assert next_state is not None
            assert next_state["kyoku"] == 0
            assert next_state["scores"] == [29000, 27000, 27000, 17000]
