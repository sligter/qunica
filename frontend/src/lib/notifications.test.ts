import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { showNotification } from '@/lib/notifications'

const mocks = vi.hoisted(() => ({ desktop: false, invoke: vi.fn() }))

vi.mock('@/lib/runtime', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/runtime')>()
  return { ...original, isDesktopRuntime: () => mocks.desktop }
})

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))

class NotificationStub {
  static permission: NotificationPermission = 'default'
  static requestPermission = vi.fn(async () => NotificationStub.permission)
  static instances: Array<{ title: string; body?: string }> = []

  constructor(title: string, options?: NotificationOptions) {
    NotificationStub.instances.push({ title, body: options?.body })
  }
}

function installWebNotification(permission: NotificationPermission) {
  NotificationStub.permission = permission
  NotificationStub.instances = []
  NotificationStub.requestPermission.mockClear()
  vi.stubGlobal('Notification', NotificationStub)
}

describe('showNotification', () => {
  beforeEach(() => {
    mocks.desktop = false
    mocks.invoke.mockReset().mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('asks the browser for permission the first time rather than giving up', async () => {
    installWebNotification('default')
    NotificationStub.requestPermission.mockImplementation(async () => {
      NotificationStub.permission = 'granted'
      return 'granted' as NotificationPermission
    })

    const result = await showNotification('Platform', 'The reply is ready.')

    expect(NotificationStub.requestPermission).toHaveBeenCalledTimes(1)
    expect(result).toEqual({ ok: true })
    expect(NotificationStub.instances).toEqual([
      { title: 'Platform', body: 'The reply is ready.' },
    ])
  })

  it('reports a refusal instead of failing silently', async () => {
    installWebNotification('denied')

    const result = await showNotification('Platform', 'The reply is ready.')

    expect(result).toEqual({ ok: false, error: 'denied' })
    expect(NotificationStub.instances).toEqual([])
  })

  it('hands the toast to the desktop shell', async () => {
    mocks.desktop = true

    const result = await showNotification('Platform', 'The reply is ready.')

    expect(mocks.invoke).toHaveBeenCalledWith('show_notification', {
      title: 'Platform',
      body: 'The reply is ready.',
    })
    expect(result).toEqual({ ok: true })
  })

  it('surfaces why the desktop shell could not raise a toast', async () => {
    mocks.desktop = true
    mocks.invoke.mockRejectedValue(new Error('element not found'))

    const result = await showNotification('Platform', 'The reply is ready.')

    expect(result).toEqual({ ok: false, error: 'element not found' })
  })
})
