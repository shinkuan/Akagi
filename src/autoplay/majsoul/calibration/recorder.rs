//! Installs `recorder.js` in an isolated world of the game page and decodes
//! the batches it sends back through a CDP binding.
//!
//! Both the script and the binding are scoped to the world named [`WORLD`],
//! so the page's main world sees neither: no global, no listener.

use anyhow::{anyhow, Context, Result};
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use chromiumoxide::cdp::js_protocol::runtime::AddBindingParams;
use chromiumoxide::page::Page;
use serde::Deserialize;

pub const WORLD: &str = "akagi_input";
pub const BINDING: &str = "__akagi_input";
const SCRIPT: &str = include_str!("recorder.js");
/// A 500 ms batch at a few hundred Hz is a few KB; anything this large is not ours.
const MAX_PAYLOAD: usize = 512 * 1024;

pub fn script() -> String {
    SCRIPT.replace("__AKAGI_BINDING__", BINDING)
}

/// Install on `page`, for the document already loaded and every later one.
pub async fn install(page: &Page) -> Result<()> {
    let binding = AddBindingParams::builder()
        .name(BINDING)
        .execution_context_name(WORLD)
        .build()
        .map_err(|e| anyhow!("build addBinding: {e}"))?;
    page.execute(binding).await.context("Runtime.addBinding")?;
    let script = AddScriptToEvaluateOnNewDocumentParams::builder()
        .source(script())
        .world_name(WORLD)
        .run_immediately(true)
        .build()
        .map_err(|e| anyhow!("build addScriptToEvaluateOnNewDocument: {e}"))?;
    page.execute(script)
        .await
        .context("Page.addScriptToEvaluateOnNewDocument")?;
    Ok(())
}

/// A pointer press or release, canvas-normalised (0..1 on each axis).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Button {
    pub t_ms: f64,
    pub down: bool,
    pub button: u8,
    pub x: f64,
    pub y: f64,
}

/// One frame in which the pointer had moved since the previous frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    pub t_ms: f64,
    pub x: f64,
    pub y: f64,
}

/// Half a second of page input; every time is epoch ms.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputBatch {
    /// Start of every animation frame.
    pub frames: Vec<f64>,
    pub moves: Vec<Move>,
    pub buttons: Vec<Button>,
    /// Canvas size, CSS px.
    pub rect: (f64, f64),
}

#[derive(Deserialize)]
struct Wire {
    v: u32,
    t0: f64,
    ft: Vec<f64>,
    mv: Vec<[f64; 3]>,
    bt: Vec<[f64; 5]>,
    rw: f64,
    rh: f64,
}

/// Decode one binding payload. Anything malformed is `None`.
pub fn parse_batch(payload: &str) -> Option<InputBatch> {
    if payload.len() > MAX_PAYLOAD {
        return None;
    }
    let w: Wire = serde_json::from_str(payload).ok()?;
    if w.v != 1 || !w.t0.is_finite() {
        return None;
    }
    let ok = |v: f64| v.is_finite();
    let frames = w.ft.iter().filter(|t| ok(**t)).map(|t| w.t0 + t).collect();
    let moves =
        w.mv.iter()
            .filter(|m| m.iter().all(|v| ok(*v)))
            .map(|m| Move {
                t_ms: w.t0 + m[0],
                x: m[1],
                y: m[2],
            })
            .collect();
    let buttons =
        w.bt.iter()
            .filter(|b| b.iter().all(|v| ok(*v)))
            .map(|b| Button {
                t_ms: w.t0 + b[0],
                down: b[1] == 0.0,
                button: b[2].clamp(0.0, 8.0) as u8,
                x: b[3],
                y: b[4],
            })
            .collect();
    let rect = if ok(w.rw) && ok(w.rh) {
        (w.rw, w.rh)
    } else {
        (0.0, 0.0)
    };
    Some(InputBatch {
        frames,
        moves,
        buttons,
        rect,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_carries_the_binding_name() {
        let s = script();
        assert!(s.contains(BINDING));
        assert!(!s.contains("__AKAGI_BINDING__"));
    }

    #[test]
    fn batch_times_become_absolute() {
        let b = parse_batch(
            r#"{"v":1,"t0":1000.5,"ft":[0,16.7],"mv":[[16.7,0.25,0.5]],"bt":[[10,0,0,0.2,0.4],[90,1,0,0.2,0.4]],"rw":1280,"rh":720}"#,
        )
        .unwrap();
        assert_eq!(b.frames, vec![1000.5, 1017.2]);
        assert_eq!(b.moves.len(), 1);
        assert!((b.moves[0].t_ms - 1017.2).abs() < 1e-9);
        assert!(b.buttons[0].down && !b.buttons[1].down);
        assert_eq!(b.rect, (1280.0, 720.0));
    }

    #[test]
    fn foreign_or_oversized_payloads_are_dropped() {
        assert!(parse_batch("not json").is_none());
        assert!(parse_batch(r#"{"v":2,"t0":0,"ft":[],"mv":[],"bt":[],"rw":0,"rh":0}"#).is_none());
        let big = format!(
            r#"{{"v":1,"t0":0,"ft":[{}],"mv":[],"bt":[],"rw":0,"rh":0}}"#,
            "0,".repeat(MAX_PAYLOAD)
        );
        assert!(parse_batch(&big).is_none());
    }
}
