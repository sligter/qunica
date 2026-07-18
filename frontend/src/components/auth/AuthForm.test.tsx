import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AuthForm } from '@/components/auth/AuthForm'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { useAuthStore } from '@/stores/authStore'

function errorResponse(status: number, code: string, message: string) {
  return new Response(JSON.stringify({ error: { code, message } }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

async function renderChineseAuthForm(mode: 'login' | 'register') {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'zh-CN',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })

  render(
    <I18nextProvider i18n={i18n}>
      <AuthForm mode={mode} onSuccess={vi.fn()} />
    </I18nextProvider>,
  )
}

describe('AuthForm localized server diagnostics', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    localStorage.clear()
  })

  it('localizes a wrong-password response in Chinese', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn<typeof fetch>().mockResolvedValue(
        errorResponse(403, 'permission_denied', 'invalid credentials'),
      ),
    )
    const user = userEvent.setup()
    await renderChineseAuthForm('login')

    await user.type(screen.getByLabelText('邮箱'), 'user@example.com')
    await user.type(screen.getByLabelText('密码'), 'wrong-password')
    await user.click(screen.getByRole('button', { name: '登录' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('邮箱或密码不正确。')
    expect(screen.getByRole('alert')).not.toHaveTextContent('invalid credentials')
  })

  it('localizes a duplicate-registration response in Chinese', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn<typeof fetch>().mockResolvedValue(
        errorResponse(409, 'conflict', 'user already exists'),
      ),
    )
    const user = userEvent.setup()
    await renderChineseAuthForm('register')

    await user.type(screen.getByLabelText('姓名'), 'Existing User')
    await user.type(screen.getByLabelText('邮箱'), 'user@example.com')
    await user.type(screen.getByLabelText('密码'), 'password123')
    await user.click(screen.getByRole('button', { name: '注册' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('该邮箱已注册。')
    expect(screen.getByRole('alert')).not.toHaveTextContent('user already exists')
  })
})
