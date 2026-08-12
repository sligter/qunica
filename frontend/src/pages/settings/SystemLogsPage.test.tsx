import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { SystemLogsPage } from '@/pages/settings/SystemLogsPage'

const desktop = vi.hoisted(() => ({
  getSystemLogs: vi.fn(),
  clearSystemLogs: vi.fn(),
}))

vi.mock('@/lib/desktop', () => ({
  isDesktopRuntime: () => true,
  getSystemLogs: desktop.getSystemLogs,
  clearSystemLogs: desktop.clearSystemLogs,
  setSystemLogFilter: vi.fn(),
  openSystemLogsFolder: vi.fn(),
}))

async function renderPage() {
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
        client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
      >
        <SystemLogsPage />
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('SystemLogsPage', () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  it('shows, pauses, searches, and clears recent logs', async () => {
    desktop.getSystemLogs.mockResolvedValue({
      filter: 'info',
      log_dir: 'C:/AG Swarmer/logs',
      entries: [
        {
          timestamp: '2026-07-29T12:00:00Z',
          level: 'INFO',
          target: 'ag_swarmer_backend::server',
          message: 'backend ready',
          fields: { message: 'backend ready', port: 8765 },
        },
      ],
    })
    desktop.clearSystemLogs.mockResolvedValue(undefined)
    await renderPage()

    expect(await screen.findByText('backend ready')).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'Pause' }))
    expect(screen.getByRole('button', { name: 'Resume' })).toBeVisible()

    fireEvent.change(screen.getByRole('textbox', { name: 'Search logs' }), {
      target: { value: 'missing' },
    })
    expect(screen.getByText('No logs match the current filters.')).toBeVisible()

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    await waitFor(() => expect(desktop.clearSystemLogs).toHaveBeenCalledOnce())
  })

  it('keeps background log loading from flashing the refresh button', async () => {
    desktop.getSystemLogs.mockReturnValue(new Promise(() => undefined))
    await renderPage()

    const refresh = screen.getByRole('button', { name: 'Refresh' })
    expect(refresh).toBeEnabled()
    expect(refresh.querySelector('svg')).not.toHaveClass('animate-spin')
  })
})
