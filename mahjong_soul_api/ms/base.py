import asyncio
import websockets

from ms.protocol_pb2 import Wrapper


class MSRPCChannel:

    def __init__(self, endpoint):
        self._endpoint = endpoint
        self._req_events = {}
        self._new_req_idx = 1
        self._res = {}
        self._hooks = {}

        self._ws = None
        self._msg_dispatcher = None

    def add_hook(self, msg_type, hook):
        if msg_type not in self._hooks:
            self._hooks[msg_type] = []
        self._hooks[msg_type].append(hook)

    def unwrap(self, wrapped):
        wrapper = Wrapper()
        wrapper.ParseFromString(wrapped)
        return wrapper

    def wrap(self, name, data):
        wrapper = Wrapper()
        wrapper.name = name
        wrapper.data = data
        return wrapper.SerializeToString()

    async def connect(self, ms_host):
        self._ws = await websockets.connect(self._endpoint, origin=ms_host)
        self._msg_dispatcher = asyncio.create_task(self.dispatch_msg())

    async def close(self):
        self._msg_dispatcher.cancel()
        try:
            await self._msg_dispatcher
        except asyncio.CancelledError:
            pass
        finally:
            await self._ws.close()

    async def dispatch_msg(self):
        while True:
            msg = await self._ws.recv()
            type_byte = msg[0]
            if type_byte == 1:  # NOTIFY
                wrapper = self.unwrap(msg[1:])
                for hook in self._hooks.get(wrapper.name, []):
                    asyncio.create_task(hook(wrapper.data))
            elif type_byte == 2:  # REQUEST
                wrapper = self.unwrap(msg[3:])
                for hook in self._hooks.get(wrapper.name, []):
                    asyncio.create_task(hook(wrapper.data))
            elif type_byte == 3:  # RESPONSE
                idx = int.from_bytes(msg[1:3], 'little')
                if not idx in self._req_events:
                    continue
                self._res[idx] = msg
                self._req_events[idx].set()

    async def send_request(self, name, msg):
        idx = self._new_req_idx
        self._new_req_idx = (self._new_req_idx + 1) % 60007

        wrapped = self.wrap(name, msg)
        pkt = b'\x02' + idx.to_bytes(2, 'little') + wrapped

        evt = asyncio.Event()
        self._req_events[idx] = evt

        await self._ws.send(pkt)
        await evt.wait()

        if not idx in self._res:
            return None
        res = self._res[idx]
        del self._res[idx]

        if idx in self._req_events:
            del self._req_events[idx]

        body = self.unwrap(res[3:])

        return body.data


class MSRPCService:

    def __init__(self, channel):
        self._channel = channel

    def get_package_name(self):
        raise NotImplementedError

    def get_service_name(self):
        raise NotImplementedError

    def get_req_class(self, method):
        raise NotImplementedError

    def get_res_class(self, method):
        raise NotImplementedError

    async def call_method(self, method, req):
        msg = req.SerializeToString()
        name = '.{}.{}.{}'.format(self.get_package_name(), self.get_service_name(), method)
        res_msg = await self._channel.send_request(name, msg)
        res_class = self.get_res_class(method)
        res = res_class()
        res.ParseFromString(res_msg)
        return res
