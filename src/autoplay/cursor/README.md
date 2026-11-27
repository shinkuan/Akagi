# `src/autoplay/cursor/` — the Mahjong Soul autoplay cursor

The one place Mahjong Soul autoplay sends mouse events from. Spawned by the
autoplay manager; idles unless Majsoul autoplay is allowed
(`mouse::gate`) and a kyoku is being played.

| File | Role |
|---|---|
| `mod.rs` | The actor. A tick every `STEP_MS` advances idle movement; a `click` command pre-empts it: approach stroke, hover, press, hold (optional 1 px jiggle), release, and for a double-click user's hand-tile press a second press after the profile's gap. Tracks where the cursor is, so every movement starts where the last ended. |
| `path.rs` | Stroke shapes: minimum-jerk progress along a slight bow, sub-pixel noise, clamped inside the canvas, the last point exactly on target. Approaches sometimes overshoot a little and correct. |
| `idle.rs` | Between presses: strokes alternate with rests sized so the moving share of time is the profile's (`own_active` while our decision is open, `other_active` otherwise). Our turn favours the hand; other turns mix the hand, the table centre and small drifts. |
| `control.rs` | Each kyoku draws a target moving-frame rate from the profile's log-normal; every 5 s the kyoku so far (page moving frames × `frame_ratio`) is compared with it and the activity gain nudged, bounded 0.3–3. |

## Timing

The manager asks the cursor how long the approach onto the next press will
take (`approach_ms`, the profile's range scaled by distance) and takes that
out of the think-time sleep before it, so a decision still lasts what the
delay model chose.

## Rules

- Never move while the button is down beyond the 1 px jiggle: the client
  starts a drag at 20 px.
- Never hold the page or rect read guard across an await on the cursor; the
  manager's `press` exists for that reason.
