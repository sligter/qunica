import { act, cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, expect, it, vi } from 'vitest'
import { LoginPage } from './LoginPage'
import { useAuthStore } from '@/stores/authStore'
import '@/i18n'

vi.mock('@/hooks/useAuthConfig', () => ({ useAuthConfig: () => ({ data: { registration_enabled: false } }) }))
vi.mock('@/components/auth/AuthForm', () => ({ AuthForm: () => <p>Manual login form</p> }))
afterEach(() => { cleanup(); useAuthStore.setState({ token: null, user: null, hydrated: false }) })

it('leaves the login page when the paired account is restored', () => {
  useAuthStore.setState({ token: null })
  render(<MemoryRouter initialEntries={['/login']}><Routes>
    <Route path="/login" element={<LoginPage />} />
    <Route path="/" element={<p>Workspace route</p>} />
  </Routes></MemoryRouter>)
  expect(screen.getByText('Manual login form')).toBeInTheDocument()
  act(() => useAuthStore.setState({ token: 'restored-pairing-token' }))
  expect(screen.getByText('Workspace route')).toBeInTheDocument()
  expect(screen.queryByText('Manual login form')).toBeNull()
})
