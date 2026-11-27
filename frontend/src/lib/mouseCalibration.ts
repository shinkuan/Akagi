import type { AppConfig, MouseCalibrationStatus, MouseProfile } from '@/types'

/** Mirror of `MouseProfile::default()` on the Rust side. */
export const DEFAULT_MOUSE_PROFILE: MouseProfile = {
  calibrated: false,
  calibrated_at: '',
  sample_games: 0,
  sample_kyoku: 0,
  move_rate: { median: 15, low: 8, high: 30 },
  own_active: 0.45,
  other_active: 0.2,
  speed: [0.4, 1.5],
  approach_ms: [180, 450],
  click_mode: 'single',
  double_click_ms: [120, 260],
  frame_ratio: 1,
}

export function mouseProfile(config: AppConfig | null | undefined): MouseProfile {
  return config?.autoplay?.majsoul?.mouse ?? DEFAULT_MOUSE_PROFILE
}

/** Why Mahjong Soul autoplay cannot be switched on right now, if it can't. */
export type AutoplayBlock = 'calibrate' | 'calibrating'

export function autoplayBlock(
  config: AppConfig | null | undefined,
  status: MouseCalibrationStatus | null,
): AutoplayBlock | null {
  if (config?.platform.kind !== 'Majsoul') return null
  if (status?.active) return 'calibrating'
  if (!mouseProfile(config).calibrated) return 'calibrate'
  return null
}

/** At startup: autoplay is already on for Mahjong Soul but was never
 *  calibrated (a config from before calibration existed). */
export function shouldPromptAtStartup(
  config: AppConfig | null | undefined,
  status: MouseCalibrationStatus | null,
): boolean {
  return !!config?.autoplay?.enabled && autoplayBlock(config, status) === 'calibrate'
}

export function canFinish(status: MouseCalibrationStatus | null): boolean {
  return !!status?.active && status.games >= status.required_games
}

export function hasEnough(status: MouseCalibrationStatus | null): boolean {
  return !!status && status.games >= status.recommended_games
}

/** `1.5` → `"1.5"`, `2` → `"2"`. */
export function formatGames(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1)
}
