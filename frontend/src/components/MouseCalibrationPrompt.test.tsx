import { act } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render } from '@testing-library/react'

import { MouseCalibrationPrompt } from './MouseCalibrationPrompt'
import { useConfigStore } from '@/stores/configStore'
import { useMouseCalibrationStore } from '@/stores/mouseCalibrationStore'
import { DEFAULT_MOUSE_PROFILE } from '@/lib/mouseCalibration'
import type { AppConfig, MouseCalibrationStatus } from '@/types'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn(), HAS_TAURI: false }))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function config(kind: string, enabled: boolean): AppConfig {
  return {
    platform: { kind },
    capture: { mode: 'chromium' },
    autoplay: { enabled, majsoul: { mouse: DEFAULT_MOUSE_PROFILE } },
  } as unknown as AppConfig
}

const STATUS: MouseCalibrationStatus = {
  active: false,
  started_at: '',
  games: 0,
  kyoku: 0,
  required_games: 2,
  recommended_games: 5,
  recording: false,
}

const promptOpen = () => useMouseCalibrationStore.getState().promptOpen
const setConfig = (c: AppConfig) => act(() => useConfigStore.getState().setConfig(c))

describe('MouseCalibrationPrompt', () => {
  beforeEach(() => {
    useMouseCalibrationStore.setState({ status: STATUS, promptOpen: false })
  })

  it('opens at startup when autoplay is already on for Mahjong Soul', () => {
    useConfigStore.getState().setConfig(config('Majsoul', true))
    render(<MouseCalibrationPrompt />)
    expect(promptOpen()).toBe(true)
  })

  // Regression: the check ran once at startup, so switching to Mahjong Soul
  // afterwards never asked.
  it('opens when the saved settings switch to Mahjong Soul with autoplay on', () => {
    useConfigStore.getState().setConfig(config('Tenhou', true))
    render(<MouseCalibrationPrompt />)
    expect(promptOpen()).toBe(false)
    setConfig(config('Majsoul', true))
    expect(promptOpen()).toBe(true)
  })

  it('does not reopen after dismissal until the state is left and re-entered', () => {
    useConfigStore.getState().setConfig(config('Majsoul', true))
    render(<MouseCalibrationPrompt />)
    act(() => useMouseCalibrationStore.getState().closePrompt())
    setConfig({ ...config('Majsoul', true) })
    expect(promptOpen()).toBe(false)
    setConfig(config('Majsoul', false))
    setConfig(config('Majsoul', true))
    expect(promptOpen()).toBe(true)
  })
})
