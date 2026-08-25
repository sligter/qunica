import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { createMemoryRouter, RouterProvider, type Location } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AppLayout } from '@/components/layout/AppLayout'
import { LegacyDetailRedirect } from '@/components/layout/LegacyDetailRedirect'
import {
  isGroupManagePath,
  isOverlayPath,
  overlayAreaLabelKey,
  overlayLinkState,
  OverlayRedirect,
} from '@/components/layout/overlayRouting'
import { TooltipProvider } from '@/components/ui/tooltip'
import { enUS } from '@/i18n/resources/en-US'

// Counts every time the conversation surface unmounts. The whole point of the
// overlay is that this never happens while settings floats above it.
const state = vi.hoisted(() => ({ conversationUnmounts: 0 }))

vi.mock('@/components/assistant/AssistantDock', () => ({
  AssistantDock: () => <div data-testid="assistant-dock" />,
}))

// The conversation surface. Locally owned draft state is the stand-in for
// everything the overlay must preserve (composer draft, scroll, open panels).
vi.mock('@/pages/group/GroupChatPage', async () => {
  const { useEffect, useState } = await import('react')
  const { OverlayLink } = await import('@/components/layout/overlayRouting')
  function GroupChatPage() {
    const [draft, setDraft] = useState('')
    useEffect(() => {
      return () => {
        state.conversationUnmounts += 1
      }
    }, [])
    return (
      <>
        <input
          aria-label="Draft"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
        />
        <OverlayLink to="/settings">Open settings</OverlayLink>
        <OverlayLink to="/usage">Open usage</OverlayLink>
      </>
    )
  }
  return { GroupChatPage }
})

vi.mock('@/pages/settings/SystemSettingsPage', () => ({
  SystemSettingsPage: () => <div>System settings content</div>,
}))

vi.mock('@/pages/settings/MediaSettingsPage', () => ({
  MediaSettingsPage: () => <div>Media settings content</div>,
}))

vi.mock('@/hooks/useTokenUsage', () => ({
  useTokenUsage: () => ({
    data: {
      summary: { input_tokens: 120, output_tokens: 30, total_tokens: 150, calls: 2, active_agents: 2 },
      timeline: [
        { date: '2026-08-21', input_tokens: 80, output_tokens: 20, total_tokens: 100, calls: 1 },
        { date: '2026-08-22', input_tokens: 40, output_tokens: 10, total_tokens: 50, calls: 1 },
      ],
      by_group: [
        {
          id: 'group-1', name: 'Research', input_tokens: 80, output_tokens: 20, total_tokens: 100, calls: 1,
          timeline: [{ date: '2026-08-21', input_tokens: 80, output_tokens: 20, total_tokens: 100, calls: 1 }],
        },
        {
          id: 'group-2', name: 'Build', input_tokens: 40, output_tokens: 10, total_tokens: 50, calls: 1,
          timeline: [{ date: '2026-08-22', input_tokens: 40, output_tokens: 10, total_tokens: 50, calls: 1 }],
        },
      ],
      by_provider: [],
      by_model: [],
      by_agent: [],
      filters: { groups: [], providers: [], models: [], agents: [] },
    },
    error: null,
    isFetching: false,
    isLoading: false,
    refetch: vi.fn(),
  }),
}))

async function renderApp(entry = '/groups/group-1') {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS },
    interpolation: { escapeValue: false },
  })

  const router = createMemoryRouter(
    [{ path: '*', element: <AppLayout /> }],
    { initialEntries: [entry] },
  )

  const view = render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={new QueryClient()}>
        <TooltipProvider>
          <RouterProvider router={router} />
        </TooltipProvider>
      </QueryClientProvider>
    </I18nextProvider>,
  )
  return { ...view, router }
}

