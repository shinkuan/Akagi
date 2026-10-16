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
  // Frames are held while Akagi is not reachable yet: a game socket usually
  // opens before ours does, and its first frames are the ones naming our
  // seat. Only recent ones, though, and only from sockets still open —
  // replaying a game that already ended would feed Akagi a match that is
  // not happening.
  var HOLD_MS = 60000;
  var HOLD_MAX_FRAMES = 2000;
  var HOLD_MAX_CHARS = 8 * 1024 * 1024;
  var RETRY_MIN_MS = 1000;
  var RETRY_MAX_MS = 30000;
  // The close code Akagi turns a page away with (wrong platform, outdated
  // script). Retrying cannot change that answer; reloading the page can.
  var CLOSE_POLICY = 1008;
  var OPEN = 1;
  var LOG_PREFIX = '[Akagi] ';
  // Chrome asks before a public site may reach this computer's own
  // addresses, and a "Block" there fails the connection exactly like Akagi
  // not running. The permission has had different names across Chrome
  // versions, so each is asked about.
  var LOCAL_NETWORK_PERMISSIONS = ['loopback-network', 'local-network-access', 'local-network'];

  var NativeWebSocket = window.WebSocket;
  if (!NativeWebSocket || window.__akagiCapture) return;
  Object.defineProperty(window, '__akagiCapture', { value: true });

  // Native members, captured before the page gets a chance to override
  // them: the wrapper runs inside a page subclass's `super()` call, before
  // that subclass's own fields exist, so it must not call anything the
  // subclass may have replaced.
  var addListener = EventTarget.prototype.addEventListener;
  var urlDescriptor = Object.getOwnPropertyDescriptor(NativeWebSocket.prototype, 'url');
  var readUrl = urlDescriptor && urlDescriptor.get;
  var nativeSend = NativeWebSocket.prototype.send;

  var flows = new WeakMap();
  var liveFlows = {};
  var nextFlow = 1;

  function now() {
    return window.performance ? window.performance.now() : Date.now();
  }

  // Status lines for the browser console: the one place a user can see why
  // nothing reaches Akagi. One line per state change, never per frame.
  function log(level, msg) {
    try {
      var c = window.console;
      if (c && typeof c[level] === 'function') c[level](LOG_PREFIX + msg);
    } catch (e) {}
  }

  var warnedUnreachable = false;
  var reportedBlocked = false;

  function reportUnreachable() {
    if (warnedUnreachable) return;
    warnedUnreachable = true;
    log(
      'warn',
      'cannot reach Akagi at ' + ENDPOINT + '. Is Akagi running with userscript capture? Retrying.'
    );
    var permissions = window.navigator && window.navigator.permissions;
    if (!permissions || typeof permissions.query !== 'function') return;
    LOCAL_NETWORK_PERMISSIONS.forEach(function (name) {
      try {
        permissions.query({ name: name }).then(
          function (status) {
            if (status.state !== 'denied' || reportedBlocked) return;
            reportedBlocked = true;
            log(
              'error',
              'this site is not allowed to access devices on your local network, so it cannot ' +
                'reach Akagi on this computer. Allow local network access in the site settings ' +
                '(the icon left of the address bar), then reload the page.'
            );
          },
          function () {}
        );
      } catch (e) {}
    });
  }

  // ---- connection to Akagi ------------------------------------------------

  var akagi = null;
  var akagiOpen = false;
  var held = [];
  var heldChars = 0;
  var retryMs = RETRY_MIN_MS;
  var retryTimer = null;
  // Akagi has stayed unreachable through the whole backoff, so it is
  // probably not running: stop encoding and holding frames until a
  // connection succeeds.
  var dormant = false;
  // Akagi turned this page away. Nothing more until the page is reloaded.
  var refused = false;

  function connected() {
    return akagiOpen && akagi !== null && akagi.readyState === OPEN;
  }

  function post(flow, msg) {
    if (refused) return;
    var text = JSON.stringify(msg);
    if (connected()) {
      try {
        akagi.send(text);
        return;
      } catch (e) {
        // Fall through and hold it; onclose reconnects.
      }
    }
    held.push({ flow: flow, at: now(), text: text });
    heldChars += text.length;
    prune();
    connect();
  }

  function prune() {
    var cutoff = now() - HOLD_MS;
    while (
      held.length > 0 &&
      (held.length > HOLD_MAX_FRAMES || heldChars > HOLD_MAX_CHARS || held[0].at < cutoff)
    ) {
      heldChars -= held.shift().text.length;
    }
  }

  function dropHeld(flow) {
    var kept = [];
    heldChars = 0;
    for (var i = 0; i < held.length; i++) {
      if (held[i].flow !== flow) {
        kept.push(held[i]);
        heldChars += held[i].text.length;
      }
    }
    held = kept;
  }

  function connect() {
    if (refused || akagi || retryTimer) return;
    var sock;
    try {
      // The page's constructor, not the wrapped one: our own socket must
      // not be forwarded to Akagi as if it were the game's.
      sock = new NativeWebSocket(ENDPOINT);
    } catch (e) {
      reportUnreachable();
      scheduleRetry();
      return;
    }
    akagi = sock;
    var opened = false;
    sock.onopen = function () {
      opened = true;
      akagiOpen = true;
      dormant = false;
      warnedUnreachable = false;
      retryMs = RETRY_MIN_MS;
      log('info', 'connected to Akagi');
      prune();
      var pending = held;
      held = [];
      heldChars = 0;
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
        // their flows. `open` is never held; this is where it is sent.
        for (var id in liveFlows) {
          sock.send(JSON.stringify({ t: 'open', flow: Number(id), url: liveFlows[id] }));
        }
        for (var i = 0; i < pending.length; i++) sock.send(pending[i].text);
      } catch (e) {
        // The socket died mid-flush; onclose reconnects.
      }
    };
    sock.onclose = function (ev) {
      akagiOpen = false;
      akagi = null;
      if (ev && ev.code === CLOSE_POLICY) {
        refused = true;
        held = [];
        heldChars = 0;
        log(
          'warn',
          'Akagi turned this page away (' + ((ev && ev.reason) || 'policy') + '). Fix it in ' +
            'Akagi (platform setting, or reinstall the userscript), then reload this page.'
        );
        return;
      }
      if (opened) log('warn', 'lost the connection to Akagi; retrying');
      else reportUnreachable();
      scheduleRetry();
    };
    sock.onerror = function () {
      // Always followed by close.
    };
  }

  function scheduleRetry() {
    if (refused || retryTimer) return;
    if (retryMs >= RETRY_MAX_MS && !dormant) {
      dormant = true;
      log('info', 'Akagi is still unreachable; pausing capture until it is back.');
    }
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

  // Copy mutable buffers now: they are encoded later, by which time the
  // page may have reused or transferred them. Strings and Blobs are
  // immutable.
  function snapshot(data) {
    if (data instanceof ArrayBuffer) return data.slice(0);
    if (ArrayBuffer.isView(data)) {
      return new Uint8Array(data.buffer, data.byteOffset, data.byteLength).slice();
    }
    return data;
  }

  // Resolves to `{kind, data}`, or null for data this cannot represent.
  function encode(data) {
    if (typeof data === 'string') return Promise.resolve({ kind: 'text', data: data });
    if (data instanceof ArrayBuffer) {
      return Promise.resolve({ kind: 'binary', data: base64(new Uint8Array(data)) });
    }
    if (data instanceof Uint8Array) {
      return Promise.resolve({ kind: 'binary', data: base64(data) });
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
        if (msg) post(state.flow, msg);
      })
      .catch(function () {});
  }

  function forward(state, dir, data) {
    if (refused || dormant) return;
    var copy = snapshot(data);
    enqueue(state, function () {
      return encode(copy).then(function (enc) {
        return enc && { t: 'frame', flow: state.flow, dir: dir, kind: enc.kind, data: enc.data };
      });
    });
  }

  function watch(ws, requestedUrl) {
    var flow = nextFlow++;
    var state = { flow: flow, chain: Promise.resolve() };
    addListener.call(ws, 'message', function (ev) {
      try {
        forward(state, 'down', ev.data);
      } catch (e) {}
    });
    addListener.call(ws, 'close', function () {
      try {
        delete liveFlows[flow];
        // After everything already queued for this socket: tell Akagi it
        // closed, or — if Akagi never heard of it — forget its held frames.
        enqueue(state, function () {
          if (connected()) return { t: 'close', flow: flow };
          dropHeld(flow);
          return null;
        });
      } catch (e) {}
    });
    var url = String((readUrl ? readUrl.call(ws) : ws.url) || requestedUrl);
    flows.set(ws, state);
    liveFlows[flow] = url;
    log('info', 'watching game socket ' + url);
    if (connected()) post(flow, { t: 'open', flow: flow, url: url });
    else connect();
  }

  NativeWebSocket.prototype.send = function (data) {
    // Send first: a frame the socket refused (it throws, as natively) never
    // left, so it must not reach Akagi either.
    var result = nativeSend.apply(this, arguments);
    try {
      var state = flows.get(this);
      if (state) forward(state, 'up', data);
    } catch (e) {}
    return result;
  };

  var WrappedWebSocket = new Proxy(NativeWebSocket, {
    construct: function (target, args, newTarget) {
      var ws = Reflect.construct(target, args, newTarget);
      try {
        watch(ws, args[0]);
      } catch (e) {}
      return ws;
    },
  });
  // Keep `ws.constructor === WebSocket` true, as it is without the script.
  try {
    NativeWebSocket.prototype.constructor = WrappedWebSocket;
  } catch (e) {}
  window.WebSocket = WrappedWebSocket;
  log('info', 'capture userscript active; reporting to ' + ENDPOINT);
})();
