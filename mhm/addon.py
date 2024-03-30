from collections.abc import Sequence
from dataclasses import dataclass
from typing import Literal

from mitmproxy import ctx, http, websocket
from mitmproxy.addons import view

from . import console
from .config import config
from .protocol import GameMessage, GameMessageType, compose, parse

_LOGIN_INFO_MESSAGES = [
    (GameMessageType.Response, ".lq.Lobby.login"),
    (GameMessageType.Response, ".lq.Lobby.emailLogin"),
    (GameMessageType.Response, ".lq.Lobby.oauth2Login"),
]

_MATCH_INFO_MESSAGES = [
    (GameMessageType.Request, ".lq.FastTest.authGame"),
]

ChannelType = Literal["LoBBY", "MaTCH"]
# O: ORIGINAL | INJECTED, TM: TO MODIFY, M: MODIFIED, D: DROPPED
MessageStatus = Literal["O", "TM", "M", "D"]
# HACK: Define Message status's colors
StatusColor: dict[MessageStatus, str] = {
    "O": "grey85",
    "TM": "red3",  # SECURITY: this indicates that the msg was not successfully modified
    "M": "magenta3",  # SECURITY: Whether the modification can be viewed by the server?
    "D": "orange3",  # SECURITY: Dropping will result in non-sequential request msg idx
}


def inject(flow: http.HTTPFlow, content: bytes):
    ctx.master.commands.call("inject.websocket", flow, True, content, False)


def broadcast(
    flows: Sequence[http.HTTPFlow],
    content: bytes,
    channel: ChannelType,
    members: list[int],
) -> None:
    for f in flows:
        if f.live and f.marked in members:
            if f.metadata.get(ChannelType) == channel:
                inject(f, content)


# NOTE: previous `log` method
def output(status: MessageStatus, tag: int | str, message: GameMessage):
    output = [
        f"[i][{StatusColor[status]}]{status}[/{StatusColor[status]}]",
        f"[grey50]{tag}[/grey50]",
        f"[cyan2]{message.kind.name}[/cyan2]",
        f"[gold1]{message.name}[/gold1]",
        f"[cyan3]{message.idx}[/cyan3]",
    ]

    console.log(" ".join(output))

    if config.base.debug:  # HACK
        console.log(f"-->> {message.data}")


class GameAddon(view.View):
    def __init__(self, methods) -> None:
        super().__init__()
        # HACK: `methods` refers to the
        # `run(mp: `MessageProcessor`)` method of `Hook` instances.
        self.methods = methods

    def websocket_start(self, flow: http.HTTPFlow):
        console.log(" ".join(["[i][green]Connected", flow.id[:13]]))

    def websocket_end(self, flow: http.HTTPFlow):
        console.log(" ".join(["[i][blue]Disconnected", flow.id[:13]]))

    def websocket_message(self, flow: http.HTTPFlow):
        # NOTE: make type checker happy
        assert flow.websocket is not None

        try:
            # NOTE: Flows are no longer saved into a dictionary
            wss_msg = flow.websocket.messages[-1]
            gam_msg = parse(flow.id, wss_msg.content)
            msg_key = (gam_msg.kind, gam_msg.name)
        except Exception:
            console.log(f"[i][red]Unsupported Message @ {flow.id[:13]}")
            return

        # HACK: Temporarily mark the LoBBY message
        if msg_key in _LOGIN_INFO_MESSAGES:
            channel: ChannelType = "LoBBY"
            account_id = gam_msg.data.get("account_id")
            flow.marked = account_id
            flow.metadata[ChannelType] = channel
        # HACK: Temporarily mark the MaTCH message
        elif msg_key in _MATCH_INFO_MESSAGES:
            channel: ChannelType = "MaTCH"
            account_id = gam_msg.data.get("account_id")
            flow.marked = account_id
            flow.metadata[ChannelType] = channel

        mu = MessageProcessor(flow=flow, wss_msg=wss_msg, gam_msg=gam_msg)

        # NOTE: Messages are only modified once the account_id is determined
        if not mu.member:
            output(mu.status, flow.id[:13], gam_msg)
            return

        try:
            for m in self.methods:
                m(mu)
            mu.apply()
        except Exception:
            console.print_exception()
            mu.drop()  # NOTE: Discard the message if fails
        finally:
            output(mu.status, mu.member, gam_msg)


@dataclass
class MessageProcessor:
    flow: http.HTTPFlow

    wss_msg: websocket.WebSocketMessage

    gam_msg: GameMessage

    # TODO: It would be best to display the status of the injected message
    status: MessageStatus = "O"

    @property
    def data(self) -> dict:
        return self.gam_msg.data

    @data.setter
    def data(self, value: dict):
        self.data = value

    @property
    def name(self) -> str:
        return self.gam_msg.name

    @property
    def kind(self) -> GameMessageType:
        return self.gam_msg.kind

    @property
    def key(self) -> tuple[GameMessageType, str]:
        return self.kind, self.name

    @property  # NOTE: the alias of account_id
    def member(self) -> int:
        return self.flow.marked

    def amend(self):
        # NOTE: After calling `amend`, method `apply` should be called.
        self.status = "TM"

    def drop(self):
        if self.status != "D":
            self.wss_msg.drop()
            self.status = "D"

    def apply(self):
        # NOTE: It's best to `compose(message)` after all hooks are completed
        if self.status == "TM":
            self.wss_msg.content = compose(self.gam_msg)
            self.status = "M"

    def request(self, data: dict, id: int):
        # SECURITY: Currently uncertain about the security of injecting into the server
        # TODO: It can provide the foundation for automation implementation
        raise NotImplementedError

    def response(self, data: dict | None = None):
        if not data:
            data = {}

        # NOTE: Discard the request sent to the server
        self.drop()

        response = GameMessage(
            data=data,
            idx=self.gam_msg.idx,
            name=self.gam_msg.name,
            kind=GameMessageType.Response,
        )

        inject(self.flow, compose(response))

    def notify(self, name: str, data: dict):
        notify = GameMessage(
            idx=0,
            name=name,
            data=data,
            kind=GameMessageType.Notify,
        )

        inject(self.flow, compose(notify))

    def broadcast(
        self,
        name: str,
        data: dict,
        channel: ChannelType,
        members: list[int],
    ):
        notify = GameMessage(
            idx=0,
            name=name,
            data=data,
            kind=GameMessageType.Notify,
        )

        broadcast(
            ctx.master.commands.call("view.flows.resolve", "@marked"),
            compose(notify),
            channel,
            members,
        )  # HACK: Marked flow includes non-live flows
