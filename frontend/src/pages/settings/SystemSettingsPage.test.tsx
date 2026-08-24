import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage'
import { useAuthStore } from '@/stores/authStore'
import type { SystemSettingsRead } from '@/types/api'

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: tauri.listen }))

const settings: SystemSettingsRead = {
  id: 'settings-1',
  owner_id: 'user-1',
  appearance: 'system',
  language: 'en-US',
  reply_insert_mode: 'instant',
  assistant_enabled: true,
  assistant_auto_approve: false,
  group_workspace_root: null,
  shell_preference: 'auto',
  web_search_provider: 'tavily',
  tavily_api_key_configured: false,
  tavily_search_url: 'https://api.tavily.com/search',
  tavily_max_results: 5,
  tavily_search_depth: 'basic',
  tavily_include_answer: true,
  tavily_include_raw_content: false,
  media_base_url: 'https://api.openai.com',
  media_api_key_configured: false,
  image_generation_model: null,
  image_generation_endpoint: '/v1/images/generations',
  video_generation_model: null,
  video_generation_endpoint: '/v1/videos',
  video_status_endpoint: '/v1/videos/{id}',
  video_content_endpoint: '/v1/videos/{id}/content',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
}

function jsonResponse(body: unknown, init?: ResponseInit) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
}

async function renderSettingsPage() {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })

  render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
          })
        }
      >
        <SystemSettingsPage />
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('SystemSettingsPage preferences', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    localStorage.clear()
  })

  it('changes language optimistically and rolls back after a failed save', async () => {
    useAuthStore.setState({ token: 'token' })
    let rejectPatch!: (reason?: unknown) => void
    const patchResponse = new Promise<Response>((_resolve, reject) => {
      rejectPatch = reject
    })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockReturnValueOnce(patchResponse)
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()

    expect(await screen.findByRole('heading', { name: 'System settings' })).toBeVisible()
    const chinese = screen.getByRole('radio', { name: '中文' })
    await waitFor(() => expect(chinese).toBeEnabled())
    await user.click(chinese)

    expect(screen.getByRole('heading', { name: '系统设置' })).toBeVisible()
    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ language: 'zh-CN' })
    rejectPatch(new Error('offline'))
    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'English' })).toHaveAttribute(
        'aria-checked',
        'true',
      )
    })
    expect(screen.getByRole('alert')).toHaveTextContent('Language update failed.')
  })

  it('saves the current user avatar and updates auth state', async () => {
    const currentUser = {
      id: 'user-1',
      email: 'me@example.com',
      name: 'Nova Ray',
      avatar_url: null,
      created_at: '2026-01-01T00:00:00Z',
    }
    useAuthStore.setState({ token: 'token', user: currentUser })
    let resolvePatch!: (response: Response) => void
    const patchResponse = new Promise<Response>((resolve) => {
      resolvePatch = resolve
    })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockReturnValueOnce(patchResponse)
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()
    await user.click(await screen.findByRole('button', { name: 'Prism' }))
    expect(useAuthStore.getState().user?.avatar_url).toBe('preset:prism')

    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ avatar_url: 'preset:prism' })
    resolvePatch(jsonResponse({ ...currentUser, avatar_url: 'preset:prism' }))
    await waitFor(() => expect(useAuthStore.getState().user?.avatar_url).toBe('preset:prism'))
  })

  it('trims and saves the current user nickname', async () => {
    const currentUser = {
      id: 'user-1',
      email: 'me@example.com',
      name: 'Nova',
      avatar_url: null,
      created_at: '2026-01-01T00:00:00Z',
    }
    useAuthStore.setState({ token: 'token', user: currentUser })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockResolvedValueOnce(jsonResponse({ ...currentUser, name: 'Nova Ray' }))
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()
    const nickname = await screen.findByLabelText('Nickname')
    await user.clear(nickname)
    await user.type(nickname, '  Nova Ray  ')
    await user.click(nickname.closest('form')!.querySelector('button')!)

    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(JSON.parse(String(patchInit.body))).toEqual({ name: 'Nova Ray' })
    await waitFor(() => expect(useAuthStore.getState().user?.name).toBe('Nova Ray'))
  })

  it('saves whether the assistant launcher is enabled', async () => {
    useAuthStore.setState({ token: 'token' })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockResolvedValueOnce(jsonResponse({ ...settings, assistant_enabled: false }))
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()

    const toggle = await screen.findByRole('switch', { name: 'Enable assistant' })
    await waitFor(() => expect(toggle).toBeEnabled())
    expect(toggle).toBeChecked()
    await user.click(toggle)

    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ assistant_enabled: false })
    await waitFor(() => expect(toggle).not.toBeChecked())
  })

  it('saves the integrated terminal shell and rolls back after a failed save', async () => {
    useAuthStore.setState({ token: 'token' })
    let rejectPatch!: (reason?: unknown) => void
    const patchResponse = new Promise<Response>((_resolve, reject) => {
      rejectPatch = reject
    })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockReturnValueOnce(patchResponse)
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()

    const select = await screen.findByLabelText('Integrated terminal shell')
    await waitFor(() => expect(select).toBeEnabled())
    expect(select).toHaveValue('auto')
    await user.selectOptions(select, 'bash')

    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ shell_preference: 'bash' })

    // A failed save must not leave the UI claiming a shell the backend never
    // stored: the next terminal would start under the old one.
    rejectPatch(new Error('offline'))
    await waitFor(() => expect(select).toHaveValue('auto'))
    expect(screen.getByRole('alert')).toBeVisible()
  })
})

