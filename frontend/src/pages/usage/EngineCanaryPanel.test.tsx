import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { EngineCanaryPanel } from './EngineCanaryPanel'
import { useAuthStore } from '@/stores/authStore'
import type { EngineCapabilities, EngineMetricsRow } from '@/hooks/useEngineRollout'
import i18n from '@/i18n'

const api = vi.hoisted(() => ({ fetchJson: vi.fn() }))
vi.mock('@/lib/api-v2/client', () => ({ fetchJson: api.fetchJson }))
const capability: EngineCapabilities = {
  rig_available: true, rig_paused: false, default_engine: 'native', app_version: '0.1.7', rig_version: '0.45.0',
}
const row: EngineMetricsRow = {
  engine: 'rig', app_version: '0.1.7', engine_version: '0.45.0', provider_id: 'p', provider_kind: 'anthropic', model: 'canary-model',
  executions: 3, partial_executions: 1, completed: 1, failed: 0, cancelled: 0, interrupted: 1, pending: 1, blocked: 0, outcome_unknown: 0,
  active_duration_ms: 100, tool_calls: 0, settled_tool_calls: 0, tool_errors: 0, unsettled_tools: 0,
  total_tokens: 0, usage_calls: 0, executions_without_usage: 1, failure_rate: 0, tool_error_rate: null,
}
beforeEach(async () => {
  await i18n.changeLanguage('en-US')
  useAuthStore.setState({ token: 'owner-one' })
  api.fetchJson.mockReset()
  api.fetchJson.mockImplementation(async (path: string) => path.includes('engine-metrics')
    ? { days: 7, rows: [row], unattributed_calls: 4 } : capability)
})
afterEach(() => {
  cleanup()
  useAuthStore.setState({ token: null })
})
function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(<QueryClientProvider client={client}><EngineCanaryPanel /></QueryClientProvider>)
  return client
}

it('shows missing coverage explicitly and pauses intake without changing the default', async () => {
  const user = userEvent.setup()
  api.fetchJson.mockImplementation(async (path: string, opts: { method?: string }) => {
    if (opts.method === 'PATCH') return { ...capability, rig_paused: true }
    return path.includes('engine-metrics') ? { days: 7, rows: [row], unattributed_calls: 4 } : capability
  })
  mount()
  expect(await screen.findByText('anthropic · canary-model')).toBeInTheDocument()
  expect(screen.getByText('—*')).toBeInTheDocument()
  expect(screen.getByText(/partial 1/)).toBeInTheDocument()
  expect(screen.getByText(/lower measured tokens do not prove lower cost/)).toBeInTheDocument()
  await user.click(screen.getByRole('button', { name: 'Pause new Rig executions' }))
  expect(await screen.findByRole('status')).toHaveTextContent('pending approvals retain their engine')
  expect(api.fetchJson).toHaveBeenCalledWith('/runtime/engines/rig', {
    token: 'owner-one', method: 'PATCH', body: { paused: true },
  })
  expect(screen.getByRole('button', { name: 'Resume Rig intake' })).toBeInTheDocument()
})

it('keeps a delayed pause response in its original account after login changes', async () => {
  const user = userEvent.setup()
  let finishPause!: (value: EngineCapabilities) => void
  api.fetchJson.mockImplementation(async (path: string, opts: { method?: string, token?: string }) => {
    if (opts.method === 'PATCH') return new Promise<EngineCapabilities>(resolve => { finishPause = resolve })
    if (path.includes('engine-metrics')) return { days: 7, rows: opts.token === 'owner-one' ? [row] : [], unattributed_calls: 0 }
    return capability
  })
  const client = mount()
  await screen.findByText('anthropic · canary-model')
  await user.click(screen.getByRole('button', { name: 'Pause new Rig executions' }))
  act(() => useAuthStore.setState({ token: 'owner-two' }))
  await screen.findByText('No executions measured in this window.')
  await act(async () => finishPause({ ...capability, rig_paused: true }))
  await waitFor(() => expect(client.getQueryData(['runtime-engines', 'owner-one'])).toEqual({ ...capability, rig_paused: true }))
  expect(client.getQueryData(['runtime-engines', 'owner-two'])).toEqual(capability)
  expect(screen.queryByRole('status')).not.toBeInTheDocument()
  expect(screen.queryByText('anthropic · canary-model')).not.toBeInTheDocument()
})
