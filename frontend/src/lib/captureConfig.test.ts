import { describe, expect, it } from 'vitest'
import {
  captureTitleKey,
  effectiveCaptureMode,
  installPageUrl,
  userscriptUrl,
  withCaptureMode,
  withChromiumPatch,
  withIngestPatch,
} from './captureConfig'
import type { CaptureConfig } from '@/types'

function capture(): CaptureConfig {
  return {
    mode: 'mitm',
    chromium: {
      executable: '',
      user_data_dir: '',
      start_url: 'https://game.maj-soul.com/1/',
      cft_channel: 'stable',
      force_cft: false,
      extra_args: [],
    },
    // Non-default on purpose, so a reset to defaults would show.
    http: { record_all: true, bodies: false, max_body_bytes: 1024, static_assets: true },
    ingest: { addr: '127.0.0.1:24000' },
  }
}

describe('capture config edits', () => {
  // Regression: Settings rebuilt `capture` as `{mode, chromium}`, dropping
  // `http`, which the backend then reset to defaults on save.
  it('switching mode keeps every other section', () => {
    const before = capture()
    const after = withCaptureMode(before, 'ingest')
    expect(after.mode).toBe('ingest')
    expect(after.http).toEqual(before.http)
    expect(after.ingest).toEqual(before.ingest)
    expect(after.chromium).toEqual(before.chromium)
  })

  it('editing chromium keeps http and ingest', () => {
    const before = capture()
    const after = withChromiumPatch(before, { force_cft: true })
    expect(after.chromium.force_cft).toBe(true)
    expect(after.chromium.start_url).toBe(before.chromium.start_url)
    expect(after.http).toEqual(before.http)
    expect(after.ingest).toEqual(before.ingest)
  })

  it('editing ingest keeps http and chromium', () => {
    const before = capture()
    const after = withIngestPatch(before, { addr: '127.0.0.1:23411' })
    expect(after.ingest.addr).toBe('127.0.0.1:23411')
    expect(after.http).toEqual(before.http)
    expect(after.chromium).toEqual(before.chromium)
  })

  it('does not mutate its input', () => {
    const before = capture()
    const snapshot = structuredClone(before)
    withCaptureMode(before, 'chromium')
    withChromiumPatch(before, { executable: '/x' })
    withIngestPatch(before, { addr: '127.0.0.1:1' })
    expect(before).toEqual(snapshot)
  })

  it('native-only platforms are always MITM', () => {
    expect(effectiveCaptureMode('ingest', false)).toBe('mitm')
    expect(effectiveCaptureMode('chromium', false)).toBe('mitm')
    expect(effectiveCaptureMode('ingest', true)).toBe('ingest')
  })

  it('names each capture backend', () => {
    expect(captureTitleKey('chromium')).toBe('overview.capture_chromium')
    expect(captureTitleKey('ingest')).toBe('ingest.overview_title')
    expect(captureTitleKey('mitm')).toBe('overview.capture_mitm')
    expect(captureTitleKey(null)).toBe('overview.capture_mitm')
  })

  it('points at the install page in the UI language', () => {
    expect(installPageUrl(' 127.0.0.1:23411 ', 'zh-TW')).toBe('http://127.0.0.1:23411/?lang=zh-TW')
  })

  it('points at the served userscript', () => {
    expect(userscriptUrl(' 127.0.0.1:23411 ')).toBe('http://127.0.0.1:23411/akagi.user.js')
  })
})
