import gzip
import json
import os
import tempfile
from pathlib import Path

import pytest
from mjai.engine import DockerMjaiLogEngine
from mjai.exceptions import EngineRuntimeError
from mjai.game import Simulator
from mjai.mlibriichi.arena import Match

from mjai import MjaiPlayerClient


def test_regular_game():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/tsumogiri.zip",
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

            assert (Path(dirpath) / "0.game.error.json").exists()
            assert (Path(dirpath) / "0.game.error.json").open(
                "r"
            ).read() == "{}"
            assert is_end is None


def test_timeout_game():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
                "examples/timeoutbot.zip",
            ]
            with pytest.raises(EngineRuntimeError):
                game = Simulator(submissions, dirpath)
                game._internal_run(0)


def test_regular_game_resume():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            env = Match(log_dir=dirpath)
            assert env is not None
            submissions = [
                "examples/shanten.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
            ]
            timeout = 2.0
            port = 28088
            players, agents = [], []
            for player_idx, submission in enumerate(submissions):
                player = MjaiPlayerClient(
                    submission, timeout=timeout, port_num=port + player_idx
                )
                player.launch_container(player_idx)
                players.append(player)
                agent = DockerMjaiLogEngine(
                    name=str(player_idx), player=player
                )
                agents.append(agent)

            try:
                scores = [25000, 25000, 50000, 0]
                kyoku = 7
                honba = 0
                kyotaku = 0
                env.py_match_continue(
                    agents[0],
                    agents[1],
                    agents[2],
                    agents[3],
                    scores,
                    kyoku,
                    honba,
                    kyotaku,
                    seed_start=(10000, 2000),
                )
            finally:
                for player_idx, player in enumerate(players):
                    player.delete_container()

            with gzip.open(Path(dirpath) / "10000_2000_a.json.gz", "r") as f:
                lines = [json.loads(line) for line in f]
                # with (Path(dirpath) / "0.mjai.jsonl").open("r") as f:
                #     lines = [json.loads(line) for line in f]
                assert lines[1]["type"] == "start_kyoku"
                assert lines[1]["bakaze"] == "S"
                assert lines[1]["kyoku"] == 4
                assert lines[1]["honba"] == 0
                assert lines[1]["kyotaku"] == 0
                assert lines[1]["oya"] == 3
                assert lines[1]["scores"][0] == 25000
                assert lines[1]["scores"][1] == 25000
                assert lines[1]["scores"][2] == 50000
                assert lines[1]["scores"][3] == 0
                assert lines[-3]["type"] == "ryukyoku"
                assert lines[-3]["deltas"][0] == 3000
                assert lines[-3]["deltas"][1] == -1000
                assert lines[-3]["deltas"][2] == -1000
                assert lines[-3]["deltas"][3] == -1000


def test_regular_game_resume2():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            Match(log_dir=dirpath)
            next_state = {
                "scores": [25000, 25000, 50000, 0],
                "kyoku": 7,
                "honba": 0,
                "kyotaku": 0,
            }
            submissions = [
                "examples/shanten.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
            ]
            game = Simulator(submissions, dirpath)
            is_end = game._internal_run(0, next_state)
            assert is_end is None

            with (Path(dirpath) / "0.mjai.jsonl").open("r") as f:
                lines = [json.loads(line) for line in f]
                assert lines[1]["type"] == "start_kyoku"
                assert lines[1]["bakaze"] == "S"
                assert lines[1]["kyoku"] == 4
                assert lines[1]["honba"] == 0
                assert lines[1]["kyotaku"] == 0
                assert lines[1]["oya"] == 3
                assert lines[1]["scores"][0] == 25000
                assert lines[1]["scores"][1] == 25000
                assert lines[1]["scores"][2] == 50000
                assert lines[1]["scores"][3] == 0
                assert lines[-3]["type"] == "ryukyoku"
                assert lines[-3]["deltas"][0] == 3000
                assert lines[-3]["deltas"][1] == -1000
                assert lines[-3]["deltas"][2] == -1000
                assert lines[-3]["deltas"][3] == -1000

            next_state = game.get_next_state(Path(dirpath) / "0.mjai.jsonl")
            assert next_state is None


def test_run_regular_game():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
            ]
            game = Simulator(submissions, dirpath)
            game.run()

            assert (Path(dirpath) / "mjai_log.json").exists()
            assert (Path(dirpath) / "errors.json").exists()
            assert (Path(dirpath) / "player_logs.0.stderr.txt").exists()
            assert (Path(dirpath) / "player_logs.0.stdout.txt").exists()


def test_run_with_timeout_game():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/timeoutbot.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
            ]
            # NOTE: start_game で timeout する場合は例外終了
            # validation step で排除されるため想定していない
            with pytest.raises(EngineRuntimeError):
                game = Simulator(submissions, dirpath)
                game.run()

            assert not (Path(dirpath) / "mjai_log.json").exists()
            assert not (Path(dirpath) / "errors.json").exists()
            assert not (Path(dirpath) / "player_logs.0.stderr.txt").exists()
            assert not (Path(dirpath) / "player_logs.0.stdout.txt").exists()


def test_run_with_timeout2_game():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        with tempfile.TemporaryDirectory() as dirpath:
            submissions = [
                "examples/shanten.zip",
                "examples/timeoutbot2.zip",
                "examples/tsumogiri.zip",
                "examples/tsumogiri.zip",
            ]
            game = Simulator(submissions, dirpath)
            game.run()

            # タイムアウトによる中断をはさみ再起動をはさみ２回実行される
            assert (Path(dirpath) / "mjai_log.json").exists()
            assert (Path(dirpath) / "errors.json").exists()
            assert (Path(dirpath) / "player_logs.0.stderr.txt").exists()
            assert (Path(dirpath) / "player_logs.0.stdout.txt").exists()

            print((Path(dirpath) / "summary.json").open("r").read())

            # # S3-7honba で actor 2 が timeout によるエラー罰則
            # # [31000,23000,23000,23000] から [2000,-6900,2900,2000] の得点移動
            # with (Path(dirpath) / "0.mjai.jsonl").open("r") as f:
            #     lines = [json.loads(line) for line in f]
            #     assert lines[-3]["type"] == "ryukyoku"
            #     assert lines[-3]["reason"] == "error"
            #     assert lines[-3]["deltas"][0] == 2000
            #     assert lines[-3]["deltas"][1] == -6900
            #     assert lines[-3]["deltas"][2] == 2900
            #     assert lines[-3]["deltas"][3] == 2000

            # # S3-8honba から再開。actor 2 が親含めて他家からロンされたものと同様に扱うため
            # # [33000,16100,25900,25000] から再開
            # with (Path(dirpath) / "1.mjai.jsonl").open("r") as f:
            #     lines = [json.loads(line) for line in f]
            #     assert lines[1]["type"] == "start_kyoku"
            #     assert lines[1]["bakaze"] == "S"
            #     assert lines[1]["kyoku"] == 3
            #     assert lines[1]["honba"] == 8
            #     assert lines[1]["kyotaku"] == 0
