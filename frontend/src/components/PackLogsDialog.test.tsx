import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'

import { PackLogsDialog } from './PackLogsDialog'
import type { LogSessionInfo } from '@/types'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

const invoke = vi.fn()
vi.mock('@/lib/tauri', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
  HAS_TAURI: false,
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: () => Promise.resolve('/tmp/akagi-logs.zip'),
}))

vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

const session = (name: string, patch: Partial<LogSessionInfo> = {}): LogSessionInfo => ({
  name,
  path: `/logs/${name}`,
  size_bytes: 0,
  mtime_ms: 0,
  is_active: false,
  ...patch,
})

const SESSIONS = [
  session('20260102-000000', { is_active: true }),
  session('20250101-000000', { legacy_layout: true }),
]

function renderDialog() {
  const onOpenChange = vi.fn()
  render(
    <PackLogsDialog open onOpenChange={onOpenChange} sessions={SESSIONS} current="20260102-000000" />,
  )
  return onOpenChange
}

const packRequest = () => {
  const call = invoke.mock.calls.find(([cmd]) => cmd === 'pack_logs')
  return (call?.[1] as { req: Record<string, unknown> }).req
}

describe('PackLogsDialog', () => {
  beforeEach(() => {
    invoke.mockReset()
    invoke.mockResolvedValue({
      path: '/tmp/akagi-logs.zip',
      sessions: 1,
      files: 1,
      sensitive_files: 0,
      withheld_files: 0,
      encrypted: false,
    })
  })

  it('packs the viewed run without full information by default', async () => {
    const onOpenChange = renderDialog()
    fireEvent.click(screen.getByText('logs.pack.submit'))
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
    expect(packRequest()).toEqual({
      sessions: ['20260102-000000'],
      include_sensitive: false,
      password: null,
      dest: '/tmp/akagi-logs.zip',
    })
  })

  it('sends the password only while full information is included', async () => {
    renderDialog()
    fireEvent.click(screen.getByRole('switch'))
    fireEvent.change(screen.getByLabelText('logs.pack.password'), { target: { value: 'pw' } })
    fireEvent.click(screen.getByText('logs.pack.submit'))
    await waitFor(() => expect(packRequest()).toMatchObject({ include_sensitive: true, password: 'pw' }))

    invoke.mockClear()
    fireEvent.click(screen.getByRole('switch'))
    fireEvent.click(screen.getByText('logs.pack.submit'))
    await waitFor(() => expect(packRequest()).toMatchObject({ include_sensitive: false, password: null }))
  })

  it('explains that older runs need full information', () => {
    renderDialog()
    expect(screen.queryByText('logs.pack.legacy_note')).toBeNull()
    fireEvent.click(screen.getByText('logs.pack.all'))
    expect(screen.getByText('logs.pack.legacy_note')).toBeTruthy()
    fireEvent.click(screen.getByRole('switch'))
    expect(screen.queryByText('logs.pack.legacy_note')).toBeNull()
  })
})
