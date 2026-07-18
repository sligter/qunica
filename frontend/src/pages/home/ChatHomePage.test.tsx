import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, waitFor } from '@testing-library/react'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { ChatHomePage } from '@/pages/home/ChatHomePage'
import { useAuthStore } from '@/stores/authStore'

describe('ChatHomePage document title', () => {
  afterEach(() => {
    cleanup()
    document.title = ''
    useAuthStore.setState({ token: null, user: null, hydrated: false })
  })

  it('sets the title through the semantic home resource', async () => {
    const i18n = i18next.createInstance()
    await i18n.use(initReactI18next).init({
      lng: 'en-US',
      resources: {
        'en-US': {
          ...enUS,
          groups: { ...enUS.groups, pageTitle: 'Localized Home Title' },
        },
      },
      interpolation: { escapeValue: false },
    })

    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={new QueryClient()}>
          <MemoryRouter>
            <ChatHomePage />
          </MemoryRouter>
        </QueryClientProvider>
      </I18nextProvider>,
    )

    await waitFor(() => expect(document.title).toBe('Localized Home Title'))
  })
})
