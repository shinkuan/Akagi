import { useState } from 'react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { invoke } from '@/lib/tauri'
import { useCaptureStore } from '@/stores/captureStore'

// Dev tool for calibrating Majsoul LOBBY coordinates for the upcoming
// auto-start feature. Majsoul is a single Laya <canvas> with no queryable
// DOM, so lobby buttons must be addressed by hand-measured 16:9-normalised
// coordinates (same convention as autoplay/majsoul/coords.rs). This card
// drives two backend commands: an on-page overlay that reports the coord
// under the cursor + records Alt+clicks, and a test-click to verify a coord.
//
// Strings are inline (not i18n) on purpose — this is a developer/calibration
// surface, not an end-user feature yet.
export function LobbyCalibrationCard() {
  const captureState = useCaptureStore((s) => s.status.state)
  const [on, setOn] = useState(false)
  const [status, setStatus] = useState('')
  const [x, setX] = useState('8.0')
  const [y, setY] = useState('4.5')
  const [scrollDelta, setScrollDelta] = useState('300')
  const [scrollRepeat, setScrollRepeat] = useState('20')
  const [dragYFrom, setDragYFrom] = useState('7.0')
  const [dragYTo, setDragYTo] = useState('3.0')
  const [dragRepeat, setDragRepeat] = useState('5')
  const [probe, setProbe] = useState('')
  const [busy, setBusy] = useState(false)

  const toggle = async (v: boolean) => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_calibration_overlay', { on: v })
      setOn(v)
      setStatus(v ? `Overlay on (${s})` : 'Overlay off')
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
      setOn(false)
    } finally {
      setBusy(false)
    }
  }

  const testClick = async () => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_test_click', {
        xNorm: Number(x),
        yNorm: Number(y),
      })
      setStatus(s)
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const testScroll = async () => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_test_scroll', {
        xNorm: Number(x),
        yNorm: Number(y),
        deltaY: Number(scrollDelta),
        repeat: Math.trunc(Number(scrollRepeat)),
      })
      setStatus(s)
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  // Vertical drag-scroll (from the x column above): drag from y-from up to
  // y-to, repeat N times. y-from > y-to scrolls the list toward the bottom.
  const testDrag = async () => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_test_drag', {
        x1Norm: Number(x),
        y1Norm: Number(dragYFrom),
        x2Norm: Number(x),
        y2Norm: Number(dragYTo),
        steps: 20,
        holdMs: 60,
        repeat: Math.trunc(Number(dragRepeat)),
      })
      setStatus(s)
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const probeStateNow = async () => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_probe_state')
      setProbe(s)
      setStatus('Probed page state')
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const screenshot = async () => {
    setBusy(true)
    try {
      const p = await invoke<string>('lobby_screenshot')
      setStatus(`Screenshot saved: ${p}`)
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const calibrateHome = async () => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_calibrate_home')
      setStatus(s)
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const checkHome = async () => {
    setBusy(true)
    try {
      const s = await invoke<string>('lobby_check_home')
      setStatus(s)
    } catch (e) {
      setStatus(`Failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Lobby coordinate calibration (dev tool)</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        {captureState !== 'running' && (
          <p className="text-xs text-amber-500">
            Start the Chromium capture backend and open the Majsoul page first
            (current capture state: {captureState})
          </p>
        )}

        <div className="flex items-center justify-between">
          <Label>Calibration overlay</Label>
          <Switch checked={on} disabled={busy} onCheckedChange={toggle} />
        </div>
        <p className="text-xs text-muted-foreground whitespace-pre-line">
          {
            'Once on, hover a Majsoul lobby button and the top-left corner shows that point’s live 16:9-normalised coords.\n' +
            'Alt+click records a point (auto-copied to the clipboard, ready to paste into lobby_coords.rs).\n' +
            'Alt+Shift+click clears the recorded points.'
          }
        </p>

        <div className="grid gap-1.5">
          <Label>Test click (verify a coord hits the button)</Label>
          <div className="flex items-center gap-2">
            <Input
              type="number"
              step="0.001"
              value={x}
              onChange={(e) => setX(e.target.value)}
              placeholder="x_norm 0–16"
            />
            <Input
              type="number"
              step="0.001"
              value={y}
              onChange={(e) => setY(e.target.value)}
              placeholder="y_norm 0–9"
            />
            <Button variant="outline" onClick={testClick} disabled={busy}>
              Click
            </Button>
          </div>
        </div>

        <div className="grid gap-1.5">
          <Label>Test scroll (wheel at the x/y above; can scroll to the bottom)</Label>
          <div className="flex items-center gap-2">
            <Input
              type="number"
              step="50"
              value={scrollDelta}
              onChange={(e) => setScrollDelta(e.target.value)}
              placeholder="delta_y (positive = scroll down)"
            />
            <Input
              type="number"
              step="1"
              min="1"
              value={scrollRepeat}
              onChange={(e) => setScrollRepeat(e.target.value)}
              placeholder="repeat (max 200)"
            />
            <Button variant="outline" onClick={testScroll} disabled={busy}>
              Scroll
            </Button>
          </div>
        </div>

        <div className="grid gap-1.5">
          <Label>Test drag-scroll (uses the x column above, y-from→y-to, y-from&gt;y-to = scroll down)</Label>
          <div className="flex items-center gap-2">
            <Input
              type="number"
              step="0.001"
              value={dragYFrom}
              onChange={(e) => setDragYFrom(e.target.value)}
              placeholder="y from"
            />
            <Input
              type="number"
              step="0.001"
              value={dragYTo}
              onChange={(e) => setDragYTo(e.target.value)}
              placeholder="y to"
            />
            <Input
              type="number"
              step="1"
              min="1"
              value={dragRepeat}
              onChange={(e) => setDragRepeat(e.target.value)}
              placeholder="repeat"
            />
            <Button variant="outline" onClick={testDrag} disabled={busy}>
              Drag
            </Button>
          </div>
        </div>

        <div className="grid gap-1.5">
          <Label>Probe current page state (find JS signals that distinguish scenes)</Label>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={probeStateNow} disabled={busy}>
              Probe page state
            </Button>
            <Button variant="outline" onClick={screenshot} disabled={busy}>
              Screenshot
            </Button>
            <span className="text-xs text-muted-foreground">
              Screenshot is saved to the temp dir; the path shows in the status below
            </span>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={calibrateHome} disabled={busy}>
              Calibrate lobby reference
            </Button>
            <Button variant="outline" onClick={checkHome} disabled={busy}>
              Check if at lobby
            </Button>
            <span className="text-xs text-muted-foreground">
              At the lobby home, click “Calibrate” to store a reference, then click “Check” on other screens to see the distance
            </span>
          </div>
          {probe && (
            <pre className="max-h-64 overflow-auto rounded-md bg-muted/40 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
              {probe}
            </pre>
          )}
        </div>

        {status && (
          <p className="text-xs font-mono text-muted-foreground break-all">{status}</p>
        )}
      </CardContent>
    </Card>
  )
}
