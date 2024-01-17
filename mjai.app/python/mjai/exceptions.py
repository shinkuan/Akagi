class EngineRuntimeError(Exception):
    def __init__(self, msg: str, player_id: int) -> None:
        self.msg = msg
        self.player_id = player_id


class TimeoutExpired(Exception):
    def __init__(self, player_id: int) -> None:
        self.player_id = player_id
