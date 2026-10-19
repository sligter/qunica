import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { ComputerDesktop } from './ComputerDesktop'

const mocks = vi.hoisted(() => ({ request: vi.fn(), rfbs: [] as { viewOnly: boolean; disconnect: () => void }[] }))
vi.mock('@/lib/api-v2/client', () => ({ fetchJson: (...args: unknown[]) => mocks.request(...args) }))
vi.mock('@/lib/realtime/client', () => ({ realtimeClient: () => ({ request: async () => ({ ticket: 'one-use-ticket' }) }) }))
vi.mock('@novnc/novnc/lib/rfb', () => ({ default: class extends EventTarget {
  viewOnly = false
  disconnect = vi.fn()
  constructor() { super(); mocks.rfbs.push(this) }
} }))
class Socket {
  static instances: Socket[] = []
  onopen?: () => void
  onmessage?: (message: { data: string }) => void
  onclose?: () => void
  frames: Record<string, unknown>[] = []
  close = vi.fn()
  constructor() { Socket.instances.push(this) }
  send(raw: string) { this.frames.push(JSON.parse(raw)) }
  authenticate() { this.onopen?.(); this.onmessage?.({ data: '{"type":"authenticated"}' }) }
}
beforeEach(async () => {
  await i18n.changeLanguage('en-US')
  useAuthStore.setState({ token: 'account-one' })
  Socket.instances = []; mocks.rfbs = []
  vi.stubGlobal('WebSocket', Socket)
  let state = 'agent_controlled'
  mocks.request.mockReset().mockImplementation(async (_path: string, opts: { body: { action: string } }) => {
    const action = opts.body.action
    if (action === 'request') state = 'user_controlled'
    if (action === 'release') state = 'released'
    if (action === 'resume') state = 'agent_controlled'
    return { state, generation: '4', credential: action === 'request' ? 'exclusive-control-secret' : null, expires_at: 9999999999 }
  })
})
afterEach(() => { cleanup(); useAuthStore.setState({ token: null }); vi.unstubAllGlobals() })
function page() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(<QueryClientProvider client={client}><ComputerDesktop id="computer-1" name="Build computer" generation="4" /></QueryClientProvider>)
  return userEvent.setup()
}
it('starts read-only, fences input with a lease and requires an explicit return after release', async () => {
  const user = page()
  await user.click(screen.getByRole('button', { name: 'Open desktop' }))
  await waitFor(() => expect(Socket.instances).toHaveLength(1))
  Socket.instances[0].authenticate()
  expect(Socket.instances[0].frames[0]).toMatchObject({ ticket: 'one-use-ticket', computer_id: 'computer-1', generation: '4', credential: null })
  expect(mocks.rfbs[0].viewOnly).toBe(true)
  await user.click(await screen.findByRole('button', { name: 'Take control' }))
  await waitFor(() => expect(Socket.instances).toHaveLength(2))
  Socket.instances[1].authenticate()
  expect(Socket.instances[1].frames[0].credential).toBe('exclusive-control-secret')
  expect(mocks.rfbs[1].viewOnly).toBe(false)
  await user.click(screen.getByRole('button', { name: 'Release control' }))
  expect(await screen.findByRole('button', { name: 'Return to agent' })).toBeVisible()
  expect(mocks.request.mock.calls.some(([, opts]) => opts.body.action === 'resume')).toBe(false)
  await user.click(screen.getByRole('button', { name: 'Return to agent' }))
  await waitFor(() => expect(mocks.request.mock.calls.some(([, opts]) => opts.body.action === 'resume')).toBe(true))
})
it('never forwards an old account control credential after login changes', async () => {
  const user = page()
  await user.click(screen.getByRole('button', { name: 'Open desktop' }))
  await user.click(await screen.findByRole('button', { name: 'Take control' }))
  await waitFor(() => expect(Socket.instances.length).toBeGreaterThanOrEqual(1))
  const before = Socket.instances.length
  useAuthStore.setState({ token: 'account-two' })
  await waitFor(() => expect(Socket.instances.length).toBeGreaterThan(before))
  const socket = Socket.instances.at(-1)!
  socket.authenticate()
  expect(socket.frames[0].credential).toBeNull()
})
