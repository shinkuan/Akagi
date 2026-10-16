import { describe, expect, it } from 'vitest'

import { withFirstRunCaptureDefault } from './setupDefaults'
import type { AppConfig, CaptureMode, PlatformKind } from '@/types'

function makeConfig(over: {
  firstRunCompleted?: boolean
  platform?: PlatformKind
  mode?: CaptureMode
}): AppConfig {
  return {
    general: {
      first_run_completed: over.firstRunCompleted ?? false,
      developer_mode: false,
    },
    logging: { dir: '', level: 'info', all_level: 'warn' },
    platform: { kind: over.platform ?? 'Majsoul' },
    proxy: { enabled: true, addr: '127.0.0.1:23410', ca_dir: '', block_telemetry: true },
    bot: {
      enabled: true,
      active_4p: 'akagi-native',
      active_3p: 'akagi-native3p',
      auto_sync: false,
      dir: '',
      api: {
        enabled: false,
        base_url: '',
        key: '',
        model_4p: '',
        model_3p: '',
        proxy_enabled: false,
        proxy: '',
        react_timeout_ms: 3000,
      },
    },
    capture: {
      mode: over.mode ?? 'mitm',
      chromium: {
        executable: '',
        user_data_dir: '',
        start_url: 'https://game.maj-soul.com/1/',
        cft_channel: 'stable',
        force_cft: false,
        extra_args: [],
      },
      http: { record_all: false, bodies: true, max_body_bytes: 262144, static_assets: false },
      ingest: { addr: '127.0.0.1:23411' },
    },
    autoplay: {
      enabled: false,
      majsoul: {
        pre_click_delay_min_ms: 0,
        pre_click_delay_max_ms: 0,
        inter_click_delay_ms: 0,
        hover_delay_ms: 0,
        click_hold_ms: 0,
        verify_input_ms: 0,
        click_retries: 0,
        reload_after_failures: 0,
        dealer_first_discard_extra_delay_ms: 0,
      },
      delay: {
        mode: 'legacy',
        min_delay_ms: 0,
        min_button_delay_ms: 0,
        distribution: 'uniform',
        lognormal: {},
        bank_on_long_thought: false,
        riichi_extra_ms: 0,
        kan_extra_ms: 0,
        safety_margin_ms: 0,
        bank_use_fraction: 0,
        bank_max_single_ms: 0,
        no_budget_cap_ms: 0,
      },
    },
    overlay: { enabled: true, top_n: 3, opacity: 1, always_on_top: true },
    network: { github_mirror_mode: 'auto', github_custom_mirror: '' },
  }
}

describe('withFirstRunCaptureDefault', () => {
  it('pre-selects Chromium on a first run for a Chromium-capable platform', () => {
    const out = withFirstRunCaptureDefault(makeConfig({ platform: 'Majsoul', mode: 'mitm' }))
    expect(out.capture.mode).toBe('chromium')
  })

  it('keeps the saved mode when setup was already completed (Settings re-run)', () => {
    // A returning user who saved MITM must not be flipped back to Chromium.
    const out = withFirstRunCaptureDefault(
      makeConfig({ firstRunCompleted: true, mode: 'mitm' }),
    )
    expect(out.capture.mode).toBe('mitm')
  })

  it('leaves native-only platforms on MITM even on a first run', () => {
    // Riichi City has no web build → Chromium capture cannot drive it.
    const out = withFirstRunCaptureDefault(
      makeConfig({ platform: 'RiichiCity', mode: 'mitm' }),
    )
    expect(out.capture.mode).toBe('mitm')
  })

  it('never overrides a mode that is already set (not the untouched default)', () => {
    const out = withFirstRunCaptureDefault(makeConfig({ mode: 'chromium' }))
    expect(out.capture.mode).toBe('chromium')
  })

  it('is idempotent and does not mutate its input', () => {
    const input = makeConfig({ platform: 'Majsoul', mode: 'mitm' })
    const once = withFirstRunCaptureDefault(input)
    const twice = withFirstRunCaptureDefault(once)
    expect(twice).toEqual(once)
    // Original untouched — the seed helpers rely on a pure return.
    expect(input.capture.mode).toBe('mitm')
  })
})
