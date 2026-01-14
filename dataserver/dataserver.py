import asyncio
import json
from contextlib import suppress
from threading import Thread

from aiohttp import web

from .logger import logger


class DataServer(Thread):
    """
    SSE DataServer for broadcasting game state to web clients.

    Protocol:
    - GET /?clientId=xxx  - Connect as SSE client
    - Server sends: "data: {...}\n\n" for game updates
    - Server sends: ": keep-alive\n\n" every 10s
    - Server sends: "data: {\"type\": \"shutdown\"}\n\n" on shutdown
    """
    def __init__(self, host: str, port: int):
        super().__init__(daemon=True)
        self.host = host
        self.port = port

        # Client registry: {client_id: {"response": StreamResponse, "request": Request}}
        self.clients: dict[str, dict] = {}
        self.latest_data = None

        # Event loop state
        self.loop: asyncio.AbstractEventLoop | None = None
        self.runner: web.AppRunner | None = None
        self.shutdown_event: asyncio.Event | None = None
        self.running = False

    def run(self):
        """Thread entry point - runs the async event loop."""
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        try:
            self.loop.run_until_complete(self._serve())
        except Exception as e:
            logger.error(f"DataServer error: {e}")
        finally:
            if self.loop.is_running():
                self.loop.stop()
            logger.info("DataServer stopped")

    def stop(self):
        """Signal the server to shutdown."""
        if self.running and self.loop and self.loop.is_running():
            self.loop.call_soon_threadsafe(lambda: self.shutdown_event and self.shutdown_event.set())
        self.running = False

    def update(self, data: dict):
        """Push data to all connected clients (thread-safe)."""
        if self.loop and self.running and self.shutdown_event and not self.shutdown_event.is_set():
            asyncio.run_coroutine_threadsafe(self._broadcast(data), self.loop)

    # ==================== Async internals ====================

    async def _serve(self):
        """Main server coroutine."""
        self.shutdown_event = asyncio.Event()

        app = web.Application()
        app.router.add_get("/", self._handle_sse)

        self.runner = web.AppRunner(app)
        await self.runner.setup()
        site = web.TCPSite(self.runner, self.host, self.port)
        await site.start()

        logger.info(f"DataServer listening on {self.host}:{self.port}")
        self.running = True

        # Start keepalive task
        keepalive = asyncio.create_task(self._keepalive())

        try:
            await self.shutdown_event.wait()
        finally:
            keepalive.cancel()
            with suppress(asyncio.CancelledError):
                await keepalive
            await self._shutdown()
            if self.runner:
                await self.runner.cleanup()
            self.running = False

    async def _handle_sse(self, request: web.Request) -> web.StreamResponse:
        """Handle SSE connection from client."""
        client_id = request.query.get("clientId")
        if not client_id:
            return web.HTTPBadRequest(text="clientId required")

        response = web.StreamResponse(
            status=200,
            headers={
                "Content-Type": "text/event-stream",
                "Cache-Control": "no-cache",
                "Connection": "keep-alive",
                "Access-Control-Allow-Origin": "*",
            },
        )
        await response.prepare(request)

        # Disconnect existing client with same ID
        if client_id in self.clients:
            logger.warning(f"Client {client_id} reconnecting, closing old connection")
            self.clients.pop(client_id, None)

        self.clients[client_id] = {"response": response, "request": request}
        logger.info(f"Client {client_id} connected from {request.remote}")

        # Send connection confirmation and latest data
        await self._send(client_id, b": connected\n\n")
        if self.latest_data:
            await self._send(client_id, self._encode(self.latest_data))

        # Wait for shutdown signal
        try:
            await self.shutdown_event.wait()
        except asyncio.CancelledError:
            pass
        finally:
            self.clients.pop(client_id, None)
            logger.info(f"Client {client_id} disconnected")

        return response

    async def _broadcast(self, data: dict):
        """Broadcast data to all connected clients."""
        if self.shutdown_event and self.shutdown_event.is_set():
            return
        self.latest_data = data

        if not self.clients:
            return

        payload = self._encode(data)

        # Send to all clients in parallel
        client_ids = list(self.clients.keys())
        results = await asyncio.gather(
            *[self._send(cid, payload) for cid in client_ids],
            return_exceptions=True,
        )

        # Remove failed clients
        for client_id, ok in zip(client_ids, results):
            if not ok or isinstance(ok, Exception):
                self.clients.pop(client_id, None)

    async def _keepalive(self):
        """Send keepalive comments to detect dead connections."""
        while not self.shutdown_event.is_set():
            try:
                await asyncio.wait_for(self.shutdown_event.wait(), timeout=10)
                break
            except asyncio.TimeoutError:
                pass

            if not self.clients:
                continue

            dead = []
            for client_id, client in list(self.clients.items()):
                req = client.get("request")
                if not req or not req.transport or req.transport.is_closing():
                    dead.append(client_id)
                    continue
                if not await self._send(client_id, b": keep-alive\n\n"):
                    dead.append(client_id)

            for client_id in dead:
                self.clients.pop(client_id, None)
                logger.debug(f"Removed dead client {client_id}")

    async def _shutdown(self):
        """Notify clients and close connections on shutdown."""
        if not self.clients:
            return
        payload = self._encode({"type": "shutdown"})
        for client_id in list(self.clients):
            await self._send(client_id, payload)
        self.clients.clear()

    async def _send(self, client_id: str, payload: bytes) -> bool:
        """Send payload to a specific client. Returns False if failed."""
        client = self.clients.get(client_id)
        if not client:
            return False
        response = client.get("response")
        if not response:
            return False
        try:
            await response.write(payload)
            await response.drain()
            return True
        except Exception:
            return False

    def _encode(self, data: dict) -> bytes:
        """Encode dict as SSE data frame."""
        return f"data: {json.dumps(data)}\n\n".encode("utf-8")
