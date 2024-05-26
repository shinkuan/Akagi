import asyncio

from mitmproxy.options import Options
from mitmproxy.tools.dump import DumpMaster

from .addon import GameAddon
from .config import config


def _cmd(dikt):
    # HACK: not support single parameter
    return [obj for key, value in dikt.items() for obj in (f"--{key}", value)]


async def start_proxy(methods: list, verbose: bool):
    master = DumpMaster(Options(**config.mitmdump.args), **config.mitmdump.dump)
    master.addons.add(GameAddon(methods, verbose))
    await master.run()
    return master


async def start_inject():
    cmd = [config.proxinject.path, *_cmd(config.proxinject.args)]
    while True:
        # HACK: Due to Proxinject exiting directly without injecting into the process
        process = await asyncio.subprocess.create_subprocess_exec(
            *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )
        await process.communicate()
        await asyncio.sleep(0.8)
