// Hook WebSocket to capture game connection and expose send function
(function() {
    if (window.__akagi_hooked) return;
    window.__akagi_hooked = true;
    window.__akagi_ws = null;
    window.__akagi_ws_messages = [];

    const OrigWebSocket = window.WebSocket;
    window.WebSocket = function(url, protocols) {
        const ws = protocols
            ? new OrigWebSocket(url, protocols)
            : new OrigWebSocket(url);

        // Capture the game gateway WebSocket
        if (url && (url.includes('gateway') || url.includes('game'))) {
            window.__akagi_ws = ws;
            console.log('[Akagi] WebSocket captured:', url);

            ws.addEventListener('message', function(event) {
                window.__akagi_ws_messages.push({
                    direction: 'recv',
                    data: event.data,
                    timestamp: Date.now()
                });
            });
        }

        return ws;
    };
    // Preserve prototype chain
    window.WebSocket.prototype = OrigWebSocket.prototype;
    window.WebSocket.CONNECTING = OrigWebSocket.CONNECTING;
    window.WebSocket.OPEN = OrigWebSocket.OPEN;
    window.WebSocket.CLOSING = OrigWebSocket.CLOSING;
    window.WebSocket.CLOSED = OrigWebSocket.CLOSED;

    // Send raw bytes through the game WebSocket
    window.__akagi_send_raw = function(data) {
        if (!window.__akagi_ws || window.__akagi_ws.readyState !== WebSocket.OPEN) {
            console.error('[Akagi] WebSocket not connected');
            return false;
        }
        window.__akagi_ws.send(data);
        return true;
    };

    // Check if WebSocket is connected
    window.__akagi_ws_ready = function() {
        return window.__akagi_ws !== null
            && window.__akagi_ws.readyState === WebSocket.OPEN;
    };

    console.log('[Akagi] WebSocket hook installed');
})();
