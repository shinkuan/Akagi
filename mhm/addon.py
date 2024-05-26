from collections.abc import Sequence
from dataclasses import dataclass
from typing import Literal

from mitmproxy import ctx, http, websocket
from mitmproxy.addons import view

from . import console
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
# O: ORIGINAL, I: INJECTED, M: MODIFIED, D: DROPPED
# NOTE: The status `to modify` is unnecessary, as message are dropped directly if fails
MessageStatus = Literal["O", "I", "M", "D"]
# HACK: Define Message status's colors
StatusColor: dict[MessageStatus, str] = {
    "O": "grey85",
    "I": "cyan1",  # NOTE: This indicates that this message is injected
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


class GameAddon(view.View):
    def __init__(self, methods, verbose: bool) -> None:
        super().__init__()
        # HACK: `methods` refers to the
        # `run(mp: `MessageProcessor`)` method of `Hook` instances.
        self.methods = methods
        self.verbose = verbose

    def websocket_start(self, flow: http.HTTPFlow):
        console.log(f"[i][green]Connected {flow.id[:8]}")

    def websocket_end(self, flow: http.HTTPFlow):
        console.log(f"[i][blue]Disconnected {flow.id[:8]}")

    def websocket_message(self, flow: http.HTTPFlow):
        # NOTE: make type checker happy
        assert flow.websocket is not None

        try:
            # NOTE: Flows are no longer saved into a dictionary
            wss = flow.websocket.messages[-1]
            msg = parse(flow.id, wss.content)
            mp = MessageProcessor(flow=flow, wss=wss, msg=msg)
        except Exception as e:
            console.log(f"[i][orange1]! {flow.id[:8]:>9} {repr(e)}")
            return

        # HACK: Temporarily mark the LoBBY message
        if msg.key in _LOGIN_INFO_MESSAGES:
            channel: ChannelType = "LoBBY"
            account_id = msg.data.get("account_id")
            flow.marked = account_id
            flow.metadata[ChannelType] = channel
        # HACK: Temporarily mark the MaTCH message
        elif msg.key in _MATCH_INFO_MESSAGES:
            channel: ChannelType = "MaTCH"
            account_id = msg.data.get("account_id")
            flow.marked = account_id
            flow.metadata[ChannelType] = channel

        # NOTE: previous `log` method
        def summary(mp: MessageProcessor):
            msg = mp.msg
            snm = f"[{msg.data['name']}]" if msg.name == ".lq.ActionPrototype" else ""
            tag = mp.member
            idx = msg.idx if msg.idx else ""
            sts = mp.status
            console.log(
                f"[i][{StatusColor[sts]}]{sts}[/{StatusColor[sts]}]"
                f" [grey50]{tag:>9}[/grey50]"
                f" [cyan2]{msg.kind.name:<8}[/cyan2]"
                f" [gold1]{msg.name}[/gold1]"
                f" [cyan3]{idx}[/cyan3]"
                f"[gold3]{snm}[/gold3]"
            )
            if self.verbose:  # HACK
                console.log(f"-->> {msg.data}")

        # NOTE: Messages are only modified once the account_id is determined
        if not isinstance(mp.member, int):
            summary(mp)
            return

        try:
            [fn(mp) for fn in self.methods]
            mp.apply()
        except Exception:
            console.print_exception()
            mp.drop()  # NOTE: Discard the message if fails
        finally:
            summary(mp)


@dataclass
class MessageProcessor:
    flow: http.HTTPFlow

    wss: websocket.WebSocketMessage

    msg: GameMessage

    modified: bool = False

    @property
    def status(self) -> MessageStatus:
        if self.wss.dropped:
            return "D"
        elif self.wss.injected:
            return "I"
        elif self.modified:
            return "M"
        else:
            return "O"

    @property  # NOTE: the alias of account_id | the short id of flow
    def member(self) -> int | str:
        if self.flow.marked:
            return self.flow.marked
        else:
            return self.flow.id[:8]

    def amend(self):
        # NOTE: After calling `amend`, method `apply` should be called.
        self.modified = True

    def drop(self):
        # NOTE: Now the 'dropped' status is determined by the original websocket message
        self.wss.drop()

    def apply(self):
        # NOTE: It's best to `compose(message)` after all hooks are completed
        if self.modified:
            self.wss.content = compose(self.msg)

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
            idx=self.msg.idx,
            name=self.msg.name,
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
