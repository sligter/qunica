import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { ComputerArtifacts } from './ComputerArtifacts'

vi.mock('@/lib/api-v2/client', () => ({
  fetchJson: async () => ({ items: [
    { id: 'large', path: 'large.zip', size: 9 * 1024 * 1024, turn_id: 'turn' },
    { id: 'limit', path: 'limit.zip', size: 1000 * 1024 * 1024, turn_id: 'turn' },
    { id: 'over', path: 'over.zip', size: 1000 * 1024 * 1024 + 1, turn_id: 'turn' },
  ] }),
}))
afterEach(() => { cleanup(); useAuthStore.setState({ token: null }) })

it('allows downloads above 8 MiB through exactly 1000 MiB and disables larger files', async () => {
  await i18n.changeLanguage('en-US')
  useAuthStore.setState({ token: 'account' })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><ComputerArtifacts id="computer" /></QueryClientProvider>)
  await userEvent.setup().click(screen.getByRole('button', { name: 'Task artifacts' }))
  const buttons = (await screen.findAllByRole('listitem')).map(item => within(item).getByRole('button', { name: 'Download' }))
  expect(buttons[0]).toBeEnabled()
  expect(buttons[1]).toBeEnabled()
  expect(buttons[2]).toBeDisabled()
})
