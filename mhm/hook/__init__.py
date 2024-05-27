from mhm import conf
from mhm.proto import MsgManager, MsgType


class Hook:
    def __init__(self) -> None:
        self.mapHook = {}

    def hook(self, mger: MsgManager):
        mKey = (mger.m.type, mger.m.method)
        if mKey in self.mapHook:
            self.mapHook[mKey](mger)

    def bind(self, mType: MsgType, mMethod: str):
        def decorator(func):
            mKey = (mType, mMethod)
            self.mapHook[mKey] = func
            return func

        return decorator


hooks: list[Hook] = []

if conf.hook.enable_aider:
    from .aider import DerHook

    hooks.append(DerHook())

if conf.hook.enable_chest:
    from .chest import OstHook

    hooks.append(OstHook())

if conf.hook.enable_skins:
    from .skins import KinHook

    hooks.append(KinHook())
