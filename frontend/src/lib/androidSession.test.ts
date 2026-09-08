import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))

afterEach(() => { vi.unstubAllEnvs(); vi.resetModules(); mocks.invoke.mockReset(); localStorage.clear() })

describe('Android remote session', () => {
  it('saves a paired desktop login directly in the vault and separates it from device identity', async () => {
    const lan = { endpoint: '192.168.1.4:8766', publicKey: 'pinned-key', credential: 'device-secret' }
    mocks.invoke.mockResolvedValueOnce({ ...lan, accountToken: 'desktop-account-token' }).mockResolvedValue({})
    const session = await import('./androidSession')
    await expect(session.pairAndroidDesktop('qunica://pair?data=once')).resolves.toBe('desktop-account-token')
    const saved = JSON.parse(mocks.invoke.mock.calls[1][1].value)
    expect(saved).toEqual({ server: session.LAN_ORIGIN, token: 'desktop-account-token', lan })
    expect(mocks.invoke).toHaveBeenLastCalledWith('mobile_lan_configure', { connection: lan })
    expect(localStorage.length).toBe(0)
    await session.saveAndroidToken(null)
    expect(JSON.parse(mocks.invoke.mock.calls.at(-1)![1].value)).toMatchObject({ token: null, lan })
  })
  it('retains manual login for older desktops and does not configure a pairing after vault failure', async () => {
    const lan = { endpoint: '192.168.1.4:8766', publicKey: 'pinned-key', credential: 'device-secret' }
    mocks.invoke.mockResolvedValueOnce(lan).mockResolvedValue({})
    const session = await import('./androidSession')
    await expect(session.pairAndroidDesktop('old-offer')).resolves.toBeNull()
    mocks.invoke.mockClear()
    mocks.invoke.mockResolvedValueOnce({ ...lan, accountToken: 'new-token' }).mockRejectedValueOnce(new Error('Vault failed'))
    await expect(session.pairAndroidDesktop('new-offer')).rejects.toThrow('Vault failed')
    expect(mocks.invoke).not.toHaveBeenCalledWith('mobile_lan_configure', expect.anything())
  })
  it('verifies a changed route against the original key and keeps the account token', async () => {
    const lan = { endpoint: '192.168.1.4:8766', publicKey: 'pinned-key', credential: 'device-secret' }
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://qunica-lan.invalid', token: 'account-token', lan }) }).mockResolvedValue({})
    const session = await import('./androidSession')
    await session.initializeAndroidSession()
    await session.changeAndroidDesktopEndpoint(' relay.example.com:18766 ')
    const changed = { ...lan, endpoint: 'relay.example.com:18766' }
    expect(mocks.invoke.mock.calls.slice(2).map(([name]) => name)).toEqual(['mobile_lan_verify', 'mobile_session_write', 'mobile_lan_configure'])
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_lan_verify', { connection: changed })
    expect(JSON.parse(mocks.invoke.mock.calls[3][1].value)).toMatchObject({ token: 'account-token', lan: changed })
    expect(session.androidDesktopAddress()).toBe(changed.endpoint)
  })
  it('keeps the old route when verification or secure storage fails', async () => {
    const lan = { endpoint: '192.168.1.4:8766', publicKey: 'pinned-key', credential: 'device-secret' }
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://qunica-lan.invalid', token: null, lan }) }).mockResolvedValue({})
    const session = await import('./androidSession')
    await session.initializeAndroidSession()
    mocks.invoke.mockRejectedValueOnce(new Error('Wrong desktop key'))
    await expect(session.changeAndroidDesktopEndpoint('relay.example.com:18766')).rejects.toThrow('Wrong desktop key')
    expect(session.androidDesktopAddress()).toBe(lan.endpoint)
    expect(mocks.invoke).not.toHaveBeenCalledWith('mobile_session_write', expect.anything())
    mocks.invoke.mockResolvedValueOnce({}).mockRejectedValueOnce(new Error('Disk full'))
    await expect(session.changeAndroidDesktopEndpoint('relay.example.com:18766')).rejects.toThrow('Disk full')
    expect(session.androidDesktopAddress()).toBe(lan.endpoint)
  })
  it('restores the paired identity through native validation and keeps all secrets out of browser storage', async () => {
    const lan = { endpoint: '192.168.1.4:8766', publicKey: 'pinned-key', credential: 'device-secret' }
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://qunica-lan.invalid', token: 'account-token', lan }) }).mockResolvedValue({})
    const session = await import('./androidSession')
    await expect(session.initializeAndroidSession()).resolves.toBe('account-token')
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_lan_configure', { connection: lan })
    expect(session.androidDesktopAddress()).toBe('192.168.1.4:8766')
    await session.saveAndroidToken(null)
    const last = mocks.invoke.mock.calls.at(-1)!
    expect(JSON.parse(last[1].value)).toMatchObject({ token: null, lan })
    expect(localStorage.length).toBe(0)
  })

  it('fails closed if the native bridge rejects a saved pairing identity', async () => {
    mocks.invoke.mockResolvedValueOnce({ value: JSON.stringify({ server: 'https://qunica-lan.invalid', token: 'token', lan: { endpoint: '8.8.8.8:8766' } }) }).mockRejectedValueOnce(new Error('Only LAN IPv4 addresses'))
    const session = await import('./androidSession')
    await expect(session.initializeAndroidSession()).rejects.toThrow('Only LAN')
    expect(session.useAndroidSession.getState()).toMatchObject({ ready: false, server: null })
  })
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
