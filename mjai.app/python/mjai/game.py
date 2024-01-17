import gzip
import json
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Any

from loguru import logger
from mjai.engine import DockerMjaiLogEngine
from mjai.exceptions import EngineRuntimeError, TimeoutExpired
from mjai.mlibriichi.arena import Match  # type: ignore
from mjai.player import MjaiPlayerClient


def to_rank(scores):
    scores = scores.copy()
    scores[0] = -scores[0] - 0.3  # 同着時に起家優先
    scores[1] = -scores[1] - 0.2
    scores[2] = -scores[2] - 0.1
    scores[3] *= -1

    player_idx_by_rank = [
        idx for _, idx in sorted(zip(scores, list(range(4))))
    ]
    player_rank_map = {
        player_idx: rank_idx
        for rank_idx, player_idx in enumerate(player_idx_by_rank)
    }
    return [player_rank_map[player_idx] + 1 for player_idx in range(4)]


def kyoku_to_zero_indexed_kyoku(bakaze: str, kyoku: int) -> int:
    if bakaze == "E":
        return kyoku - 1  # 東
    elif bakaze == "S":
        return kyoku - 1 + 4  # 南
    else:
        return kyoku - 1 + 8  # 西


class Simulator:
    def __init__(
        self,
        submissions: list[Path] | list[str],
        logs_dir: Path | str,
        timeout: float = 2.0,
        port: int = 28090,
        seed: tuple[int, int] = (10000, 2000),
    ):
        self.submissions = submissions
        self.timeout = timeout
        self.logs_dir = Path(logs_dir)
        self.port = port
        self.seed = seed

    def run(self, dup_index: int = 0):
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        max_run = 10
        next_state: dict | None = None
        for run_idx in range(max_run):
            next_state = self._internal_run(run_idx, next_state)
            if next_state is None:
                break

        logger.info("Merge log files")
        self.merge_logs()

    def merge_logs(self):
        assert self.logs_dir.exists()

        # mjai_log.json
        start_game = ""
        jsonl_lines = []
        for idx, jsonl_path in enumerate(
            sorted(self.logs_dir.glob("*.mjai.jsonl"))
        ):
            for line in jsonl_path.open("r"):
                if line.startswith('{"type":"end_game"}'):
                    continue
                if line.startswith('{"type":"start_game"'):
                    start_game = line
                    continue
                jsonl_lines.append(line)
            jsonl_path.unlink()
        with (self.logs_dir / "mjai_log.json").open("w") as f:
            f.write(
                "".join([start_game] + jsonl_lines + ['{"type":"end_game"}'])
            )

        # errors.json
        errors_json = []
        for error_path in sorted(self.logs_dir.glob("*.game.error.json")):
            errors_json.append(json.load(error_path.open("r")))
            error_path.unlink()
        json.dump(
            errors_json, (self.logs_dir / "errors.json").open("w"), indent=4
        )

        # player_logs.0.stdout.txt, player_logs.0.stderr.txt
        for player_idx in range(4):
            for log_type in ["stderr", "stdout"]:
                content = ""
                for error_path in sorted(
                    self.logs_dir.glob(f"*.player{player_idx}.{log_type}.log")
                ):
                    content += error_path.open("r").read()
                    error_path.unlink()
                with (
                    self.logs_dir / f"player_logs.{player_idx}.{log_type}.txt"
                ).open("w") as f:
                    f.write(content)

        json.dump(
            self._summarize_mjai_log(self.logs_dir / "mjai_log.json"),
            (self.logs_dir / "summary.json").open("w"),
        )

    def get_next_state(self, jsonl_path: Path | str) -> dict | None:
        # エラー終了時の場合に最後の局からゲームを続行するか判断する
        jsonl_path = Path(jsonl_path)
        first_start_kyoku_event: dict[str, Any] | None = None
        last_start_kyoku_event: dict[str, Any] | None = None
        last_kyoku_riichi_count = 0

        with jsonl_path.open("r") as f:
            events = [json.loads(event_str) for event_str in f]

        for event in events:
            if event["type"] == "start_kyoku":
                last_kyoku_riichi_count = 0
                if first_start_kyoku_event is None:
                    first_start_kyoku_event = event
                last_start_kyoku_event = event
            if event["type"] == "reach_accepted":
                last_kyoku_riichi_count += 1

        if first_start_kyoku_event is None:
            raise RuntimeError("can't start game")
        scores: list[int] = first_start_kyoku_event["scores"]  # type: ignore
        last_delta: list[int] = [0, 0, 0, 0]
        for event in events:
            if "deltas" in event:
                for idx in range(4):
                    scores[idx] += event["deltas"][idx]
                last_delta = event["deltas"]

        # 飛び判定
        if min(scores) < 0:
            return None

        # オーラス以降の終局判定
        bakaze: str = last_start_kyoku_event["bakaze"]  # type: ignore
        kyoku: int = last_start_kyoku_event["kyoku"]  # type: ignore
        honba: int = last_start_kyoku_event["honba"]  # type: ignore
        kyotaku: int = last_start_kyoku_event["kyotaku"]  # type: ignore
        oya: int = last_start_kyoku_event["oya"]  # type: ignore
        if bakaze != "E" and kyoku >= 4:
            # オーラス
            # - 親の delta が positive かつ 1位なら終了 (True)
            # - 親の delta が 0 以下なら終了 (True)
            if last_delta[oya] > 0 and max(scores) == scores[oya]:
                return None
            if last_delta[oya] <= 0:
                return None

        # 南1局 → `4` のようにゼロ基準の値に変換する
        zero_indexed_kyoku = kyoku_to_zero_indexed_kyoku(bakaze, kyoku)

        # 親の連続か否か
        if last_delta[oya] > 0:
            return {
                "scores": scores,
                "kyoku": zero_indexed_kyoku,
                "honba": honba + 1,
                "kyotaku": kyotaku,
            }

        # 次の局へ
        return {
            "scores": scores,
            "kyoku": zero_indexed_kyoku + 1,
            "honba": 0,
            "kyotaku": kyotaku + last_kyoku_riichi_count,
        }

    def _internal_run(
        self, run_idx: int = 0, next_state: dict | None = None
    ) -> dict | None:
        next_state_after_run = None

        with tempfile.TemporaryDirectory() as dirpath:
            env = Match(log_dir=dirpath)
            assert env is not None

            players = []
            agents = []
            error_report = {}
            for player_idx, submission in enumerate(self.submissions):
                player = MjaiPlayerClient(
                    submission,
                    timeout=self.timeout,
                    port_num=self.port + player_idx,
                )
                player.launch_container(player_idx)
                players.append(player)
                agent = DockerMjaiLogEngine(
                    name=str(player_idx), player=player
                )
                agents.append(agent)

            try:
                logger.info(f"Start hanchang game. seed_value={self.seed}")
                if next_state is None:
                    env.py_match(
                        agents[0],
                        agents[1],
                        agents[2],
                        agents[3],
                        seed_start=self.seed,
                    )
                else:
                    env.py_match_continue(
                        agents[0],
                        agents[1],
                        agents[2],
                        agents[3],
                        next_state["scores"],
                        next_state["kyoku"],
                        next_state["honba"],
                        next_state["kyotaku"],
                        seed_start=self.seed,
                    )

            except TimeoutExpired as e:
                logger.error(f"Timeout agent: {e.player_id}")
                error_report = {
                    "error_type": "timeout",
                    "exception": str(e),
                    "player_id": e.player_id,
                }
            except RuntimeError as e:
                # logger.error(f"RuntimeError: {str(e)}")
                error_report = {
                    "error_type": "runtime",
                    "exception": str(e),
                    "player_id": -1,
                }
                m = re.search(r"actor: (\d)", str(e))
                if m:
                    error_report["player_id"] = int(m.group(1))  # type: ignore
            except Exception as e:
                logger.error(f"Exception: {str(e)}")
                error_report = {
                    "error_type": "unknown",
                    "exception": str(e),
                    "player_id": -1,
                }
            finally:
                # コンテナログの収集
                logger.info("Collect agent log data...")
                for player_idx, player in enumerate(players):
                    logger.info(f"Collecting from {player.container_name}")
                    commands = [
                        "docker",
                        "logs",
                        player.container_name,
                    ]

                    proc = subprocess.Popen(
                        commands,
                        stderr=subprocess.PIPE,
                        stdout=subprocess.PIPE,
                    )
                    try:
                        outs, errs = proc.communicate(timeout=5)
                        logger.info(
                            f"Writing {run_idx}.player{player_idx}.stdout.log"
                        )
                        with open(
                            self.logs_dir
                            / f"{run_idx}.player{player_idx}.stdout.log",
                            "w",
                        ) as f:
                            if proc.stdout is not None:
                                f.write(outs.decode("utf8"))

                        logger.info(
                            f"Writing {run_idx}.player{player_idx}.stderr.log"
                        )
                        with open(
                            self.logs_dir
                            / f"{run_idx}.player{player_idx}.stderr.log",
                            "w",
                        ) as f:
                            if proc.stderr is not None:
                                f.write(errs.decode("utf8"))

                    except Exception:
                        proc.kill()

                logger.info("Collect agent log data... DONE!")

                logger.info("Delete player containers...")
                for player_idx, player in enumerate(players):
                    player.delete_container()
                logger.info("Delete player containers... DONE!")

            # エラーがある場合、マッチが途中で終わっている
            game_jsons: list[Path] = list(Path(dirpath).glob("*.gz"))

            # ログが保存されていない場合は終了
            if len(game_jsons) == 0:
                json.dump(
                    error_report,
                    (self.logs_dir / f"{run_idx}.game.error.json").open("w"),
                )
                raise EngineRuntimeError(
                    "Cannot start the game: Invalid Agent detected.",
                    error_report["player_id"],
                )

            game_json = game_jsons[0]
            with gzip.open(game_json, "rb") as f:
                jsonl_data = [json.loads(line) for line in f.readlines()]

            # JSONL ログを保存
            (self.logs_dir / f"{run_idx}.mjai.jsonl").open("w").write(
                "\n".join(
                    [
                        json.dumps(line, separators=(",", ":"))
                        for line in jsonl_data
                    ]
                )
            )

            # 最後にエラー流局で終わっている場合はエラーレポートを更新
            for event in reversed(jsonl_data):
                if (
                    event["type"] == "ryukyoku"
                    and "reason" in event
                    and event["reason"] == "error"
                ):
                    error_player_id = min(
                        [(d, i) for i, d in enumerate(event["deltas"])]
                    )[1]
                    error_report = {
                        "error_type": "invalid",
                        "exception": "unexpected message received",
                        "player_id": error_player_id,
                    }
                    break

            # JSONL ログから終了判定
            next_state_after_run = self.get_next_state(
                self.logs_dir / f"{run_idx}.mjai.jsonl"
            )

            # エラーを保存
            json.dump(
                error_report,
                (self.logs_dir / f"{run_idx}.game.error.json").open("w"),
            )

        # 終了判定
        return next_state_after_run

    def end(self, mjai_jsonl_file: Path) -> bool:
        first_start_kyoku_event: dict[str, Any] | None = None
        last_start_kyoku_event: dict[str, Any] | None = None

        with mjai_jsonl_file.open("r") as f:
            events = [json.loads(event_str) for event_str in f]

        for event in events:
            if event["type"] == "start_kyoku":
                if first_start_kyoku_event is None:
                    first_start_kyoku_event = event
                last_start_kyoku_event = event

        scores: list[int] = first_start_kyoku_event["scores"]  # type: ignore
        last_delta: list[int] = [0, 0, 0, 0]
        for event in events:
            if "deltas" in event:
                for idx in range(4):
                    scores[idx] += event["deltas"][idx]
                last_delta = event["deltas"]

        # 飛び判定
        if min(scores) < 0:
            return True

        # オーラス以降の終局判定
        bakaze: str = last_start_kyoku_event["bakaze"]  # type: ignore
        kyoku: int = last_start_kyoku_event["kyoku"]  # type: ignore
        oya: int = last_start_kyoku_event["oya"]  # type: ignore
        if bakaze != "E" and kyoku >= 4:
            # オーラス
            # - 親の delta が positive かつ 1位なら終了 (True)
            # - 親の delta が 0 以下なら終了 (True)
            if last_delta[oya] > 0 and max(scores) == scores[oya]:
                return True
            if last_delta[oya] <= 0:
                return True

        return False

    def _summarize_mjai_log(self, mjai_jsonl_path: str | Path) -> dict:
        mjai_jsonl_path = Path(mjai_jsonl_path)

        summary = {}
        kyoku_summary = []
        with mjai_jsonl_path.open("r") as f:
            deltas = [0, 0, 0, 0]
            start_kyoku_scores = [25000, 25000, 25000, 25000]
            kyoku_info = {}
            for line in f:
                line = line.strip()
                if "reach_accepted" in line:
                    json_data = json.loads(line)
                    actor = json_data["actor"]
                    deltas[actor] = -1000
                if "deltas" in line:
                    json_data = json.loads(line)
                    for idx in range(4):
                        deltas[idx] += json_data["deltas"][idx]
                if "start_kyoku" in line:
                    json_data = json.loads(line)
                    deltas = [0, 0, 0, 0]
                    start_kyoku_scores = json_data["scores"]
                    kyoku_info = {
                        "bakaze": json_data["bakaze"],
                        "kyoku": json_data["kyoku"],
                        "honba": json_data["honba"],
                        "kyotaku": json_data["kyotaku"],
                        "oya": json_data["oya"],
                    }
                if "end_kyoku" in line:
                    end_kyoku_scores = start_kyoku_scores.copy()
                    error_player_idx = None
                    for idx in range(4):
                        end_kyoku_scores[idx] += deltas[idx]

                    kyoku_summary.append(
                        {
                            "start_kyoku_scores": start_kyoku_scores,
                            "end_kyoku_scores": end_kyoku_scores,
                            "kyoku_info": kyoku_info,
                            "error_info": error_player_idx,
                        }
                    )

        summary["kyoku"] = kyoku_summary
        summary["kyoku_count"] = len(kyoku_summary)
        summary["rank"] = to_rank(kyoku_summary[-1]["end_kyoku_scores"])
        return summary
