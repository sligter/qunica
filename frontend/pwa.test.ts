import { runInNewContext } from 'node:vm'
import { describe, expect, it, vi } from 'vitest'
import { serviceWorkerSource } from './pwa'

describe('service worker static cache boundary', () => {
  it('intercepts only exact same-origin build assets without credentials or query strings', async () => {
    const listeners: Record<string, (event: unknown) => void> = {}
    const cached = new Response('static')
    const match = vi.fn().mockResolvedValue(cached)
    const fetch = vi.fn()
    runInNewContext(serviceWorkerSource(['/assets/app-123.js'], 'test'), {
      self: { location: { origin: 'https://qunica.test' }, addEventListener: (name: string, listener: (event: unknown) => void) => { listeners[name] = listener } },
      caches: { open: vi.fn().mockResolvedValue({ match }) }, URL, fetch,
    })
    for (const [url, init] of [
      ['/api/v2/messages/stream', {}],
      ['/workspace/secret.js', {}],
      ['/assets/app-123.js?token=secret', {}],
      ['https://other.test/assets/app-123.js', {}],
      ['/assets/app-123.js', { method: 'POST' }],
      ['/assets/app-123.js', { headers: { Authorization: 'Bearer secret' } }],
    ] as const) {
      const respondWith = vi.fn()
      listeners.fetch({ request: new Request(new URL(url, 'https://qunica.test'), init), respondWith })
      expect(respondWith).not.toHaveBeenCalled()
    }
    const respondWith = vi.fn()
    listeners.fetch({ request: new Request('https://qunica.test/assets/app-123.js'), respondWith })
    await expect(respondWith.mock.calls[0][0]).resolves.toBe(cached)
    expect(fetch).not.toHaveBeenCalled()
  })
})
