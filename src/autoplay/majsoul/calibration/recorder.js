// Runs in an isolated world: the page's own scripts share the DOM with it but
// none of its globals or listeners. Samples the pointer once per animation
// frame, as the client does, and hands a batch to the binding every 500 ms.
(() => {
  const BINDING = '__AKAGI_BINDING__';
  if (globalThis.__akagiInputInstalled) return;
  globalThis.__akagiInputInstalled = true;

  const origin = performance.timeOrigin;
  const opts = { capture: true, passive: true };
  let px = NaN;
  let py = NaN;
  let lx = NaN;
  let ly = NaN;
  let rect = null;
  let rectAt = -Infinity;
  let t0 = 0;
  let ft = [];
  let mv = [];
  let bt = [];

  const canvasRect = (now) => {
    if (now - rectAt > 1000) {
      const c = document.querySelector('canvas');
      rect = c ? c.getBoundingClientRect() : null;
      rectAt = now;
    }
    return rect && rect.width > 0 && rect.height > 0 ? rect : null;
  };
  const r1 = (v) => Math.round(v * 10) / 10;
  const r4 = (v) => Math.round(v * 10000) / 10000;
  const norm = (x, y, now) => {
    const r = canvasRect(now);
    return r ? [r4((x - r.left) / r.width), r4((y - r.top) / r.height)] : null;
  };

  addEventListener(
    'pointermove',
    (e) => {
      px = e.clientX;
      py = e.clientY;
    },
    opts,
  );
  const onButton = (kind) => (e) => {
    const now = performance.now();
    const p = norm(e.clientX, e.clientY, now);
    if (!p) return;
    if (!t0) t0 = now;
    bt.push([r1(now - t0), kind, e.button, p[0], p[1]]);
  };
  addEventListener('pointerdown', onButton(0), opts);
  addEventListener('pointerup', onButton(1), opts);

  const frame = (ts) => {
    if (!t0) t0 = ts;
    ft.push(r1(ts - t0));
    if (!Number.isNaN(px) && (px !== lx || py !== ly)) {
      if (!Number.isNaN(lx)) {
        const p = norm(px, py, ts);
        if (p) mv.push([r1(ts - t0), p[0], p[1]]);
      }
      lx = px;
      ly = py;
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  setInterval(() => {
    if (!ft.length && !bt.length) return;
    const r = canvasRect(performance.now());
    const batch = {
      v: 1,
      t0: origin + t0,
      ft,
      mv,
      bt,
      rw: r ? r.width : 0,
      rh: r ? r.height : 0,
    };
    ft = [];
    mv = [];
    bt = [];
    t0 = 0;
    try {
      globalThis[BINDING](JSON.stringify(batch));
    } catch (_) {
      // Binding not installed in this world yet; the batch is dropped.
    }
  }, 500);
})();
