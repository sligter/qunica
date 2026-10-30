import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CodexLoginPanel } from '@/components/agents/CodexLoginPanel'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'

function loginStream() {
  let controller: ReadableStreamDefaultController<Uint8Array>
  const body = new ReadableStream<Uint8Array>({ start: (value) => { controller = value } })
  const encoder = new TextEncoder()
  return {
    response: new Response(body, { headers: { 'Content-Type': 'text/event-stream' } }),
    send: (value: unknown) => controller.enqueue(encoder.encode(`event: codex_login\ndata: ${JSON.stringify(value)}\n\n`)),
    close: () => controller.close(),
  }
}

const deviceCode = { type: 'device_code', verification_url: 'https://auth.openai.com/codex/device', user_code: 'TEST-1234' }

describe('CodexLoginPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    useAuthStore.setState({ token: 'test-token' })
  })

  afterEach(() => {
    cleanup()
    useAuthStore.setState({ token: null })
    vi.unstubAllGlobals()
  })

  it('checks existing authentication only when requested, with the runtime environment', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ authenticated: true, method: 'apiKey' }), { headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    const refreshed = vi.fn()
    render(<CodexLoginPanel envText="HTTPS_PROXY=http://proxy.test:8080" onAuthenticated={refreshed} />)
    expect(fetch).not.toHaveBeenCalled()
    await userEvent.click(screen.getByRole('button', { name: 'Check sign-in' }))
    expect(await screen.findByRole('status')).toHaveTextContent('authenticated with an API key')
    expect(fetch).toHaveBeenCalledWith(expect.stringContaining('/codex-auth/status'), expect.objectContaining({ method: 'POST', body: JSON.stringify({ env: { HTTPS_PROXY: 'http://proxy.test:8080' } }) }))
    expect(refreshed).toHaveBeenCalledOnce()
  })

  it('displays a device code and refreshes ACP capabilities after successful authorization', async () => {
    const user = userEvent.setup()
    const stream = loginStream()
    const fetch = vi.fn().mockResolvedValue(stream.response)
    vi.stubGlobal('fetch', fetch)
    const refreshed = vi.fn()
    render(<CodexLoginPanel envText="" onAuthenticated={refreshed} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    expect(await screen.findByLabelText('One-time device code')).toHaveTextContent('TEST-1234')
    expect(screen.getByRole('link')).toHaveAttribute('href', deviceCode.verification_url)
    await user.click(screen.getByRole('button', { name: 'Copy code' }))
    expect(await navigator.clipboard.readText()).toBe('TEST-1234')
    await user.click(screen.getByRole('button', { name: 'Copy link' }))
    expect(await navigator.clipboard.readText()).toBe(deviceCode.verification_url)
    expect(screen.getByRole('button', { name: 'Sign in with ChatGPT' })).toBeDisabled()
    stream.send({ type: 'completed' })
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Codex is signed in'))
    expect(refreshed).toHaveBeenCalledOnce()
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true)
    stream.close()
  })

  it('cancels the pending request and ignores a late completion', async () => {
    const stream = loginStream()
    const fetch = vi.fn().mockResolvedValue(stream.response)
    vi.stubGlobal('fetch', fetch)
    const refreshed = vi.fn()
    render(<CodexLoginPanel envText="" onAuthenticated={refreshed} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    await screen.findByLabelText('One-time device code')
    await userEvent.click(screen.getByRole('button', { name: 'Cancel sign-in' }))
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true)
    stream.send({ type: 'completed' })
    stream.close()
    expect(refreshed).not.toHaveBeenCalled()
    expect(screen.queryByLabelText('One-time device code')).not.toBeInTheDocument()
  })

  it.each(['environment', 'unmount'])('aborts login on %s changes', async (reason) => {
    const stream = loginStream()
    const fetch = vi.fn().mockResolvedValue(stream.response)
    vi.stubGlobal('fetch', fetch)
    const refreshed = vi.fn()
    const view = render(<CodexLoginPanel envText="" onAuthenticated={refreshed} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    await screen.findByLabelText('One-time device code')
    if (reason === 'environment') view.rerender(<CodexLoginPanel envText="HTTPS_PROXY=http://other.test:8080" onAuthenticated={refreshed} />)
    else view.unmount()
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true)
    expect(screen.queryByLabelText('One-time device code')).not.toBeInTheDocument()
    view.unmount()
    stream.close()
  })

  it('reports missing CLI without automatically repeating the login POST', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'codex_cli_missing' } }), { status: 400, headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    render(<CodexLoginPanel envText="" onAuthenticated={vi.fn()} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Install the Codex CLI')
    expect(fetch).toHaveBeenCalledOnce()
  })

  it('fails a disconnected stream without starting another authorization attempt', async () => {
    const stream = loginStream()
    const fetch = vi.fn().mockResolvedValue(stream.response)
    vi.stubGlobal('fetch', fetch)
    render(<CodexLoginPanel envText="" onAuthenticated={vi.fn()} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    stream.close()
    expect(await screen.findByRole('alert')).toHaveTextContent('Unable to complete Codex sign-in')
    expect(fetch).toHaveBeenCalledOnce()
  })
})
