//! Thin wrappers around chromiumoxide for the autoplay manager.
//!
//! Centralised so the click + canvas-rect query logic can be unit-mocked
//! and the manager keeps a single dependency on chromiumoxide types.

use crate::autoplay::context::CanvasRect;
use anyhow::{anyhow, Context, Result};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use chromiumoxide::layout::Point;
use chromiumoxide::page::Page;
use std::time::Duration;

/// Dispatch a single mouse click at `(x, y)` (CSS pixels) as four CDP
/// events, with mandatory hover before press:
///
/// 1. `mouseMoved` to `(x, y)`
/// 2. sleep `hover_delay_ms` (≥100ms — Laya's input system samples hover
///    state before mousedown registers a hit on a tile sprite)
/// 3. `mousePressed`
/// 4. sleep `click_hold_ms`
/// 5. `mouseReleased`
///
/// `chromiumoxide::Page::click` collapses 3+5 into back-to-back frames
/// without the hover delay, which Majsoul drops on the floor for hand
/// tiles. Hand-rolling the sequence is required.
pub async fn dispatch_click(
    page: &Page,
    x: f64,
    y: f64,
    hover_delay_ms: u32,
    click_hold_ms: u32,
) -> Result<()> {
    let pt = Point::new(x, y);
    page.move_mouse(pt).await.context("CDP move_mouse")?;
    if hover_delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(hover_delay_ms as u64)).await;
    }

    let press = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MousePressed)
        .x(pt.x)
        .y(pt.y)
        .button(MouseButton::Left)
        .click_count(1)
        .build()
        .map_err(|e| anyhow!("build mousePressed: {e}"))?;
    page.execute(press).await.context("CDP mousePressed")?;

    if click_hold_ms > 0 {
        tokio::time::sleep(Duration::from_millis(click_hold_ms as u64)).await;
    }

    let release = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseReleased)
        .x(pt.x)
        .y(pt.y)
        .button(MouseButton::Left)
        .click_count(1)
        .build()
        .map_err(|e| anyhow!("build mouseReleased: {e}"))?;
    page.execute(release).await.context("CDP mouseReleased")?;

    Ok(())
}

/// Dispatch a single mouse-wheel scroll tick at `(x, y)` (CSS pixels).
///
/// `delta_y > 0` scrolls the content downward (toward the end of a list);
/// `delta_y < 0` scrolls up. One CDP `Input.dispatchMouseEvent` of type
/// `mouseWheel`. To scroll a long list "to the bottom" issue several ticks
/// in a row (the canvas has no measurable scroll position, so callers just
/// repeat until far enough).
///
/// Majsoul's Laya lists usually accept wheel events; if a particular panel
/// ignores them, a press-move-release drag is the fallback (not yet wired).
pub async fn dispatch_scroll(page: &Page, x: f64, y: f64, delta_y: f64) -> Result<()> {
    let pt = Point::new(x, y);
    // Hover the wheel origin first — Laya samples pointer position before
    // routing the wheel to the element under the cursor.
    page.move_mouse(pt)
        .await
        .context("CDP move_mouse (scroll)")?;
    let wheel = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseWheel)
        .x(pt.x)
        .y(pt.y)
        .delta_x(0.0)
        .delta_y(delta_y)
        .build()
        .map_err(|e| anyhow!("build mouseWheel: {e}"))?;
    page.execute(wheel).await.context("CDP mouseWheel")?;
    Ok(())
}

/// Press at `(x1, y1)`, drag to `(x2, y2)` with the left button held (moving
/// through `steps` interpolated hops), then release. Models a touch/drag
/// scroll — Laya lists that ignore the synthesized wheel event usually
/// respond to this. `hold_ms` is a short dwell after press and before
/// release so the engine registers the gesture rather than a flick.
///
/// To scroll a list "to the bottom", drag from a lower point up to a higher
/// point and repeat several times (a canvas has no measurable scroll offset).
pub async fn dispatch_drag(
    page: &Page,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    steps: u32,
    hold_ms: u32,
) -> Result<()> {
    let steps = steps.max(1);
    page.move_mouse(Point::new(x1, y1))
        .await
        .context("CDP move_mouse (drag start)")?;
    let press = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MousePressed)
        .x(x1)
        .y(y1)
        .button(MouseButton::Left)
        .buttons(1_i64)
        .click_count(1)
        .build()
        .map_err(|e| anyhow!("build mousePressed (drag): {e}"))?;
    page.execute(press)
        .await
        .context("CDP mousePressed (drag)")?;
    if hold_ms > 0 {
        tokio::time::sleep(Duration::from_millis(hold_ms as u64)).await;
    }
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = x1 + (x2 - x1) * t;
        let y = y1 + (y2 - y1) * t;
        let mv = DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseMoved)
            .x(x)
            .y(y)
            .button(MouseButton::Left)
            .buttons(1_i64)
            .build()
            .map_err(|e| anyhow!("build mouseMoved (drag): {e}"))?;
        page.execute(mv).await.context("CDP mouseMoved (drag)")?;
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
    if hold_ms > 0 {
        tokio::time::sleep(Duration::from_millis(hold_ms as u64)).await;
    }
    let release = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseReleased)
        .x(x2)
        .y(y2)
        .button(MouseButton::Left)
        .buttons(0_i64)
        .click_count(1)
        .build()
        .map_err(|e| anyhow!("build mouseReleased (drag): {e}"))?;
    page.execute(release)
        .await
        .context("CDP mouseReleased (drag)")?;
    Ok(())
}

/// Read the game canvas's `getBoundingClientRect()` via `Runtime.evaluate`.
///
/// Majsoul renders into the first `<canvas>` element on the page; Tenhou
/// uses the same selector when running in browser mode. We grab the
/// first canvas indiscriminately — multi-canvas pages aren't a thing on
/// these platforms.
pub async fn evaluate_canvas_rect(page: &Page) -> Result<CanvasRect> {
    // IIFE so `Runtime.evaluate` returns a single value, not a Promise.
    // `is_likely_js_function` in chromiumoxide picks the right CDP call
    // based on whether the expression looks like a function — we wrap
    // in `(()=>{...})()` to ensure plain-expression evaluation.
    let expr = "(()=>{const c=document.getElementsByTagName('canvas')[0];\
                if(!c)return null;\
                const r=c.getBoundingClientRect();\
                return {x:r.x,y:r.y,width:r.width,height:r.height};})()";
    let result = page
        .evaluate(expr)
        .await
        .context("CDP evaluate canvas rect")?;
    let value = result
        .value()
        .ok_or_else(|| anyhow!("canvas rect: no value returned"))?;
    if value.is_null() {
        return Err(anyhow!("canvas rect: page has no <canvas> element"));
    }
    let rect: CanvasRect = serde_json::from_value(value.clone())
        .context("canvas rect: deserialise from page value")?;
    Ok(rect)
}
