import { create } from 'zustand'
import type { MouseCalibrationStatus } from '@/types'

type MouseCalibrationStore = {
  status: MouseCalibrationStatus | null
  /** The "calibrate first" dialog is open. */
  promptOpen: boolean
  setStatus: (s: MouseCalibrationStatus) => void
  openPrompt: () => void
  closePrompt: () => void
}

export const useMouseCalibrationStore = create<MouseCalibrationStore>((set) => ({
  status: null,
  promptOpen: false,
  setStatus: (status) => set({ status }),
  openPrompt: () => set({ promptOpen: true }),
  closePrompt: () => set({ promptOpen: false }),
}))
