import { describe, expect, it } from 'vitest'
import type { AppConfig, MouseCalibrationStatus } from '@/types'
import {
  DEFAULT_MOUSE_PROFILE,
  autoplayBlock,
  canFinish,
  formatGames,
  hasEnough,
  shouldPromptAtStartup,
} from './mouseCalibration'

function config(kind: string, enabled: boolean, calibrated: boolean): AppConfig {
  return {
    platform: { kind },
    autoplay: {
      enabled,
      majsoul: { mouse: { ...DEFAULT_MOUSE_PROFILE, calibrated } },
    },
  } as unknown as AppConfig
}

function status(active: boolean, games = 0): MouseCalibrationStatus {
  return {
    active,
    started_at: '',
    games,
    kyoku: 0,
    required_games: 2,
    recommended_games: 5,
    recording: false,
    recent_autoplay_rate: null,
    recent_autoplay_kyoku: 0,
  }
}

describe('mouse calibration gating', () => {
  it('blocks Mahjong Soul autoplay until calibrated', () => {
    expect(autoplayBlock(config('Majsoul', false, false), status(false))).toBe('calibrate')
    expect(autoplayBlock(config('Majsoul', false, true), status(false))).toBeNull()
  })

  it('blocks it while a calibration records, even when calibrated', () => {
    expect(autoplayBlock(config('Majsoul', true, true), status(true))).toBe('calibrating')
  })

  it('leaves other platforms alone', () => {
    expect(autoplayBlock(config('Tenhou', true, false), status(true))).toBeNull()
  })

  it('asks at startup only when autoplay is already on without a calibration', () => {
    expect(shouldPromptAtStartup(config('Majsoul', true, false), status(false))).toBe(true)
    expect(shouldPromptAtStartup(config('Majsoul', false, false), status(false))).toBe(false)
    expect(shouldPromptAtStartup(config('Majsoul', true, false), status(true))).toBe(false)
    expect(shouldPromptAtStartup(config('Majsoul', true, true), status(false))).toBe(false)
  })
})

describe('mouse calibration progress', () => {
  it('finishes from the required games and calls the recommended enough', () => {
    expect(canFinish(status(true, 1.5))).toBe(false)
    expect(canFinish(status(true, 2))).toBe(true)
    expect(canFinish(status(false, 3))).toBe(false)
    expect(hasEnough(status(true, 4.5))).toBe(false)
    expect(hasEnough(status(true, 5))).toBe(true)
  })

  it('prints half games', () => {
    expect(formatGames(2)).toBe('2')
    expect(formatGames(1.5)).toBe('1.5')
  })
})
