import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import {
  CommandPaletteProvider,
  useOpenCommandPalette,
} from './CommandPalette'

const mocks = vi.hoisted(() => ({
  settings: { appearance: 'system', language: 'en-US' },
  updateMutateAsync: vi.fn(),
  openLibraryWindow: vi.fn(),
  openSettingsWindow: vi.fn(),
}))

vi.mock('@/hooks/useSystemSettings', () => ({
  useSystemSettings: () => ({ data: mocks.settings, isLoading: false }),
  useUpdateSystemSettings: () => ({
    mutateAsync: mocks.updateMutateAsync,
    isPending: false,
  }),
}))
vi.mock('@/lib/desktop', async () => {
  const actual = await vi.importActual<typeof import('@/lib/desktop')>('@/lib/desktop')
  return {
    ...actual,
    openLibraryWindow: mocks.openLibraryWindow,
    openSettingsWindow: mocks.openSettingsWindow,
  }
})

function LocationProbe() {
  const location = useLocation()
  const state = location.state as { backgroundLocation?: { pathname: string } } | null
  return (
    <>
      <div data-testid="location">{location.pathname}</div>
      <div data-testid="background-location">
        {state?.backgroundLocation?.pathname ?? ''}
      </div>
    </>
  )
}

/** The only way chrome opens the palette is through the context hook. */
function OpenButton() {
  const open = useOpenCommandPalette()
  return (
    <button type="button" onClick={open}>
      open
    </button>
  )
}

function renderPalette(initialEntry = '/chats/chat-1') {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <CommandPaletteProvider>
          <OpenButton />
          <LocationProbe />
        </CommandPaletteProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('CommandPalette', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    mocks.updateMutateAsync.mockReset()
    mocks.updateMutateAsync.mockResolvedValue({})
    mocks.openLibraryWindow.mockReset().mockResolvedValue(undefined)
    mocks.openSettingsWindow.mockReset().mockResolvedValue(undefined)
    window.localStorage.clear()
  })
  afterEach(cleanup)

  it('opens from Ctrl+K anywhere and closes on Escape', async () => {
    const { unmount } = renderPalette()

    // Global chord while nothing is focused — this is what makes it a command
    // menu rather than a button with extra steps.
    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    expect(await screen.findByRole('dialog')).toBeVisible()

    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    await waitForDialogToClose()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    unmount()
  })

  it('toggles the palette off when the chord repeats', async () => {
    const { unmount } = renderPalette()
    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    expect(await screen.findByRole('dialog')).toBeVisible()
    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await waitForDialogToClose()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    unmount()
  })

  it('opens through the sidebar hook', async () => {
    const user = userEvent.setup()
    const { unmount } = renderPalette()
    await user.click(screen.getByRole('button', { name: 'open' }))
    expect(await screen.findByRole('dialog')).toBeVisible()
    unmount()
  })

  it('filters across groups and navigates as an overlay over the stage', async () => {
    const user = userEvent.setup()
    const { unmount } = renderPalette('/chats/chat-1')

    fireEvent.keyDown(document, { key: 'k', metaKey: true })
    await screen.findByRole('dialog')
    await user.type(screen.getByRole('textbox'), 'agent')

    const options = screen.getAllByRole('option').map((node) => node.textContent)
    // The navigation label is the area's plural form; the create action is
    // singular. Both must surface for one query.
    expect(options).toContain('Agents')
    expect(options).toContain('New agent')
    expect(options).not.toContain('Token usage')

    await user.click(screen.getByRole('option', { name: 'Agents' }))
    await waitForDialogToClose()
    expect(screen.getByTestId('location')).toHaveTextContent('/agents')
    // The conversation underneath survives the jump — same contract as every
    // other link into an overlay area.
    expect(screen.getByTestId('background-location')).toHaveTextContent(
      '/chats/chat-1',
    )
    unmount()
  })

  it('switches theme instantly and closes', async () => {
    const user = userEvent.setup()
    const { unmount } = renderPalette()

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('option', { name: 'Dark' }))

    expect(mocks.updateMutateAsync).toHaveBeenCalledWith({ appearance: 'dark' })
    await waitForDialogToClose()
    unmount()
  })

  it('switches language instantly; the palette relabels on next open', async () => {
    const user = userEvent.setup()
    const { unmount } = renderPalette()

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('option', { name: '中文' }))

    expect(mocks.updateMutateAsync).toHaveBeenCalledWith({ language: 'zh-CN' })
    // Success closes the palette like every other command; the relabeled
    // placeholder proves the switch took effect app-wide, not just here.
    await waitForDialogToClose()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    expect(await screen.findByPlaceholderText('搜索命令…')).toBeVisible()

    unmount()
    await i18n.changeLanguage('en-US')
  })

  it('shows an empty state for unmatched queries instead of a blank list', async () => {
    const user = userEvent.setup()
    const { unmount } = renderPalette()

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.type(screen.getByRole('textbox'), 'zzzz-no-match')
    expect(screen.getByText('No matching commands.')).toBeVisible()
    unmount()
  })

  it('opens library and settings as native windows from the desktop conversation', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('__TAURI_INTERNALS__', {
      metadata: { currentWindow: { label: 'main' } },
    })
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...window.location, hostname: 'tauri.localhost' },
    })
    const { unmount } = renderPalette('/chats/chat-1')

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('option', { name: 'Providers' }))
    await waitForDialogToClose()
    expect(mocks.openLibraryWindow).toHaveBeenCalledWith('/providers')
    expect(screen.getByTestId('location')).toHaveTextContent('/chats/chat-1')

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('option', { name: 'Runtime logs' }))
    await waitForDialogToClose()
    expect(mocks.openSettingsWindow).toHaveBeenCalledWith('/settings/logs')
    expect(screen.getByTestId('location')).toHaveTextContent('/chats/chat-1')
    unmount()
    vi.unstubAllGlobals()
  })

  it('keeps native auxiliary windows in their own route families', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('__TAURI_INTERNALS__', {
      metadata: { currentWindow: { label: 'library' } },
    })
    const { unmount } = renderPalette('/agents')

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('option', { name: 'Providers' }))
    await waitForDialogToClose()
    expect(screen.getByTestId('location')).toHaveTextContent('/providers')
    expect(mocks.openLibraryWindow).not.toHaveBeenCalled()

    fireEvent.keyDown(document, { key: 'k', ctrlKey: true })
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('option', { name: 'Runtime logs' }))
    await waitForDialogToClose()
    expect(mocks.openSettingsWindow).toHaveBeenCalledWith('/settings/logs')
    expect(screen.getByTestId('location')).toHaveTextContent('/providers')

    unmount()
    vi.unstubAllGlobals()
  })
})

function waitForDialogToClose(): Promise<void> {
  // Radix removes the dialog synchronously; a tick keeps the assertion honest
  // about having waited for the state flip rather than racing it.
  return new Promise((resolve) => setTimeout(resolve, 0))
}
