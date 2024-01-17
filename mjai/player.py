"""
Docker コンテナに submission file を配置して実行するためのラッパー (HTTP版)

1. submission file をマウントした Docker コンテナを起動
2. Docker コンテナに submission file のアーカイブを展開する
3. Docker コンテナ内で `python/mjai/http_server/server.py` を実行する
"""
import json
import shutil
import subprocess
import time
from pathlib import Path

import requests
from loguru import logger
from mjai.exceptions import EngineRuntimeError, TimeoutExpired

MEMORY_SIZE = "1G"
CPU_CORES = "1"


class MjaiPlayerClient:
    def __init__(
        self,
        submission_path: Path | str,
        docker_image_name: str = "docker.io/smly/mjai-client:v3",
        port_num: int = 28088,
        timeout: float = 100,
    ) -> None:
        self.submission_path: Path = Path(submission_path)
        self.docker_image_name = docker_image_name
        self.container_name: str | None = None
        self.proc: subprocess.Popen | None = None
        self.port_num = port_num
        self.timeout = timeout
        self.player_id = 0

    def check_docker_image_available(self) -> bool:
        if shutil.which("docker") is None:
            return False

        command_pull = [
            "docker",
            "pull",
            "docker.io/smly/mjai-client:v3",
        ]
        proc = subprocess.Popen(
            command_pull,
            bufsize=0,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        proc.wait()

        command_images = [
            "docker",
            "images",
            "--format",
            "{{.Repository}}",
            "smly/mjai-client",
        ]
        proc = subprocess.Popen(
            command_images,
            bufsize=0,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        proc.wait()

        if proc.stdout is None:
            return False

        if proc.stdout.readline().strip().decode("utf8") != "smly/mjai-client":
            return False

        return True

    def launch_container(self, player_id: int) -> None:
        self.player_id = player_id
        logger.info(f"Start docker container for player{player_id}")
        command = [
            "docker",
            "run",
            "-d",
            # "--rm",
            "--platform",
            "linux/x86_64",
            "-w",
            "/workspace",
            "-p",
            f"{self.port_num}:3000",
            "--memory",
            MEMORY_SIZE,
            "--cpus",
            CPU_CORES,
            "--mount",
            f"type=bind,src={self.submission_path.resolve()},dst=/bot.zip,readonly",  # noqa: E501
            "--mount",
            f"type=bind,src={Path(__file__).parent / 'http_server/server.py'},dst=/workspace/00__server__.py,readonly",  # noqa: E501
            self.docker_image_name,
            "/workspace/.pyenv/shims/python",
            "-u",
            "/workspace/00__server__.py",
            f"{player_id}",
        ]
        logger.info("cmd: " + " ".join(command))
        proc = subprocess.Popen(
            command,
            bufsize=0,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        assert proc.stdout is not None
        self.container_name = proc.stdout.readline().strip().decode("utf8")
        logger.info(f"Started. Container ID: {self.container_name}")

        # Wait until the server is ready
        logger.info(f"Wait until the player {player_id} is ready")
        # Workaround: http server が立ち上がる前にリクエストが飛ぶとエラーになる
        time.sleep(1.0)

        wait_start_ts = time.time()
        while True:
            try:
                resp = requests.get(
                    f"http://localhost:{self.port_num}", timeout=100
                )
                if resp.status_code == 200:
                    break
            except requests.exceptions.ConnectionError:
                time.sleep(0.1)
                if time.time() - wait_start_ts > 100.0:
                    raise ValueError(
                        "Failed to receive response from http server: timeout"
                    )
                continue
        logger.info(f"Done. Player {player_id} is ready!")

        self.proc = proc

        if proc.stdout is None:
            raise ValueError("Failed to launch container: stdout is None")
        if proc.stderr is None:
            raise ValueError("Failed to launch container: stderr is None")

    def delete_container(self):
        if self.container_name is None:
            return

        command = [
            "docker",
            "rm",
            "-f",
            self.container_name,
        ]
        proc = subprocess.Popen(
            command,
            bufsize=0,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        proc.wait()
        self.proc = None

    def react(self, events: str) -> str:
        if self.proc is None:
            raise ValueError("Container is not running (3)")

        try:
            input_data = events.encode("utf8")
            logger.debug(f"{self.player_id} <- {input_data}")

            timeout_per_action = self.timeout
            if json.loads(events)[0]["type"] in ["start_game", "start_kyoku"]:
                # Workaround: To avoid timeouts
                # Some bots time out when loading models with start_game.
                timeout_per_action = 100.0

            resp = requests.post(
                f"http://localhost:{self.port_num}",
                data=input_data,
                timeout=timeout_per_action,
            )
            assert resp.status_code == 200
            outs = resp.content.decode("utf8")
            logger.debug(f"{self.player_id} -> {outs}")

            if (
                json.loads(events)[-1]["type"] == "tsumo"
                and json.loads(events)[-1]["actor"] == self.player_id
            ):
                json_data = {}
                try:
                    json_data = json.loads(outs)
                except Exception:
                    raise RuntimeError(f"JSON parser error: {outs}")

                if json_data["type"] == "none":
                    raise RuntimeError(f"invalid response: {str(outs)}")

        except requests.Timeout:
            raise TimeoutExpired(self.player_id)
        except Exception as e:
            raise EngineRuntimeError(f"RuntimeError: {str(e)}", self.player_id)

        return outs
