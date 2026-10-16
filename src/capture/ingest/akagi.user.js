// ==UserScript==
// @name         Akagi capture
// @namespace    https://github.com/shinkuan/Akagi
// @version      __AKAGI_VERSION__
// @description  Sends this game page's WebSocket frames to Akagi running on this computer. Read-only: the game's own traffic is never changed.
// @match        __AKAGI_MATCH__
// @run-at       document-start
// @grant        none
// @inject-into  page
// @noframes
// ==/UserScript==

// Served by Akagi's ingest capture backend, which fills in the endpoint
// and the list of game sites. See `src/capture/ingest/` in the Akagi
// repository for the wire protocol this speaks.
//
// The one rule: the game must behave exactly as if this script were not
// installed. Every frame is passed to the game untouched and unblocked,
// forwarding is fire-and-forget, and nothing this script does may throw
// into the page. Losing a frame on the way to Akagi is acceptable; changing
// one on the way to the game is not.

(function () {
  'use strict';

  var ENDPOINT = '__AKAGI_ENDPOINT__';
  var PROTOCOL_VERSION = 1;
  var AGENT = 'userscript/__AKAGI_VERSION__';
  // Frames held while Akagi is not reachable yet. A game socket usually
  // opens before ours does, so its first frames (the ones naming our seat)
  // must wait instead of being lost. Oldest are dropped past the cap.
  var MAX_QUEUE = 2000;
  var RETRY_MIN_MS = 1000;
  var RETRY_MAX_MS = 30000;

  var NativeWebSocket = window.WebSocket;
  if (!NativeWebSocket || window.__akagiCapture) return;
  Object.defineProperty(window, '__akagiCapture', { value: true });

  var flows = new WeakMap();
  var liveFlows = {};
  var nextFlow = 1;

  // ---- connection to Akagi ------------------------------------------------

  var akagi = null;
  var akagiOpen = false;
  var queue = [];
  var retryMs = RETRY_MIN_MS;
  var retryTimer = null;

  function post(msg) {
    var text = JSON.stringify(msg);
    if (akagiOpen) {
      try {
        akagi.send(text);
        return;
      } catch (e) {
        akagiOpen = false;
      }
    }
    queue.push(text);
    if (queue.length > MAX_QUEUE) queue.shift();
    connect();
  }

  function connect() {
    if (akagi || retryTimer) return;
    var sock;
    try {
      // The page's constructor, not the wrapped one: our own socket must
      // not be forwarded to Akagi as if it were the game's.
      sock = new NativeWebSocket(ENDPOINT);
    } catch (e) {
      scheduleRetry();
      return;
    }
    akagi = sock;
    sock.onopen = function () {
      akagiOpen = true;
      retryMs = RETRY_MIN_MS;
      var held = queue;
      queue = [];
      try {
        sock.send(
          JSON.stringify({
            t: 'hello',
            v: PROTOCOL_VERSION,
            agent: AGENT,
            origin: window.location.origin,
            url: window.location.origin + window.location.pathname,
          })
        );
        // Announce every live game socket — the ones opened while Akagi was
        // unreachable, and after a reconnect all of them — so Akagi can name
        // their flows. `open` is never queued; this is where it is sent.
        for (var id in liveFlows) {
          sock.send(JSON.stringify({ t: 'open', flow: Number(id), url: liveFlows[id] }));
        }
        for (var i = 0; i < held.length; i++) sock.send(held[i]);
      } catch (e) {
        // The socket died mid-flush; onclose reconnects.
      }
    };
    sock.onclose = function () {
      akagiOpen = false;
      akagi = null;
      scheduleRetry();
    };
    sock.onerror = function () {
      // Always followed by close.
    };
  }

  function scheduleRetry() {
    if (retryTimer) return;
    retryTimer = window.setTimeout(function () {
      retryTimer = null;
      connect();
    }, retryMs);
    retryMs = Math.min(retryMs * 2, RETRY_MAX_MS);
  }

  // ---- frame encoding -----------------------------------------------------

  function base64(bytes) {
    var parts = [];
    for (var i = 0; i < bytes.length; i += 0x8000) {
      parts.push(String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000)));
    }
    return window.btoa(parts.join(''));
  }

  function readBlob(blob) {
    if (typeof blob.arrayBuffer === 'function') return blob.arrayBuffer();
    return new Promise(function (resolve, reject) {
      var reader = new window.FileReader();
      reader.onload = function () {
        resolve(reader.result);
      };
      reader.onerror = function () {
        reject(reader.error);
      };
      reader.readAsArrayBuffer(blob);
    });
  }

  // Resolves to `{kind, data}`, or null for data this cannot represent.
  function encode(data) {
    if (typeof data === 'string') return Promise.resolve({ kind: 'text', data: data });
    if (data instanceof ArrayBuffer) {
      return Promise.resolve({ kind: 'binary', data: base64(new Uint8Array(data)) });
    }
    if (ArrayBuffer.isView(data)) {
      var view = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      return Promise.resolve({ kind: 'binary', data: base64(view) });
    }
    if (window.Blob && data instanceof window.Blob) {
      return readBlob(data).then(function (buf) {
        return { kind: 'binary', data: base64(new Uint8Array(buf)) };
      });
    }
    return Promise.resolve(null);
  }

  // ---- watching the game's sockets ----------------------------------------

  // Each socket's messages go out in the order they happened, even though a
  // Blob can only be read asynchronously: everything for one socket runs
  // through one promise chain. Mahjong Soul pairs responses with requests,
  // so a reply overtaking its request would be misread.
  function enqueue(state, make) {
    state.chain = state.chain
      .then(make)
      .then(function (msg) {
        if (msg) post(msg);
      })
      .catch(function () {});
  }

  function forward(state, dir, data) {
    enqueue(state, function () {
      return encode(data).then(function (enc) {
        return enc && { t: 'frame', flow: state.flow, dir: dir, kind: enc.kind, data: enc.data };
      });
    });
  }

  function watch(ws, requestedUrl) {
    var flow = nextFlow++;
    var url = String(ws.url || requestedUrl);
    var state = { flow: flow, chain: Promise.resolve() };
    flows.set(ws, state);
    liveFlows[flow] = url;
    if (akagiOpen) post({ t: 'open', flow: flow, url: url });
    else connect();
    ws.addEventListener('message', function (ev) {
      try {
        forward(state, 'down', ev.data);
      } catch (e) {}
    });
    ws.addEventListener('close', function () {
      try {
        delete liveFlows[flow];
        enqueue(state, function () {
          return { t: 'close', flow: flow };
        });
      } catch (e) {}
    });
  }

  var nativeSend = NativeWebSocket.prototype.send;
  NativeWebSocket.prototype.send = function (data) {
    try {
      var state = flows.get(this);
      if (state) forward(state, 'up', data);
    } catch (e) {}
    return nativeSend.apply(this, arguments);
  };

  window.WebSocket = new Proxy(NativeWebSocket, {
    construct: function (target, args, newTarget) {
      var ws = Reflect.construct(target, args, newTarget);
      try {
        watch(ws, args[0]);
      } catch (e) {}
      return ws;
    },
  });
})();