describe('SystemSettingsPage about and updates', () => {
  const about = {
    name: 'AG Swarmer',
    version: '0.1.1-alpha',
    identifier: 'ag-swarmer.desktop',
    tauri_version: '2.11.2',
    os: 'windows',
    arch: 'x86_64',
  }

  const release = {
    version: '0.2.0',
    current_version: '0.1.1-alpha',
    notes: 'Faster startup.',
    pub_date: '2026-08-20T10:00:00Z',
    target: 'windows-x86_64',
  }

  function enterDesktop() {
    useAuthStore.setState({ token: 'token' })
    vi.stubGlobal('__TAURI_INTERNALS__', {})
    vi.stubGlobal('fetch', vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(settings)))
  }

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    vi.clearAllMocks()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    localStorage.clear()
  })

  it('names the running build and reports it as current', async () => {
    enterDesktop()
    tauri.invoke.mockImplementation((command: string) =>
      command === 'app_about'
        ? Promise.resolve(about)
        : command === 'check_for_update'
          ? Promise.resolve(null)
          : Promise.reject(new Error(`unexpected command ${command}`)),
    )
    const user = userEvent.setup()

    await renderSettingsPage()

    expect(await screen.findByText('0.1.1-alpha')).toBeVisible()
    expect(screen.getByText('windows · x86_64')).toBeVisible()
    expect(screen.getByText('ag-swarmer.desktop')).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Check for updates' }))
    expect(await screen.findByText('You are on the latest version.')).toBeVisible()
  })

  it('offers the package built for this device and installs it', async () => {
    enterDesktop()
    const unlisten = vi.fn()
    tauri.listen.mockResolvedValue(unlisten)
    let rejectInstall!: (reason: unknown) => void
    tauri.invoke.mockImplementation((command: string) => {
      if (command === 'app_about') return Promise.resolve(about)
      if (command === 'check_for_update') return Promise.resolve(release)
      if (command === 'install_update') {
        return new Promise((_resolve, reject) => {
          rejectInstall = reject
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })
    const user = userEvent.setup()

    await renderSettingsPage()
    await user.click(await screen.findByRole('button', { name: 'Check for updates' }))

    expect(await screen.findByText('Version 0.2.0 is available.')).toBeVisible()
    // The manifest is keyed by target, so the row has to name the artifact this
    // machine gets rather than promising a generic "update".
    expect(screen.getByText('Package for this device: windows-x86_64')).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Download and install' }))
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('install_update'))

    // A successful install never resolves — it replaces this process. A failed
    // one must release the progress UI and say why, or the user is stranded on
    // "Installing…" with no way to retry.
    rejectInstall('signature mismatch')
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Update install failed: signature mismatch',
    )
    expect(unlisten).toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Download and install' })).toBeEnabled()
  })

  it('sends browser tabs to the desktop app instead of offering an installer', async () => {
    useAuthStore.setState({ token: 'token' })
    vi.stubGlobal('fetch', vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(settings)))

    await renderSettingsPage()

    expect(
      await screen.findByText(
        'Updates are handled by the desktop app. A browser tab loads the latest version on reload.',
      ),
    ).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Check for updates' })).toBeNull()
    expect(tauri.invoke).not.toHaveBeenCalled()
  })
})
