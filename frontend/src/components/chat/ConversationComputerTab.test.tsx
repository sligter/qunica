import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { ConversationComputerTab } from './ConversationComputerTab'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'

const mocks = vi.hoisted(() => ({ request: vi.fn(), status: 'running' }))
vi.mock('@/lib/api-v2/client', async () => ({ ...await vi.importActual('@/lib/api-v2/client'), fetchJson: (...args: unknown[]) => mocks.request(...args) }))
vi.mock('@/components/computers/ComputerDesktop', () => ({ ComputerDesktop: ({ id }: { id: string }) => <div>desktop:{id}</div> }))

beforeEach(async () => {
  await i18n.changeLanguage('en-US')
  useAuthStore.setState({ token: 'panel-account' })
  mocks.status = 'running'
  mocks.request.mockReset().mockImplementation(async (path: string, options?: { body?: { action: string } }) => {
    if (path === '/computers') return { configured: true, items: [
      { id: 'team-1', name: 'Studio computer', binding: { kind: 'team', id: 'group-1' }, selected: true },
      { id: 'other-team', name: 'Unrelated computer', binding: { kind: 'team', id: 'group-2' }, selected: true },
      { id: 'private-1', name: 'Agent computer', binding: { kind: 'private', id: 'agent-1' }, selected: false },
      { id: 'other-private', name: 'Former member computer', binding: { kind: 'private', id: 'former-agent' }, selected: true },
    ] }
    if (path === '/groups/group-1/agents') return [{ agent_id: 'agent-1', status: 'active' }, { agent_id: 'former-agent', status: 'left' }]
    if (path === '/direct-chats/chat-1') return { agent_id: 'agent-1' }
    if (path.endsWith('/actions')) { if (options?.body?.action === 'start') mocks.status = 'running'; return {} }
    return { generation: '7', status: mocks.status, capabilities: { desktop: true }, error_code: null }
  })
})
afterEach(() => { cleanup(); useAuthStore.setState({ token: null }) })

function page(scope: 'groups' | 'direct-chats' = 'groups', id = 'group-1') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ConversationComputerTab scope={scope} conversationId={id} /></MemoryRouter></QueryClientProvider>)
  return userEvent.setup()
}

it('shows only the current Team computer and active members’ Private computers', async () => {
  const user = page()
  expect(await screen.findByText('desktop:team-1')).toBeVisible()
  expect(screen.getByRole('option', { name: 'Studio computer · Team' })).toBeInTheDocument()
  expect(await screen.findByRole('option', { name: 'Agent computer · Private' })).toBeInTheDocument()
  expect(screen.queryByRole('option', { name: /Unrelated|Former member/ })).not.toBeInTheDocument()
  await user.selectOptions(screen.getByRole('combobox', { name: 'Conversation computer' }), 'private-1')
  expect(await screen.findByText('desktop:private-1')).toBeVisible()
  expect(screen.getByRole('note')).toHaveTextContent('Connecting its desktop does not route Agent tools here')
  expect(screen.getByRole('note')).toHaveTextContent('Private computer takes priority')
  await user.click(screen.getByRole('button', { name: 'Use for execution' }))
  await waitFor(() => expect(mocks.request).toHaveBeenCalledWith('/execution-environments/binding', expect.objectContaining({ method: 'PUT', body: { scope: 'agent', target_id: 'agent-1', execution_environment_id: 'private-1' } })))
})

it('does not show an unbound warning for the selected Team computer', async () => {
  page()
  expect(await screen.findByText('desktop:team-1')).toBeVisible()
  expect(screen.queryByRole('note')).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Use for execution' })).not.toBeInTheDocument()
})

it('uses the direct chat Agent’s computer without querying group membership', async () => {
  page('direct-chats', 'chat-1')
  expect(await screen.findByText('desktop:private-1')).toBeVisible()
  expect(screen.getAllByRole('option')).toHaveLength(1)
  expect(mocks.request.mock.calls.some(([path]) => path.includes('/groups/'))).toBe(false)
})

it('starts a stopped computer with its current generation before opening its desktop', async () => {
  mocks.status = 'stopped'
  const user = page()
  expect(await screen.findByText('Computer is stopped')).toBeVisible()
  expect(screen.queryByText('desktop:team-1')).not.toBeInTheDocument()
  await user.click(screen.getByRole('button', { name: 'Start' }))
  await waitFor(() => expect(mocks.request).toHaveBeenCalledWith('/computers/team-1/actions', expect.objectContaining({ method: 'POST', body: { action: 'start', expected_generation: '7', confirm: null, purge_home: false } })))
  expect(await screen.findByText('desktop:team-1')).toBeVisible()
})

it('reports list failures and lets the user refresh', async () => {
  mocks.request.mockRejectedValue(new Error('computer service offline'))
  page()
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not load computers')
  expect(screen.getByRole('button', { name: 'Refresh computer' })).toBeVisible()
})

it('keeps the same desktop mounted while an Agent command is executing', async () => {
  const user = page()
  const desktop = await screen.findByText('desktop:team-1')
  mocks.status = 'executing'
  await user.click(screen.getByRole('button', { name: 'Refresh computer' }))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeDisabled())
  expect(screen.getByText('desktop:team-1')).toBe(desktop)
})
