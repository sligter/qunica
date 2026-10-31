import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CodexLoginPanel } from '@/components/agents/CodexLoginPanel'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { ApiError } from '@/lib/api-v2/client'
import type { AcpRuntimeVersionRead } from '@/types/api'

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
const missingCli: AcpRuntimeVersionRead = {
  id: 'codex-cli', package_name: '@openai/codex', default_package_spec: '@openai/codex@latest',
  installed: false, externally_managed: false, local_version: null, latest_version: '0.148.0',
  status: 'not_installed', message: null,
}

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
    render(<CodexLoginPanel envText="HTTPS_PROXY=http://proxy.test:8080" onAuthenticated={refreshed} onInstallCli={vi.fn()} />)
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
    render(<CodexLoginPanel envText="" onAuthenticated={refreshed} onInstallCli={vi.fn()} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    expect(await screen.findByLabelText('One-time device code')).toHaveTextContent('TEST-1234')
    expect(screen.getByRole('link', { name: deviceCode.verification_url })).toHaveAttribute('href', deviceCode.verification_url)
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
    render(<CodexLoginPanel envText="" onAuthenticated={refreshed} onInstallCli={vi.fn()} />)
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
    const view = render(<CodexLoginPanel envText="" onAuthenticated={refreshed} onInstallCli={vi.fn()} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    await screen.findByLabelText('One-time device code')
    if (reason === 'environment') view.rerender(<CodexLoginPanel envText="HTTPS_PROXY=http://other.test:8080" onAuthenticated={refreshed} onInstallCli={vi.fn()} />)
    else view.unmount()
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true)
    expect(screen.queryByLabelText('One-time device code')).not.toBeInTheDocument()
    view.unmount()
    stream.close()
  })

  it('reports missing CLI without automatically repeating the login POST', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'codex_cli_missing' } }), { status: 400, headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    render(<CodexLoginPanel envText="" onAuthenticated={vi.fn()} onInstallCli={vi.fn()} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Install the Codex CLI')
    expect(fetch).toHaveBeenCalledOnce()
  })

  it('fails a disconnected stream without starting another authorization attempt', async () => {
    const stream = loginStream()
    const fetch = vi.fn().mockResolvedValue(stream.response)
    vi.stubGlobal('fetch', fetch)
    render(<CodexLoginPanel envText="" onAuthenticated={vi.fn()} onInstallCli={vi.fn()} />)
    await userEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }))
    stream.send(deviceCode)
    stream.close()
    expect(await screen.findByRole('alert')).toHaveTextContent('Unable to complete Codex sign-in')
    expect(fetch).toHaveBeenCalledOnce()
  })

  it('installs the missing CLI and checks existing sign-in without starting authorization', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ authenticated: false, method: null }), { headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    const install = vi.fn().mockResolvedValue(undefined)
    const refreshed = vi.fn()
    render(<CodexLoginPanel envText="HTTPS_PROXY=http://proxy.test:8080" onAuthenticated={refreshed} cli={missingCli} onInstallCli={install} />)
    expect(screen.getByText('Not installed')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Install Codex CLI' }))
    expect(install).toHaveBeenCalledOnce()
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Codex is not signed in'))
    expect(fetch).toHaveBeenCalledOnce()
    expect(fetch.mock.calls[0][0]).toContain('/codex-auth/status')
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ env: { HTTPS_PROXY: 'http://proxy.test:8080' } })
    expect(refreshed).not.toHaveBeenCalled()
  })

  it('blocks duplicate installation and refreshes capabilities if the installed CLI is already signed in', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ authenticated: true, method: 'chatgpt' }), { headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    let finish!: () => void
    const install = vi.fn(() => new Promise<void>((resolve) => { finish = resolve }))
    const refreshed = vi.fn()
    render(<CodexLoginPanel envText="" onAuthenticated={refreshed} cli={missingCli} onInstallCli={install} />)
    await userEvent.click(screen.getByRole('button', { name: 'Install Codex CLI' }))
    expect(screen.getByRole('button', { name: 'Installing Codex CLI…' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Sign in with ChatGPT' })).toBeDisabled()
    expect(screen.queryByRole('button', { name: 'Cancel sign-in' })).not.toBeInTheDocument()
    expect(fetch).not.toHaveBeenCalled()
    await act(async () => { finish() })
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Codex is signed in'))
    expect(install).toHaveBeenCalledOnce()
    expect(refreshed).toHaveBeenCalledOnce()
  })

  it('shows installation failures and offers a copyable manual command without retrying', async () => {
    const user = userEvent.setup()
    const fetch = vi.fn()
    vi.stubGlobal('fetch', fetch)
    const install = vi.fn().mockRejectedValue(new ApiError(502, 'acp_runtime_install_failed', 'npm EACCES: global directory is not writable'))
    render(<CodexLoginPanel envText="" onAuthenticated={vi.fn()} cli={missingCli} onInstallCli={install} />)
    await user.click(screen.getByRole('button', { name: 'Install Codex CLI' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Unable to install the Codex CLI')
    expect(screen.getByText('npm EACCES: global directory is not writable')).toBeInTheDocument()
    await user.click(screen.getByText('Manual installation and help'))
    await user.click(screen.getByRole('button', { name: 'Copy install command' }))
    expect(await navigator.clipboard.readText()).toBe('npm install -g @openai/codex@latest')
    expect(install).toHaveBeenCalledOnce()
    expect(fetch).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Check sign-in' })).toBeEnabled()
  })

  it('uses externally managed Codex without offering an npm overwrite', () => {
    const install = vi.fn()
    render(<CodexLoginPanel envText="" onAuthenticated={vi.fn()} onInstallCli={install} cli={{ ...missingCli, installed: true, externally_managed: true, status: 'local_only' }} />)
    expect(screen.getByText(/managed by an external installer/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Install Codex CLI' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Update Codex CLI' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Check sign-in' })).toBeEnabled()
    expect(install).not.toHaveBeenCalled()
  })

  it.each(['environment', 'unmount', 'token'])('does not check the old account after installation finishes following %s changes', async (reason) => {
    const fetch = vi.fn()
    vi.stubGlobal('fetch', fetch)
    let finish!: () => void
    const install = vi.fn(() => new Promise<void>((resolve) => { finish = resolve }))
    const props = { onAuthenticated: vi.fn(), onInstallCli: install, cli: missingCli }
    const view = render(<CodexLoginPanel envText="" {...props} />)
    await userEvent.click(screen.getByRole('button', { name: 'Install Codex CLI' }))
    if (reason === 'environment') view.rerender(<CodexLoginPanel envText="HTTPS_PROXY=http://other.test:8080" {...props} />)
    else if (reason === 'token') act(() => { useAuthStore.setState({ token: 'new-token' }) })
    else view.unmount()
    await act(async () => { finish() })
    expect(fetch).not.toHaveBeenCalled()
    expect(props.onAuthenticated).not.toHaveBeenCalled()
  })
})
