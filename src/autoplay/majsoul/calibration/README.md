# `src/autoplay/majsoul/calibration/` — the user's mouse habits

Mahjong Soul's client sends `Lobby.logReport` at the end of every kyoku,
on the lobby socket. Besides the kyoku's indices, start time and device
info, it carries:

| Field | Meaning |
|---|---|
| `d` | Frames this kyoku in which the cursor position differed from the previous frame (the client's per-frame hand-input loop; zeroed at each new kyoku). A frame count, not a distance. |
| `m` | The click-mode setting: 0 single-click discard, 1 double-click. |
| `t` / `g` | Shortest / longest gap between the two presses of a double-click discard this kyoku, ms. Double-click mode only. |
| `e` / `f` | Half the screen height / width. |

A cursor that only moves when it clicks shows a `d` near the number of
clicks; a person shows thousands. This module measures the user; the
cursor (`../cursor/`) reproduces what it measured.

## Pieces

| File | Role |
|---|---|
| `mod.rs` | `MouseFeed` (in `AutoplayContext::majsoul_mouse`): the input and report channels, the session, the status channel for the UI. `majsoul_gate`: autoplay may act only when calibrated and not calibrating — enforced by the manager and the cursor, mirrored by the UI. `start` / `cancel` / `finish`, which the IPC commands call. |
| `recorder.js` / `recorder.rs` | Installed on Majsoul pages by the chromium backend, in an isolated world (`Page.addScriptToEvaluateOnNewDocument` with `worldName`, `Runtime.addBinding` with `executionContextName`), so the page's own scripts see no global and no listener. Samples the pointer once per animation frame, as the client does, plus every press and release, and sends a batch every 500 ms. It sees our own synthetic events too, which is how the cursor's output is counted. |
| `report.rs` | `LogReportSample`: the fields above, decoded by the Majsoul bridge (`BridgeHooks.mouse_reports`). Device id and UA are not kept. |
| `session.rs` | `Session`: the kyoku in progress (page frames and moving frames, for the cursor's controller), the client's last `m`, the smoothed frame ratio, and while calibrating, the games and their raw input. Persisted under `<config dir>/mouse_calibration/`: `state.json` (games) and `raw/*.jsonl` (one file per game, one line per kyoku with its report). |
| `fit.rs` | Raw kyoku → `MouseProfile` (see `src/config/README.md`). A field with too few samples keeps its previous value and is reported back. |
| `service.rs` | The long-lived task feeding the session from `MjaiBus`, the page batches, the reports and our decision window (`time_budget`, polled at 20 Hz). Spawned at startup whatever `autoplay.enabled` says, because calibrating is the user playing with autoplay off. |

## What counts

- **A game** counts once it ends with a final result and its first kyoku
  seen was East 1, 0 honba (a reconnect into the same game continues it).
  South games count 1, East-only games 0.5, other modes 0. Two are
  required to finish, around five is plenty.
- **A kyoku** is a sample when its report arrived and it lasted at least
  15 s; kyoku of games that do not count are still samples.
- **Phase**: a frame belongs to "our decision" while the bridge holds an
  open window for our seat in `time_budget`, otherwise to "other turns".

## Units

`d` is in client frames; the recorder counts browser animation frames. The
client caps itself below the display rate in its slow mode, so the ratio
between the two (`frame_ratio`) is measured from each kyoku (`d` over the
page's moving frames) rather than assumed, and the cursor's controller
targets client frames through it.

## Changing it

- New per-kyoku measurement: derive it in `fit.rs` from `RawKyoku`; if it
  needs data the raw line lacks, add the field (`#[serde(default)]`, so
  older files still load) and fill it in `Session::on_input` /
  `on_report`.
- New raw page field: extend `recorder.js`'s batch and `recorder::Wire`
  together; `parse_batch` drops anything it cannot read.
- The client may change this report in a hot update. If `d`'s meaning
  moves, the fit and the cursor's controller are what need revisiting.
