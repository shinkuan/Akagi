import asyncio
import json
from threading import Thread
from aiohttp import web
from .logger import logger


class DataServer(Thread):
    def __init__(self, external_port=8765):
        super().__init__()
        self.daemon = True
        self.external_port = external_port
        self.clients: dict[str, dict] = {}  # {clientId: {"response": StreamResponse, "request": Request}}
        self.latest_data = None
        self.loop = None
        self.runner = None
        self.running = False
        self.keep_alive_task = None

    def _format_sse_message(self, data: dict) -> bytes:
        return f"data: {json.dumps(data)}\n\n".encode("utf-8")

    async def _remove_client(self, client_id: str, expected_response=None):
        client_data = self.clients.get(client_id)
        # If the stored response does not match the one we intend to close, skip removal to avoid
        # kicking out a fresh connection that reused the same client_id.
        if expected_response is not None and client_data is not None:
            if client_data.get("response") is not expected_response:
                return

        client_data = self.clients.pop(client_id, None)
        if not client_data:
            return

        response = client_data.get("response")
        try:
            if response:
                await response.write_eof()
        except ConnectionResetError:
            logger.debug(f"Client {client_id} already closed connection.")
        except Exception as exc:
            logger.warning(f"Error while closing connection for {client_id}: {exc}")

        logger.info(f"SSE client {client_id} disconnected.")

    async def keep_alive(self):
        while True:
            await asyncio.sleep(10)
            if not self.clients:
                continue

            logger.debug(f"Running keep-alive for {len(self.clients)} clients")
            zombie_client_ids = []
            zombie_responses = {}
            keepalive_payload = b": keep-alive\n\n"

            for client_id, client_data in list(self.clients.items()):
                response = client_data.get("response")
                request = client_data.get("request")

                if not request or not request.transport or request.transport.is_closing():
                    zombie_client_ids.append(client_id)
                    zombie_responses[client_id] = response
                    continue
                
                try:
                    await response.write(keepalive_payload)
                    await response.drain()
                except ConnectionResetError:
                    logger.warning(f"Connection reset for {client_id}, marking as zombie.")
                    zombie_client_ids.append(client_id)
                    zombie_responses[client_id] = response
                except Exception as exc:
                    logger.warning(f"Keep-alive failed for {client_id}: {exc}")
                    zombie_client_ids.append(client_id)
                    zombie_responses[client_id] = response

            for client_id in zombie_client_ids:
                await self._remove_client(client_id, expected_response=zombie_responses.get(client_id))
            
            logger.debug(f"Keep-alive check finished. {len(zombie_client_ids)} zombie clients removed.")

    def run(self):
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        
        try:
            app = web.Application()
            app.router.add_get('/', self.sse_handler)
            
            self.runner = web.AppRunner(app)
            self.loop.run_until_complete(self.runner.setup())
            
            site = web.TCPSite(self.runner, '0.0.0.0', self.external_port)
            self.loop.run_until_complete(site.start())
            
            logger.info(f"DataServer SSE listening on 0.0.0.0:{self.external_port}")
            self.running = True
            self.keep_alive_task = self.loop.create_task(self.keep_alive())
            self.loop.run_forever()
        finally:
            if self.keep_alive_task:
                self.keep_alive_task.cancel()
            if self.runner:
                self.loop.run_until_complete(self.runner.cleanup())
            logger.info("DataServer event loop stopped.")

    def stop(self):
        if self.running and self.loop and self.loop.is_running():
            logger.info("Stopping DataServer...")
            self.loop.call_soon_threadsafe(self.loop.stop)
            logger.info("DataServer stop signal sent.")
        self.running = False

    async def _send_payload(self, client_id: str, response: web.StreamResponse, payload: bytes) -> bool:
        try:
            await response.write(payload)
            await response.drain()
            return True
        except ConnectionResetError:
            logger.warning(f"SSE connection reset for {client_id}")
            return False
        except Exception as exc:
            logger.error(f"Failed to send data to {client_id}: {exc}")
            return False

    async def _update_data_async(self, data):
        self.latest_data = data
        if not self.clients:
            return

        payload = self._format_sse_message(data)
        logger.debug(f"Broadcasting to {len(self.clients)} clients: {json.dumps(data)}")

        zombie_client_ids = []
        zombie_responses = {}
        tasks = []
        client_order = []

        for client_id, client_data in list(self.clients.items()):
            request = client_data.get("request")
            response = client_data.get("response")
            if not request or not request.transport or request.transport.is_closing():
                zombie_client_ids.append(client_id)
                zombie_responses[client_id] = response
                continue

            client_order.append(client_id)
            tasks.append(self._send_payload(client_id, response, payload))

        if tasks:
            results = await asyncio.gather(*tasks, return_exceptions=True)
            for client_id, result in zip(client_order, results):
                if result is False or isinstance(result, Exception):
                    zombie_client_ids.append(client_id)
                    zombie_responses[client_id] = self.clients.get(client_id, {}).get("response")

        for client_id in zombie_client_ids:
            await self._remove_client(client_id, expected_response=zombie_responses.get(client_id))

    def update_data(self, data):
        if self.loop and self.running:
            asyncio.run_coroutine_threadsafe(self._update_data_async(data), self.loop)

    async def sse_handler(self, request):
        client_id = request.query.get('clientId')
        if not client_id:
            logger.warning("Client connected without clientId, rejecting.")
            return web.HTTPBadRequest(text="clientId is required")

        headers = {
            "Content-Type": "text/event-stream",
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "Access-Control-Allow-Origin": "*",
        }

        response = web.StreamResponse(status=200, headers=headers)
        await response.prepare(request)

        if client_id in self.clients:
            logger.warning(f"Client {client_id} already connected. Closing old connection.")
            await self._remove_client(client_id, expected_response=self.clients[client_id].get("response"))
        
        self.clients[client_id] = {'response': response, 'request': request}
        logger.info(f"SSE client {client_id} connected from {request.remote}")

        try:
            await response.write(b": connected\n\n")
            await response.drain()
        except Exception as exc:
            logger.warning(f"Failed to send initial SSE comment to {client_id}: {exc}")

        if self.latest_data:
            await self._send_payload(client_id, response, self._format_sse_message(self.latest_data))

        try:
            while True:
                await asyncio.sleep(1)
                transport = request.transport
                if not transport or transport.is_closing():
                    logger.info(f"SSE client {client_id} closed the connection.")
                    break
        except asyncio.CancelledError:
            logger.debug(f"SSE handler for {client_id} cancelled.")
        finally:
            await self._remove_client(client_id, expected_response=response)

        return response
