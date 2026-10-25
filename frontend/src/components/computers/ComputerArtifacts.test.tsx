import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { ComputerArtifacts } from './ComputerArtifacts'
import { useAndroidSession } from '@/lib/androidSession'

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), saveAndroid: vi.fn(), saveDesktop: vi.fn(), android: false, desktop: false }))
vi.mock('@/lib/authFetch', async original => ({ ...await original<typeof import('@/lib/authFetch')>(), authFetch: mocks.fetch }))
vi.mock('@/lib/androidSession', async original => ({ ...await original<typeof import('@/lib/androidSession')>(), isAndroidRuntime: () => mocks.android }))
vi.mock('@/lib/androidFileExport', () => ({ saveAndroidResponse: mocks.saveAndroid }))
vi.mock('@/lib/desktop', () => ({ isDesktopRuntime: () => mocks.desktop, saveFileViaDialog: mocks.saveDesktop }))

vi.mock('@/lib/api-v2/client', () => ({
  fetchJson: async () => ({ items: [
    { id: 'large', path: 'large.zip', size: 9 * 1024 * 1024, turn_id: 'turn' },
    { id: 'limit', path: 'limit.zip', size: 1000 * 1024 * 1024, turn_id: 'turn' },
    { id: 'over', path: 'over.zip', size: 1000 * 1024 * 1024 + 1, turn_id: 'turn' },
  ] }),
}))
afterEach(() => { cleanup(); useAuthStore.setState({ token: null }); useAndroidSession.setState({ server: null }); vi.clearAllMocks(); mocks.android = false; mocks.desktop = false })

async function openArtifacts() {
  await i18n.changeLanguage('en-US')
  if (mocks.android) useAndroidSession.setState({ server: 'https://qunica.example' })
  useAuthStore.setState({ token: 'account' })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><ComputerArtifacts id="computer" /></QueryClientProvider>)
  await userEvent.click(screen.getByRole('button', { name: 'Task artifacts' }))
  const row = (await screen.findAllByRole('listitem'))[0]
  return { ...view, download: within(row).getByRole('button', { name: 'Download' }) }
}

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

it('saves artifacts through the Android picker and keeps duplicate downloads disabled until it returns', async () => {
  mocks.android = true
  const response = new Response('fixture')
  mocks.fetch.mockResolvedValue(response)
  let complete!: (uri: string | null) => void
  mocks.saveAndroid.mockImplementation((_name, _response, _size, progress) => {
    progress(9 * 1024 * 1024)
    return new Promise(resolve => { complete = resolve })
  })
  const view = await openArtifacts()
  await userEvent.click(view.download)
  expect(mocks.fetch).toHaveBeenCalledWith(expect.stringContaining('/api/v2/computer-artifacts/large'), expect.objectContaining({ headers: { Authorization: 'Bearer account' }, signal: expect.any(AbortSignal) }))
  expect(mocks.saveAndroid).toHaveBeenCalledWith('large.zip', response, 9 * 1024 * 1024, expect.any(Function), expect.any(AbortSignal))
  expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '100')
  expect(screen.getByRole('status')).toHaveTextContent('Choose where to save')
  expect(screen.getByRole('button', { name: 'Downloading 100%' })).toBeDisabled()
  complete(null)
  await waitFor(() => expect(screen.queryByRole('progressbar')).toBeNull())
  expect(view.download).toBeEnabled()
  expect(mocks.saveDesktop).not.toHaveBeenCalled()
})

it('never opens the Android picker for a failed download', async () => {
  mocks.android = true
  mocks.fetch.mockResolvedValue(new Response('', { status: 403 }))
  const view = await openArtifacts()
  await userEvent.click(view.download)
  expect(await screen.findByRole('alert')).toHaveTextContent('Artifact unavailable')
  expect(mocks.saveAndroid).not.toHaveBeenCalled()
})

it('aborts an active artifact transfer when the account changes', async () => {
  let signal!: AbortSignal
  mocks.fetch.mockImplementation((_url, options) => {
    signal = options.signal
    return new Promise((_resolve, reject) => signal.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError'))))
  })
  const view = await openArtifacts()
  await userEvent.click(view.download)
  useAuthStore.setState({ token: 'another-account' })
  await waitFor(() => expect(signal.aborted).toBe(true))
  await waitFor(() => expect(screen.queryByRole('progressbar')).toBeNull())
  expect(screen.queryByRole('alert')).toBeNull()
})
