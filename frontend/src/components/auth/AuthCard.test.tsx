import { cleanup, render, screen } from '@testing-library/react'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'

async function renderAuthPage(page: 'login' | 'register', language: 'en-US' | 'zh-CN') {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: language,
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })

  render(
    <I18nextProvider i18n={i18n}>
      <MemoryRouter>{page === 'login' ? <LoginPage /> : <RegisterPage />}</MemoryRouter>
    </I18nextProvider>,
  )
}

describe('AuthCard', () => {
  afterEach(cleanup)

  it('introduces the product beside the sign-in form', async () => {
    await renderAuthPage('login', 'en-US')

    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('AG Swarmer')
    expect(screen.getByText('One room. Many agents.')).toBeInTheDocument()
    expect(screen.getByText(/group-first workbench/)).toBeInTheDocument()
    expect(screen.getByText('Groups, not chat windows')).toBeInTheDocument()
    expect(screen.getByText('Bound workspaces')).toBeInTheDocument()
    expect(screen.getByText('Nothing lands unapproved')).toBeInTheDocument()
    expect(screen.getByText('Pick your project rooms back up.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeInTheDocument()
  })

  it('localizes the pitch, so the two locales stay in step', async () => {
    await renderAuthPage('login', 'zh-CN')

    expect(screen.getByText('一个房间，多个 Agent。')).toBeInTheDocument()
    expect(screen.getByText('群组，而不是聊天窗口')).toBeInTheDocument()
    expect(screen.getByText('回到你的项目房间继续。')).toBeInTheDocument()
  })

  it('gives register the same surface with its own subtitle', async () => {
    await renderAuthPage('register', 'en-US')

    expect(screen.getByText('One room. Many agents.')).toBeInTheDocument()
    expect(
      screen.getByText('Set up an account and open your first project room.'),
    ).toBeInTheDocument()
  })
})
