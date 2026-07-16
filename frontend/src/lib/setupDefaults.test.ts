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
    proxy: { enabled: true, addr: '127.0.0.1:23410', ca_dir: '' },
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
    },
    autoplay: {
      enabled: false,
      majsoul: {
        pre_click_delay_min_ms: 0,
        pre_click_delay_max_ms: 0,
        inter_click_delay_ms: 0,
        hover_delay_ms: 0,
        click_hold_ms: 0,
        dealer_first_discard_extra_delay_ms: 0,
      },
    },
    overlay: { enabled: true, top_n: 3, opacity: 1, always_on_top: true },
    autostart: {
      category: 'Ranked',
      player_count: 'Four',
      round_length: 'South',
      tier: 'Gold',
      target_game_count: 0,
      count_only_our_seat: true,
      use_vision: true,
      home_ncc_threshold: 0.65,
      settle_delay_ms: 8000,
      inter_click_delay_ms: 700,
      confirm_interval_ms: 2500,
      inter_game_delay_ms: 3000,
      matchmaking_timeout_ms: 180000,
      max_attempts: 12,
      auto_calibrate_home: true,
      rank_rules: [],
    },
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
