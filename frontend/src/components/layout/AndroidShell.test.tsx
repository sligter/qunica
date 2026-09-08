import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'
import { AndroidShell } from './AndroidShell'
import { useAndroidSession } from '@/lib/androidSession'
import { useAuthStore } from '@/stores/authStore'
import '@/i18n'

const mocks = vi.hoisted(() => ({ initialize: vi.fn() }))
vi.mock('@/lib/androidSession', async (original) => ({
  ...await original<typeof import('@/lib/androidSession')>(),
  initializeAndroidSession: mocks.initialize,
}))
afterEach(() => {
  cleanup(); mocks.initialize.mockReset()
  useAuthStore.setState({ token: null, user: null, hydrated: false })
  useAndroidSession.setState({ server: null, ready: false, error: null })
})

it('keeps routes unmounted until the restored account token is installed', async () => {
  let complete!: (token: string) => void
  mocks.initialize.mockReturnValue(new Promise<string>(resolve => { complete = resolve }))
  const mountedTokens: (string | null)[] = []
  function Routes() {
    const token = useAuthStore(s => s.token)
    mountedTokens.push(token)
    return <div>Authenticated workspace</div>
  }
  render(<AndroidShell><Routes /></AndroidShell>)
  // Vault restoration publishes its origin before its promise resolves.
  act(() => useAndroidSession.setState({ ready: true, server: 'https://qunica-lan.invalid' }))
  expect(screen.queryByText('Authenticated workspace')).toBeNull()
  // Neither the workspace nor a second pairing form is offered while restoring.
  expect(screen.queryAllByRole('button')).toHaveLength(0)
  expect(screen.getByRole('status')).toBeInTheDocument()
  await act(async () => { complete('paired-account-token') })
  expect(screen.getByText('Authenticated workspace')).toBeInTheDocument()
  expect(mountedTokens).not.toContain(null)
  expect(useAuthStore.getState().token).toBe('paired-account-token')
})
