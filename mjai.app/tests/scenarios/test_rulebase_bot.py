import os
import tempfile
from pathlib import Path

from mjai.game import Simulator


def test_regular_game():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/rulebase.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
            ]
            game = Simulator(submissions, dirpath)
            is_end = game._internal_run(0)

            for player_idx in range(4):
                assert (
                    Path(dirpath) / f"0.player{player_idx}.stderr.log"
                ).exists()
                assert (
                    Path(dirpath) / f"0.player{player_idx}.stdout.log"
                ).exists()

            print((Path(dirpath) / "0.player1.stdout.log").open("r").read())
            print((Path(dirpath) / "0.player1.stderr.log").open("r").read())

            assert (Path(dirpath) / "0.game.error.json").exists()
            assert (Path(dirpath) / "0.game.error.json").open(
                "r"
            ).read() == "{}"
            assert is_end is None  # type: ignore
