import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import userEvent from '@testing-library/user-event'
import { AndroidShell } from './AndroidShell'
import { useAndroidSession } from '@/lib/androidSession'
import { useAuthStore } from '@/stores/authStore'
import i18n from '@/i18n'
import { MobileConnectionStatus } from './MobileConnectionStatus'

const mocks = vi.hoisted(() => ({ initialize: vi.fn(), scan: vi.fn(), permission: vi.fn(), changeServer: vi.fn(), pair: vi.fn() }))
vi.mock('@/lib/androidSession', async (original) => ({
  ...await original<typeof import('@/lib/androidSession')>(),
  initializeAndroidSession: mocks.initialize,
  changeAndroidServer: mocks.changeServer,
  pairAndroidDesktop: mocks.pair,
}))
vi.mock('@/lib/realtime/instance', () => ({ checkInstance: async () => ({}) }))
vi.mock('@tauri-apps/plugin-barcode-scanner', () => ({ scan: mocks.scan, requestPermissions: mocks.permission, Format: { QRCode: 'QR_CODE' } }))

beforeEach(async () => { await i18n.changeLanguage('en-US'); mocks.permission.mockResolvedValue('granted') })
afterEach(() => {
  cleanup(); Object.values(mocks).forEach(mock => mock.mockReset())
  useAuthStore.setState({ token: null, user: null, hydrated: false })
  useAndroidSession.setState({ server: null, ready: false, error: null })
})

async function openCurrentConnection() {
  mocks.initialize.mockImplementation(async () => {
    useAndroidSession.setState({ ready: true, server: 'https://qunica.example' })
    return 'existing-login'
  })
  render(<AndroidShell><div>Current workspace</div><MobileConnectionStatus /></AndroidShell>)
  await screen.findByText('Current workspace')
  await userEvent.click(screen.getByRole('button', { name: 'Switch connection' }))
}

it('keeps the login when reconnecting to the same normalized server', async () => {
  await openCurrentConnection()
  await userEvent.type(screen.getByRole('textbox', { name: 'Server address' }), 'https://QUNICA.example:443/')
  await userEvent.click(screen.getByRole('button', { name: 'Connect and sign in' }))
  expect(await screen.findByText('Current workspace')).toBeVisible()
  expect(useAuthStore.getState().token).toBe('existing-login')
  expect(mocks.changeServer).not.toHaveBeenCalled()
})

it('provides connection actions to navigation without adding a persistent connection toolbar', async () => {
  mocks.initialize.mockImplementation(async () => {
    useAndroidSession.setState({ ready: true, server: 'https://qunica.example' })
    return 'existing-login'
  })
  render(<AndroidShell><div>Current workspace</div></AndroidShell>)
  expect(await screen.findByText('Current workspace')).toBeVisible()
  expect(screen.queryByRole('button', { name: 'Switch connection' })).toBeNull()
  expect(screen.queryByText('https://qunica.example')).toBeNull()
})

it('recognizes a server QR and preserves the current login when scanning the same instance', async () => {
  await openCurrentConnection()
  mocks.scan.mockResolvedValue({ content: 'https://QUNICA.example:443/' })
  await userEvent.click(screen.getByRole('button', { name: 'Scan a connection QR code' }))
  expect(await screen.findByText('Current workspace')).toBeVisible()
  expect(useAuthStore.getState().token).toBe('existing-login')
  expect(mocks.pair).not.toHaveBeenCalled()
  expect(mocks.changeServer).not.toHaveBeenCalled()
})

it('rejects a server QR containing credentials before changing the connection', async () => {
  await openCurrentConnection()
  mocks.scan.mockResolvedValue({ content: 'https://user:secret@qunica.example' })
  await userEvent.click(screen.getByRole('button', { name: 'Scan a connection QR code' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('without a path, credentials')
  expect(mocks.changeServer).not.toHaveBeenCalled()
  expect(mocks.pair).not.toHaveBeenCalled()
  expect(useAuthStore.getState().token).toBe('existing-login')
})

it('keeps routes unmounted until the restored account token is installed', async () => {
  let complete!: (token: string) => void
  mocks.initialize.mockReturnValue(new Promise<string>(resolve => { complete = resolve }))
  const mountedTokens: (string | null)[] = []
  function Routes() {
    const token = useAuthStore(s => s.token)
    mountedTokens.push(token)
    return <div>Authenticated workspace</div>
  }
  render(<AndroidShell><Routes /></AndroidShell>)
  // Vault restoration publishes its origin before its promise resolves.
  act(() => useAndroidSession.setState({ ready: true, server: 'https://qunica-lan.invalid' }))
  expect(screen.queryByText('Authenticated workspace')).toBeNull()
  // Neither the workspace nor a second pairing form is offered while restoring.
  expect(screen.queryAllByRole('button')).toHaveLength(0)
  expect(screen.getByRole('status')).toBeInTheDocument()
  await act(async () => { complete('paired-account-token') })
  expect(screen.getByText('Authenticated workspace')).toBeInTheDocument()
  expect(mountedTokens).not.toContain(null)
  expect(useAuthStore.getState().token).toBe('paired-account-token')
})
