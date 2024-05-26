import argparse
import asyncio

from . import console
from .common import start_inject, start_proxy
from .config import config
from .hook import Hook
from .resource import ResourceManager, load_resource


# TODO: Plugins should be independent of this project and should be loaded from a folder
def create_hooks(resger: ResourceManager) -> list[Hook]:
    hooks = []
    if config.base.aider:
        from .hook.aider import DerHook

        hooks.append(DerHook())
    if config.base.chest:
        from .hook.chest import EstHook

        hooks.append(EstHook(resger))
    if config.base.skins:
        from .hook.skins import KinHook

        hooks.append(KinHook(resger))
    return hooks


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    console.log("Load Resource")
    with console.status("[magenta]Fetch LQC.LQBIN"):
        resger = load_resource()
    console.log(f"LQBin Version: [cyan3]{resger.version}")
    console.log(f"> {len(resger.item_rows):0>3} items")
    console.log(f"> {len(resger.title_rows):0>3} titles")
    console.log(f"> {len(resger.character_rows):0>3} characters")

    console.log("Init Hooks")
    hooks = create_hooks(resger)
    for h in hooks:
        console.log(f"> [cyan3]{h.__class__.__name__}")

    async def start():
        tasks = set()
        if config.mitmdump.args:
            tasks.add(start_proxy([h.run for h in hooks], args.verbose))
            console.log(f"Start mitmdump @ {config.mitmdump.args.get('mode')}")
        if config.proxinject.path:
            tasks.add(start_inject())
            console.log(f"Start proxinject @ {config.proxinject.args.get('set-proxy')}")
        await asyncio.gather(*tasks)

    try:
        asyncio.run(start())
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
