import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { fetchEventSource } from '@microsoft/fetch-event-source'
import { Copy, Download, ExternalLink, LoaderCircle, LogIn, RefreshCw } from 'lucide-react'

import { parseAcpEnv } from '@/components/agents/acpRuntimeConfig'
import { Button } from '@/components/ui/button'
import { ApiError, fetchJson } from '@/lib/api-v2/client'
import { abortOnAuthChange, authFetch } from '@/lib/authFetch'
import { apiUrl } from '@/lib/runtime'
import { useAuthStore } from '@/stores/authStore'
import type { AcpRuntimeVersionRead } from '@/types/api'

type LoginState =
  | { kind: 'idle' | 'checking' | 'starting' | 'signed-out' | 'installing' }
  | { kind: 'waiting'; url: string; code: string }
  | { kind: 'authenticated'; method: string | null }
  | { kind: 'error'; code: string; detail?: string }

interface Status { authenticated: boolean; method: string | null }

const INSTALL_COMMAND = 'npm install -g @openai/codex@latest'

export function CodexLoginPanel({ envText, onAuthenticated, cli, onInstallCli, onCliDetected, versionsLoading = false, installBlocked = false }: {
  envText: string
  onAuthenticated: () => void
  cli?: AcpRuntimeVersionRead
  onInstallCli: () => Promise<void>
  onCliDetected?: () => void
  versionsLoading?: boolean
  installBlocked?: boolean
}) {
  const { t } = useTranslation('agents')
  const token = useAuthStore((state) => state.token)
  const [state, setState] = useState<LoginState>({ kind: 'idle' })
  const [copyMessage, setCopyMessage] = useState('')
  const controller = useRef<AbortController | null>(null)
  const canCancel = state.kind === 'checking' || state.kind === 'starting' || state.kind === 'waiting'
  const busy = canCancel || state.kind === 'installing' || installBlocked
  const missingCli = cli?.installed === false || (state.kind === 'error' && state.code === 'codex_cli_missing')

  useEffect(() => {
    controller.current?.abort()
    setState({ kind: 'idle' })
    setCopyMessage('')
    return () => { controller.current?.abort() }
  }, [envText, token])

  const cancel = () => {
    controller.current?.abort()
    controller.current = null
    setState({ kind: 'idle' })
    setCopyMessage('')
  }

  const copy = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value)
      setCopyMessage(t('codexLogin.copied'))
    } catch {
      setCopyMessage(t('codexLogin.copyFallback'))
    }
  }

  const run = async (login: boolean, refreshCli = true) => {
    if (!token) return
    controller.current?.abort()
    const current = new AbortController()
    controller.current = current
    const cleanupAuth = abortOnAuthChange(current, token)
    const active = () => controller.current === current && !current.signal.aborted
    setCopyMessage('')
    setState({ kind: login ? 'starting' : 'checking' })
    try {
      const body = { env: parseAcpEnv(envText) }
      if (!login) {
        const status = await fetchJson<Status>('/agents/codex-auth/status', {
          method: 'POST', token, body, signal: current.signal,
        })
        if (active()) {
          setState(status.authenticated ? { kind: 'authenticated', method: status.method } : { kind: 'signed-out' })
          if (refreshCli && cli?.installed === false) onCliDetected?.()
          if (status.authenticated) onAuthenticated()
        }
        return
      }
      let completed = false
      await fetchEventSource(apiUrl('/api/v2/agents/codex-auth/login'), {
        method: 'POST',
        headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json', Accept: 'text/event-stream' },
        body: JSON.stringify(body), signal: current.signal, openWhenHidden: true,
        fetch: authFetch,
        onopen: async (response) => {
          if (!response.ok) {
            const error = await response.json().catch(() => null)
            throw new ApiError(response.status, error?.error?.code ?? 'codex_auth_failed', '')
          }
          if (!response.headers.get('content-type')?.includes('text/event-stream')) {
            throw new Error('Invalid Codex login stream')
          }
        },
        onmessage: (message) => {
          if (!active() || !message.data) return
          const event = JSON.parse(message.data) as { type: string; verification_url?: string; user_code?: string; code?: string }
          if (event.type === 'device_code') {
            const url = new URL(event.verification_url ?? '')
            if (url.href !== 'https://auth.openai.com/codex/device' || !event.user_code) {
              throw new Error('Invalid Codex verification link')
            }
            setState({ kind: 'waiting', url: url.href, code: event.user_code })
          } else if (event.type === 'completed') {
            completed = true
            setState({ kind: 'authenticated', method: 'chatgpt' })
            onAuthenticated()
            current.abort()
          } else if (event.type === 'error') {
            throw new ApiError(502, event.code ?? 'codex_auth_failed', '')
          }
        },
        // Note: Reconnecting this POST would start a second login, not resume it.
        onerror: (error) => { throw error },
        onclose: () => { if (!completed) throw new Error('Codex login stream closed') },
      })
    } catch (error) {
      if (active()) setState({ kind: 'error', code: error instanceof ApiError ? error.code : 'codex_auth_failed' })
    } finally {
      cleanupAuth()
      if (controller.current === current) controller.current = null
    }
  }

  const installCli = async () => {
    if (!token || busy) return
    controller.current?.abort()
    const current = new AbortController()
    controller.current = current
    const cleanupAuth = abortOnAuthChange(current, token)
    const active = () => controller.current === current && !current.signal.aborted
    setCopyMessage('')
    setState({ kind: 'installing' })
    try {
      await onInstallCli()
      // A package install may finish after this form changes or closes. Only
      // check the account for the still-current form; never start authorization.
      if (active()) await run(false, false)
    } catch (error) {
      if (active()) setState({ kind: 'error', code: error instanceof ApiError ? error.code : 'codex_cli_install_failed', detail: error instanceof Error ? error.message : undefined })
    } finally {
      cleanupAuth()
      if (controller.current === current) controller.current = null
    }
  }

  const message = state.kind === 'authenticated'
    ? t(state.method === 'apiKey' ? 'codexLogin.apiKey' : 'codexLogin.authenticated')
    : state.kind === 'signed-out' ? t('codexLogin.signedOut')
      : state.kind === 'checking' ? t('codexLogin.checking')
        : state.kind === 'starting' ? t('codexLogin.starting')
          : state.kind === 'waiting' ? t('codexLogin.waiting')
            : state.kind === 'installing' ? t('codexLogin.installingCli')
              : t('codexLogin.description')
  const errorKey = state.kind === 'error' && state.code === 'codex_cli_missing' ? 'missingCli'
    : state.kind === 'error' && state.code === 'codex_auth_timeout' ? 'timeout'
      : state.kind === 'error' && state.code === 'conflict' ? 'busy'
        : state.kind === 'error' && state.code === 'acp_runtime_managed_externally' ? 'externallyManaged'
          : state.kind === 'error' && ['acp_runtime_install_failed', 'codex_cli_install_failed'].includes(state.code) ? 'installFailed' : 'failed'

  return (
    <section aria-label={t('codexLogin.title')} className="space-y-3 rounded-md border border-border bg-background p-3">
      <div>
        <h4 className="text-sm font-medium">{t('codexLogin.title')}</h4>
        <p role="status" className="mt-1 text-xs leading-relaxed text-muted-foreground">{message}</p>
      </div>
      <div className="space-y-2 rounded-md border border-border bg-muted/20 p-3">
        <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
          <span className="font-medium">Codex CLI <span className="font-normal text-muted-foreground">(@openai/codex)</span></span>
          <span className="text-muted-foreground">{cli ? (cli.installed ? (cli.local_version ?? t('codexLogin.detectedCli')) : t('runtime.notInstalled')) : (versionsLoading ? t('codexLogin.checkingCli') : t('runtime.unavailable'))}</span>
        </div>
        <p className="text-xs leading-relaxed text-muted-foreground">{t('codexLogin.cliDescription')}</p>
        {cli?.externally_managed ? (
          <p className="text-xs leading-relaxed text-muted-foreground">{t('codexLogin.externallyManaged')}</p>
        ) : (
          <Button type="button" size="sm" variant="outline" disabled={busy || !token} onClick={() => void installCli()}>
            {state.kind === 'installing' ? <LoaderCircle className="mr-1.5 h-3.5 w-3.5 animate-spin" aria-hidden /> : <Download className="mr-1.5 h-3.5 w-3.5" aria-hidden />}
            {state.kind === 'installing' ? t('codexLogin.installingCli') : t(cli?.installed ? 'codexLogin.updateCli' : 'codexLogin.installCli')}
          </Button>
        )}
        <details className="text-xs">
          <summary className="cursor-pointer text-muted-foreground">{t('codexLogin.manualInstall')}</summary>
          <div className="mt-2 space-y-2">
            {!cli?.externally_managed && <>
              <p className="leading-relaxed text-muted-foreground">{t('codexLogin.installHint')}</p>
              <code className="block select-all break-all rounded bg-muted p-2 font-mono">{INSTALL_COMMAND}</code>
              <Button type="button" size="sm" variant="ghost" onClick={() => void copy(INSTALL_COMMAND)}><Copy className="mr-1.5 h-3.5 w-3.5" aria-hidden />{t('codexLogin.copyInstall')}</Button>
            </>}
            <a href="https://learn.chatgpt.com/docs/codex/cli" target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1.5 text-primary underline underline-offset-4">{t('codexLogin.installDocs')}<ExternalLink className="h-3.5 w-3.5" aria-hidden /></a>
          </div>
        </details>
      </div>
      {state.kind === 'waiting' && (
        <div className="space-y-2 rounded-md border border-primary/25 bg-primary/5 p-3">
          <p className="text-xs leading-relaxed">{t('codexLogin.instructions')}</p>
          <output aria-label={t('codexLogin.deviceCode')} className="block select-all break-all font-mono text-lg font-semibold tracking-widest">{state.code}</output>
          <a href={state.url} target="_blank" rel="noopener noreferrer" className="inline-flex select-all items-center gap-1.5 break-all text-xs text-primary underline underline-offset-4">
            {state.url}<ExternalLink className="h-3.5 w-3.5 shrink-0" aria-hidden />
          </a>
          <div className="flex flex-wrap gap-2">
            <Button type="button" size="sm" variant="outline" onClick={() => void copy(state.code)}><Copy className="mr-1.5 h-3.5 w-3.5" aria-hidden />{t('codexLogin.copyCode')}</Button>
            <Button type="button" size="sm" variant="outline" onClick={() => void copy(state.url)}>{t('codexLogin.copyLink')}</Button>
          </div>
          {copyMessage && <p className="text-xs text-muted-foreground">{copyMessage}</p>}
        </div>
      )}
      {state.kind === 'error' && <p role="alert" className="text-xs leading-relaxed text-destructive">{t(`codexLogin.${errorKey}`)}</p>}
      {state.kind === 'error' && state.detail && <details className="text-xs"><summary className="cursor-pointer text-muted-foreground">{t('codexLogin.installDetails')}</summary><pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-muted p-2">{state.detail}</pre></details>}
      {missingCli && state.kind !== 'error' && <p className="text-xs leading-relaxed text-muted-foreground">{t('codexLogin.missingCli')}</p>}
      <div className="flex flex-wrap gap-2">
        <Button type="button" size="sm" disabled={busy || !token} onClick={() => void run(true)}>
          {state.kind === 'starting' || state.kind === 'waiting' ? <LoaderCircle className="mr-1.5 h-3.5 w-3.5 animate-spin" aria-hidden /> : <LogIn className="mr-1.5 h-3.5 w-3.5" aria-hidden />}
          {t('codexLogin.signIn')}
        </Button>
        <Button type="button" size="sm" variant="outline" disabled={busy || !token} onClick={() => void run(false)}>
          <RefreshCw className="mr-1.5 h-3.5 w-3.5" aria-hidden />{t('codexLogin.check')}
        </Button>
        {canCancel && <Button type="button" size="sm" variant="ghost" onClick={cancel}>{t('codexLogin.cancel')}</Button>}
      </div>
      {copyMessage && state.kind !== 'waiting' && <p className="text-xs text-muted-foreground">{copyMessage}</p>}
      <p className="text-2xs leading-relaxed text-muted-foreground">{t('codexLogin.hostHint')}</p>
    </section>
  )
}
