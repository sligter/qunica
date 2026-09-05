import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))

afterEach(() => { vi.unstubAllEnvs(); vi.resetModules(); mocks.invoke.mockReset(); localStorage.clear() })

describe('Android remote session', () => {
  it('accepts exact HTTPS origins and rejects insecure or ambiguous server addresses', async () => {
    const { normalizeServerOrigin } = await import('./androidSession')
    expect(normalizeServerOrigin(' https://PHONE.example:443/ ')).toBe('https://phone.example')
    expect(normalizeServerOrigin('https://phone.example:8443')).toBe('https://phone.example:8443')
    for (const value of ['http://phone.example', 'https://user:pass@phone.example', 'https://phone.example/api', 'https://phone.example?token=x', 'https://phone.example#x', 'javascript:alert(1)', 'https://tauri.localhost']) {
      expect(() => normalizeServerOrigin(value)).toThrow()
    }
  })

  it('restores credentials from the native vault and never treats Android as desktop', async () => {
    vi.stubEnv('MODE', 'android')
    vi.stubEnv('VITE_API_BASE_URL', 'http://127.0.0.1:8765')
    localStorage.setItem('qunica:auth:v1', JSON.stringify({ token: 'browser-token' }))
    mocks.invoke.mockResolvedValue({ value: JSON.stringify({ server: 'https://phone.example', token: 'secure-token' }) })
    const session = await import('./androidSession')
    await expect(session.initializeAndroidSession()).resolves.toBe('secure-token')
    expect(localStorage.getItem('qunica:auth:v1')).toBeNull()
    const runtime = await import('./runtime')
    expect(runtime.isDesktopRuntime()).toBe(false)
    expect(runtime.apiUrl('/api/v2/health')).toBe('https://phone.example/api/v2/health')
  })

  it('serializes login, sign-out and server replacement without carrying a token across servers', async () => {
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://first.example', token: null }) })
    const session = await import('./androidSession')
    await session.initializeAndroidSession()
    let complete!: () => void
    mocks.invoke.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve }))
    mocks.invoke.mockResolvedValue({})
    const login = session.saveAndroidToken('secret')
    const logout = session.saveAndroidToken(null)
    const change = session.changeAndroidServer('https://second.example')
    await Promise.resolve()
    expect(mocks.invoke).toHaveBeenCalledTimes(2)
    complete()
    await Promise.all([login, logout, change])
    expect(mocks.invoke.mock.calls.slice(1).map(([, args]) => JSON.parse(args.value))).toEqual([
      { server: 'https://first.example', token: 'secret' },
      { server: 'https://first.example', token: null },
      { server: 'https://second.example', token: null },
    ])
    expect(session.useAndroidSession.getState().server).toBe('https://second.example')
    expect(localStorage.getItem('qunica:auth:v1')).toBeNull()
  })

  it('fails closed when the native vault cannot be read', async () => {
    localStorage.setItem('qunica:auth:v1', JSON.stringify({ token: 'unsafe-fallback' }))
    mocks.invoke.mockRejectedValue(new Error('Keystore unavailable'))
    const session = await import('./androidSession')
    await expect(session.initializeAndroidSession()).rejects.toThrow('Keystore unavailable')
    expect(session.useAndroidSession.getState()).toMatchObject({ ready: false, server: null })
  })

  it('does not restore an in-flight login after the user signs out', async () => {
    vi.stubEnv('MODE', 'android')
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://phone.example', token: null }) })
    const session = await import('./androidSession')
    await session.initializeAndroidSession()
    const { useAuthStore } = await import('@/stores/authStore')
    let complete!: () => void
    mocks.invoke.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve }))
    mocks.invoke.mockResolvedValue({})
    const login = useAuthStore.getState().setToken('pending-token')
    await Promise.resolve()
    useAuthStore.getState().logout()
    complete()
    await login
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(3))
    expect(useAuthStore.getState().token).toBeNull()
    expect(JSON.parse(mocks.invoke.mock.calls[2][1].value).token).toBeNull()
  })

  it('surfaces persistence errors and permits retry without browser credential storage', async () => {
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://phone.example', token: null }) })
    const session = await import('./androidSession')
    await session.initializeAndroidSession()
    mocks.invoke.mockRejectedValueOnce(new Error('disk unavailable'))
    await expect(session.saveAndroidToken('secret')).rejects.toThrow('disk unavailable')
    expect(session.useAndroidSession.getState().error).toContain('disk unavailable')
    mocks.invoke.mockResolvedValue({})
    await session.retryAndroidPersistence()
    expect(session.useAndroidSession.getState().error).toBeNull()
    expect(localStorage.getItem('qunica:auth:v1')).toBeNull()
  })
})
