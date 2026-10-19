import { useState } from 'react'
import { Check, Copy, KeyRound, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useAutomationSecret } from '@/hooks/useAutomations'
import { apiUrl } from '@/lib/runtime'
import type { AutomationRead } from '@/types/api'

function hookUrl(hookId: string): string {
  const path = `/api/v2/automation-hooks/${hookId}`
  try {
    const resolved = apiUrl(path)
    return resolved.startsWith('/') && typeof window !== 'undefined'
      ? `${window.location.origin}${resolved}`
      : resolved
  } catch {
    return path
  }
}

export function AutomationSecretDialog({
  automation,
  open,
  onOpenChange,
  onChanged,
}: {
  automation: AutomationRead
  open: boolean
  onOpenChange: (open: boolean) => void
  onChanged: () => void
}) {
  const { t } = useTranslation('automations')
  const secretMutation = useAutomationSecret()
  const [secret, setSecret] = useState<string | null>(null)
  const [configured, setConfigured] = useState(automation.hook_configured)
  const [version, setVersion] = useState(automation.version)
  const [copied, setCopied] = useState<'url' | 'secret' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [revokeOpen, setRevokeOpen] = useState(false)
  const url = hookUrl(automation.hook_id)

  const copy = async (kind: 'url' | 'secret', value: string) => {
    if (!navigator.clipboard) return
    try {
      await navigator.clipboard.writeText(value)
      setCopied(kind)
      window.setTimeout(() => setCopied((current) => (current === kind ? null : current)), 1600)
    } catch {
      setError(t('errors.network'))
    }
  }

  const rotate = async () => {
    setError(null)
    try {
      const result = await secretMutation.mutateAsync({
        id: automation.id,
        action: 'rotate',
        expectedVersion: version,
      })
      setSecret(result.secret)
      setConfigured(true)
      setVersion(result.version)
      onChanged()
    } catch (cause) {
      setError(
        cause instanceof Error
          ? t('secret.error', { message: cause.message })
          : t('errors.network'),
      )
    }
  }

  const revoke = async () => {
    setError(null)
    try {
      const result = await secretMutation.mutateAsync({
        id: automation.id,
        action: 'revoke',
        expectedVersion: version,
      })
      setSecret(null)
      setConfigured(false)
      setVersion(result.version)
      setRevokeOpen(false)
      onChanged()
    } catch (cause) {
      setError(
        cause instanceof Error
          ? t('secret.error', { message: cause.message })
          : t('errors.network'),
      )
      throw cause
    }
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <KeyRound className="h-4 w-4 text-primary" aria-hidden />
              {t('secret.title')}
            </DialogTitle>
            <DialogDescription>{t('secret.description')}</DialogDescription>
          </DialogHeader>

          <div className="space-y-3">
            <div className="rounded-lg border border-border/70 bg-muted/25 p-3">
              <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                {t('secret.hookId')}
              </p>
              <div className="mt-1.5 flex items-start gap-2">
                <code className="min-w-0 flex-1 break-all font-mono text-xs leading-relaxed">
                  {url}
                </code>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0"
                  onClick={() => void copy('url', url)}
                  aria-label={t('secret.copy')}
                  title={copied === 'url' ? t('secret.copied') : t('secret.copy')}
                >
                  {copied === 'url' ? (
                    <Check className="h-3.5 w-3.5 text-success" aria-hidden />
                  ) : (
                    <Copy className="h-3.5 w-3.5" aria-hidden />
                  )}
                </Button>
              </div>
            </div>

            <p className="text-sm text-muted-foreground">
              {configured ? t('secret.configured') : t('secret.notConfigured')}
            </p>

            {secret ? (
              <div className="space-y-2 rounded-lg border border-warning/40 bg-warning/10 p-3">
                <p className="flex items-start gap-2 text-xs font-medium text-warning-foreground">
                  <TriangleAlert className="mt-px h-3.5 w-3.5 shrink-0" aria-hidden />
                  {t('secret.shownOnce')}
                </p>
                <div className="flex items-start gap-2">
                  <code className="min-w-0 flex-1 break-all rounded-md border border-border/70 bg-background px-2 py-1.5 font-mono text-xs leading-relaxed">
                    {secret}
                  </code>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => void copy('secret', secret)}
                    aria-label={t('secret.copy')}
                    title={copied === 'secret' ? t('secret.copied') : t('secret.copy')}
                  >
                    {copied === 'secret' ? (
                      <Check className="h-3.5 w-3.5 text-success" aria-hidden />
                    ) : (
                      <Copy className="h-3.5 w-3.5" aria-hidden />
                    )}
                  </Button>
                </div>
              </div>
            ) : null}

            {error ? (
              <p role="alert" className="text-xs text-destructive">
                {error}
              </p>
            ) : null}
          </div>

          <DialogFooter className="gap-2 sm:justify-between">
            <div>
              {configured ? (
                <Button
                  type="button"
                  variant="ghost"
                  className="text-destructive hover:text-destructive"
                  onClick={() => setRevokeOpen(true)}
                  disabled={secretMutation.isPending}
                >
                  {t('secret.revoke')}
                </Button>
              ) : null}
            </div>
            <div className="flex gap-2">
              <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                {t('secret.close')}
              </Button>
              <Button
                type="button"
                onClick={() => void rotate()}
                disabled={secretMutation.isPending}
              >
                {configured ? t('secret.rotate') : t('secret.generate')}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={revokeOpen}
        onOpenChange={setRevokeOpen}
        title={t('secret.revokeTitle')}
        description={t('secret.revokeDescription')}
        confirmLabel={t('secret.revoke')}
        destructive
        onConfirm={revoke}
      />
    </>
  )
}
