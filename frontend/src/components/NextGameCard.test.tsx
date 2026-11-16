import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { NextGameCard } from './NextGameCard'

const invoke = vi.fn()
vi.mock('@/lib/tauri', () => ({ invoke: (cmd: string, args?: unknown) => invoke(cmd, args) }))
const stopped = { enabled: false, limited: true, limit: 3, remaining: 3, started: 0, reason: '自动续局已关闭', ready: true }

describe('integrated continuation controls', () => {
  beforeEach(() => { localStorage.clear(); invoke.mockReset(); invoke.mockResolvedValue(stopped) })

  it('starts with the chosen extra count and disables editing until stopped', async () => {
    render(<NextGameCard canStart />)
    await screen.findByText('自动续局已关闭')
    fireEvent.change(screen.getByLabelText('额外场数'), { target: { value: '2' } })
    invoke.mockResolvedValue({ ...stopped, enabled: true, limit: 2, remaining: 2 })
    fireEvent.click(screen.getByRole('switch', { name: '自动开始下一场' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('control_next_game', { control: { enabled: true, limited: true, limit: 2 } }))
    await waitFor(() => expect(screen.getByLabelText('额外场数').hasAttribute('disabled')).toBe(true))
    invoke.mockResolvedValue(stopped)
    fireEvent.click(screen.getByRole('button', { name: '停止后续续局' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('control_next_game', { control: { enabled: false, limited: true, limit: 2 } }))
  })

  it('never arms when configuration is unsupported or the count is invalid', async () => {
    const view = render(<NextGameCard canStart={false} />)
    await screen.findByText('自动续局已关闭')
    expect(screen.getByRole('switch', { name: '自动开始下一场' }).hasAttribute('disabled')).toBe(true)
    view.rerender(<NextGameCard canStart />)
    fireEvent.change(screen.getByLabelText('额外场数'), { target: { value: '0' } })
    expect(screen.getByRole('switch', { name: '自动开始下一场' }).hasAttribute('disabled')).toBe(true)
    expect(invoke.mock.calls.every(([cmd]) => cmd === 'get_next_game_status')).toBe(true)
  })

  it('remembers the count preference but does not re-enable on remount', async () => {
    const view = render(<NextGameCard canStart />)
    await screen.findByText('自动续局已关闭')
    fireEvent.change(screen.getByLabelText('额外场数'), { target: { value: '5' } })
    view.unmount()
    render(<NextGameCard canStart />)
    await screen.findByText('自动续局已关闭')
    expect((screen.getByLabelText('额外场数') as HTMLInputElement).value).toBe('5')
    expect(screen.getByRole('switch', { name: '自动开始下一场' }).getAttribute('aria-checked')).toBe('false')
  })
})