describe('settings overlay keeps the conversation mounted', () => {
  beforeEach(() => {
    state.conversationUnmounts = 0
  })

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
  })

  it('floats settings over the conversation without unloading it', async () => {
    const { router } = await renderApp()

    const draft = screen.getByRole('textbox', { name: 'Draft' })
    fireEvent.change(draft, { target: { value: 'unsaved reply' } })

    fireEvent.click(screen.getByRole('link', { name: 'Open settings' }))

    // The overlay panel appears and is an accessible modal dialog.
    const dialog = await screen.findByRole('dialog', { name: 'Settings' })
    expect(dialog).toBeInTheDocument()
    expect(await screen.findByText('System settings content')).toBeInTheDocument()

    // The conversation is still there behind it, with its draft untouched, and
    // never went through a teardown.
    expect(state.conversationUnmounts).toBe(0)
    expect(screen.getByRole('textbox', { name: 'Draft' })).toHaveValue('unsaved reply')

    // Background is inert while the overlay is open.
    expect(screen.getByRole('textbox', { name: 'Draft' }).closest('[inert]')).not.toBeNull()
    expect(router.state.location.pathname).toBe('/settings/system')
  })

  it('closes back to the original conversation, still mounted', async () => {
    const { router } = await renderApp()

    fireEvent.change(screen.getByRole('textbox', { name: 'Draft' }), {
      target: { value: 'still typing' },
    })
    fireEvent.click(screen.getByRole('link', { name: 'Open settings' }))
    await screen.findByRole('dialog', { name: 'Settings' })

    fireEvent.click(screen.getByRole('button', { name: 'Back to chat' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(router.state.location.pathname).toBe('/groups/group-1')
    expect(state.conversationUnmounts).toBe(0)
    expect(screen.getByRole('textbox', { name: 'Draft' })).toHaveValue('still typing')
  })

  it('closes token usage back to the original conversation', async () => {
    const { router } = await renderApp()

    fireEvent.click(screen.getByRole('link', { name: 'Open usage' }))
    const dialog = await screen.findByRole('dialog', { name: 'Token usage' })
    // The library shell is eager and the report behind it is lazy, so the panel
    // is on screen with its rail and its back button before the chart arrives.
    // The extra Suspense boundary is a real await, and under a loaded full-suite
    // run it outlasts the 1s default — hence the explicit budget.
    const trend = (
      await within(dialog).findByRole(
        'heading',
        { name: 'Daily usage by owner' },
        { timeout: 5000 },
      )
    ).closest('section')!

    expect(within(trend).getAllByRole('tab')).toHaveLength(4)
    expect(within(trend).getByRole('img', { name: 'Daily token usage by Group' }).querySelectorAll('path')).toHaveLength(2)

    fireEvent.click(await screen.findByRole('button', { name: 'Back to chat' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(router.state.location.pathname).toBe('/groups/group-1')
    expect(state.conversationUnmounts).toBe(0)
  })

  it('closes on Escape and hands focus back to the conversation', async () => {
    await renderApp()

    const opener = screen.getByRole('link', { name: 'Open settings' })
    opener.focus()
    fireEvent.click(opener)
    const dialog = await screen.findByRole('dialog', { name: 'Settings' })
    expect(dialog).toHaveFocus()

    fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true })
    const lastItem = screen.getByRole('link', { name: 'Assistant actions' })
    expect(lastItem).toHaveFocus()
    fireEvent.keyDown(lastItem, { key: 'Tab' })
    const back = screen.getByRole('button', { name: 'Back to chat' })
    expect(back).toHaveFocus()

    fireEvent.keyDown(back, { key: 'Escape' })

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(opener).toHaveFocus()
    expect(state.conversationUnmounts).toBe(0)
  })

  it('keeps the conversation mounted across state-less navigation inside the overlay', async () => {
    const { router } = await renderApp()

    fireEvent.change(screen.getByRole('textbox', { name: 'Draft' }), {
      target: { value: 'keep this draft' },
    })
    fireEvent.click(screen.getByRole('link', { name: 'Open settings' }))
    await screen.findByText('System settings content')

    fireEvent.click(screen.getByRole('link', { name: 'Media' }))

    expect(await screen.findByText('Media settings content')).toBeInTheDocument()
    expect(router.state.location.pathname).toBe('/settings/media')
    expect(router.state.location.state).toBeNull()
    expect(screen.getByRole('textbox', { name: 'Draft' })).toHaveValue('keep this draft')
    expect(state.conversationUnmounts).toBe(0)
  })

  it('renders full-page when the overlay URL is loaded cold', async () => {
    const { router } = await renderApp('/settings/system')

    expect(await screen.findByText('System settings content')).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(state.conversationUnmounts).toBe(0)

    fireEvent.click(screen.getByRole('button', { name: 'Back to chat' }))
    await waitFor(() => expect(router.state.location.pathname).toBe('/'))
  })
})

describe('overlay route metadata', () => {
  afterEach(cleanup)

  const route = (pathname: string, state: unknown = null): Location => ({
    pathname,
    search: '',
    hash: '',
    state,
    key: pathname,
    unstable_mask: undefined,
  })

  it('recognizes every overlay area without matching lookalike paths', () => {
    const overlays = [
      ['/settings/system', 'navigation:settings'],
      ['/agents/new', 'navigation:agents'],
      ['/providers/provider-1', 'navigation:providers'],
      ['/mcp-servers/server-1', 'navigation:mcpServers'],
      ['/skills/skill-1', 'navigation:skills'],
      ['/workspaces/workspace-1', 'navigation:workspaces'],
      ['/usage', 'navigation:usage'],
      ['/groups/group-1/manage', 'groups:manage.title'],
    ] as const

    for (const [pathname, label] of overlays) {
      expect(isOverlayPath(pathname)).toBe(true)
      expect(overlayAreaLabelKey(pathname)).toBe(label)
    }
    expect(isOverlayPath('/settings-old')).toBe(false)
    expect(isOverlayPath('/groups/group-1/manage/members')).toBe(false)
    expect(isGroupManagePath('/groups/group-1/manage')).toBe(true)
    expect(isGroupManagePath('/settings/system')).toBe(false)
  })

  it('carries the original stage, but does not invent one for a cold deep link', () => {
    const stage = route('/groups/group-1')

    expect(overlayLinkState(stage)).toEqual({ backgroundLocation: stage })
    expect(overlayLinkState(route('/settings/system'))).toBeUndefined()
    expect(
      overlayLinkState(route('/settings/media', { backgroundLocation: stage })),
    ).toEqual({ backgroundLocation: stage })
  })

  it('preserves the stage through index and legacy-detail redirects', async () => {
    const stage = route('/groups/group-1')
    const redirects = [
      {
        source: '/settings',
        sourceRoute: '/settings',
        target: '/settings/system',
        element: <OverlayRedirect to="/settings/system" />,
      },
      {
        source: '/settings/agents/agent-1',
        sourceRoute: '/settings/agents/:id',
        target: '/agents/agent-1',
        element: <LegacyDetailRedirect base="/agents" />,
      },
    ]

    for (const redirect of redirects) {
      const router = createMemoryRouter(
        [
          { path: redirect.sourceRoute, element: redirect.element },
          { path: redirect.target, element: <div>Destination</div> },
        ],
        {
          initialEntries: [{
            pathname: redirect.source,
            state: { backgroundLocation: stage },
          }],
        },
      )
      const view = render(<RouterProvider router={router} />)

      await waitFor(() => expect(router.state.location.pathname).toBe(redirect.target))
      expect(router.state.location.state).toEqual({ backgroundLocation: stage })
      view.unmount()
    }
  })
})
