import asyncio

from . import pRoot, logger, conf, resver, init


PROXINJECTOR = pRoot / "common/proxinject/proxinjector-cli"


def _cmd(dict):
    return [obj for key, value in dict.items() for obj in (f"--{key}", value)]


async def start_proxy():
    from mitmproxy.tools.dump import DumpMaster
    from mitmproxy.options import Options
    from .addons import addons

    master = DumpMaster(Options(**conf.mitmdump), **conf.dump)
    master.addons.add(*addons)
    await master.run()
    return master


async def start_inject():
    cmd = [PROXINJECTOR, *_cmd(conf.proxinject)]

    while True:
        process = await asyncio.subprocess.create_subprocess_exec(
            *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )

        stdout, stderr = await process.communicate()

        await asyncio.sleep(0.8)


def main():
    async def start():
        logger.info(f"[i]log level: {conf.mhm.log_level}")
        logger.info(f"[i]pure python protobuf: {conf.mhm.pure_python_protobuf}")

        logger.info(f"[i]version: {resver.version}")
        logger.info(f"[i]characters: {len(resver.emotes)}")

        tasks = set()

        if conf.mitmdump:
            tasks.add(start_proxy())
            logger.info(f"[i]mitmdump launched @ {len(conf.mitmdump.get('mode'))} mode")

        # if conf.proxinject:
        #     tasks.add(start_inject())
        #     logger.info(f"[i]proxinject launched @ {conf.proxinject.get('set-proxy')}")

        await asyncio.gather(*tasks)

    init()

    try:
        asyncio.run(start())
    except KeyboardInterrupt:
        pass
