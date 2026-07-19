import { cleanup, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import { AppLayout } from '@/components/layout/AppLayout'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'

async function renderAppLayout(language: 'en-US' | 'zh-CN' = 'en-US') {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: language,
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })

  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <Routes>
            <Route element={<AppLayout />}>
              <Route index element={<div>Application content</div>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('AppLayout', () => {
  afterEach(cleanup)

  it('prevents the native context menu anywhere in the application surface', async () => {
    const { container } = await renderAppLayout()

    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    expect(container.firstElementChild?.dispatchEvent(event)).toBe(false)
    expect(event.defaultPrevented).toBe(true)
  })

  it('renders English navigation labels', async () => {
    await renderAppLayout('en-US')

    const directChats = screen.getByText('Chats')
    const groups = screen.getByText('Groups')
    expect(
      Boolean(directChats.compareDocumentPosition(groups) & Node.DOCUMENT_POSITION_FOLLOWING),
    ).toBe(true)
    expect(screen.getByText('Groups')).toBeInTheDocument()
    expect(screen.getByText('Agents')).toBeInTheDocument()
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders Chinese navigation labels', async () => {
    await renderAppLayout('zh-CN')

    expect(screen.getByText('私聊')).toBeInTheDocument()
    expect(screen.getByText('群组')).toBeInTheDocument()
    expect(screen.getByText('Agent')).toBeInTheDocument()
    expect(screen.getByText('设置')).toBeInTheDocument()
  })
})
