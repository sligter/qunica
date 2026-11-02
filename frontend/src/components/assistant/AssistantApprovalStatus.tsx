import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useSystemSettings } from '@/hooks/useSystemSettings'
import { isDesktopRuntime, openSettingsWindow } from '@/lib/desktop'

export function AssistantApprovalStatus({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation('assistant')
  const settings = useSystemSettings()
  const [error, setError] = useState<string | null>(null)
  const status = !settings.data || settings.isError ? 'approvalUnknown'
    : settings.data.assistant_auto_approve === true ? 'approvalAutomatic' : 'approvalManual'

  return (
    <div className={compact ? 'shrink-0 border-b border-border px-3 py-2 text-xs' : 'space-y-2 text-xs'}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="font-medium" title={t('settings.approvalScope')}>{t(`settings.${status}`)}</span>
        <Link className="text-primary underline-offset-2 hover:underline" to="/settings/assistant-actions" onClick={(event) => {
          if (!isDesktopRuntime()) return
          event.preventDefault()
          void openSettingsWindow('/settings/assistant-actions').catch((cause: unknown) => {
            setError(cause instanceof Error ? cause.message : String(cause))
          })
        }}>{t('settings.actionHistory')}</Link>
      </div>
      {!compact ? <p className="leading-5 text-muted-foreground">{t('settings.approvalScope')}</p> : null}
      {error ? <p role="alert" className="text-destructive">{error}</p> : null}
    </div>
  )
}
