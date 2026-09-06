import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MobileConnection } from './MobileConnection'
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ i18n: { language: 'en-US' } }) }))
afterEach(() => { cleanup(); mocks.invoke.mockReset() })
const off = { endpoint: null, interfaces: [{ name: 'Wi-Fi', address: '192.168.1.2' }], devices: [] }
describe('desktop phone pairing', () => {
  it('requires an explicit action and never presents a QR when the listener fails', async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'mobile_link_status') return off
      throw new Error('Address unavailable')
    })
    render(<MobileConnection />)
    const start = await screen.findByRole('button', { name: 'Enable and pair' })
    await waitFor(() => expect(start).toBeEnabled())
    expect(mocks.invoke.mock.calls.every(([command]) => command === 'mobile_link_status')).toBe(true)
    await userEvent.click(start)
    expect(await screen.findByRole('alert')).toHaveTextContent('Address unavailable')
    expect(mocks.invoke).not.toHaveBeenCalledWith('mobile_link_offer')
    expect(screen.queryByTitle('Phone pairing code')).toBeNull()
  })
  it('generates a code only after binding and clears it when sharing is stopped', async () => {
    let status: { endpoint: string | null; interfaces: typeof off.interfaces; devices: [] } = { ...off, devices: [] }
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'mobile_link_start') status = { ...status, endpoint: '192.168.1.2:8766' }
      if (command === 'mobile_link_stop') status = { ...status, endpoint: null }
      if (command === 'mobile_link_offer') return { uri: 'qunica://pair?data=once', expiresAt: Date.now() / 1000 + 120 }
      return status
    })
    render(<MobileConnection />)
    await waitFor(() => expect(screen.getByRole('button', { name: 'Enable and pair' })).toBeEnabled())
    await userEvent.click(screen.getByRole('button', { name: 'Enable and pair' }))
    expect(await screen.findByTitle('Phone pairing code')).toBeInTheDocument()
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_link_start', { address: '192.168.1.2' })
    await userEvent.click(screen.getByRole('button', { name: 'Stop sharing' }))
    await waitFor(() => expect(screen.queryByTitle('Phone pairing code')).toBeNull())
  })
})
