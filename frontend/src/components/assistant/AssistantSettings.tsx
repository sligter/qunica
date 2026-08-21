/**
 * The Assistant's own settings: which provider it calls, and which model.
 *
 * Reachable from the dock header at any time. Everything else about the
 * Assistant — its prompt, its tools, its lack of a workspace — is fixed by
 * construction, because those are what make it safe to give app-control tools.
 */

import { useState } from 'react'
import { ExternalLink, Settings2, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { useAssistant, useUpdateAssistant } from '@/hooks/useAssistant'
import { useProviders } from '@/hooks/useProviders'
import { useSystemSettings, useUpdateSystemSettings } from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api-v2/client'

interface AssistantSettingsProps {
  onClose: () => void
}

const SELECT_CLASS =
  'h-9 w-full rounded-lg border border-input bg-background px-3 text-sm outline-none transition-colors focus-visible:border-ring/60 focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-60'

export function AssistantSettings({ onClose }: AssistantSettingsProps) {
  const { t } = useTranslation('assistant')
  const assistant = useAssistant()
  const providers = useProviders()
  const update = useUpdateAssistant()
  const systemSettings = useSystemSettings()
  const updateSystemSettings = useUpdateSystemSettings()

  const [draft, setDraft] = useState<{ providerId: string; model: string } | null>(null)
  const [error, setError] = useState<string | null>(null)

  // The query may still be in flight on first render, so the form reads through
  // to the loaded values until the user edits something. Seeding useState from
  // `assistant.data` alone would leave the selects permanently empty.
  const providerId = draft?.providerId ?? assistant.data?.provider_id ?? ''
  const model = draft?.model ?? assistant.data?.model ?? ''
  const setModel = (next: string) => setDraft({ providerId, model: next })

  const available = providers.data ?? []
  const selected = available.find((provider) => provider.id === providerId)
  const models = selected?.models ?? []
  const autoApprove = systemSettings.data?.assistant_auto_approve ?? false

  // A model belongs to one provider. Carrying the old choice across a provider
  // change would pin the Assistant to something the new one rejects at send
  // time, inside a stream, with no obvious cause.
  const changeProvider = (nextId: string) => {
    const next = available.find((provider) => provider.id === nextId)
    const keepModel = next?.models?.some((entry) => entry.id === model) ? model : ''
    setDraft({ providerId: nextId, model: keepModel })
  }

  const save = async () => {
    setError(null)
    try {
      await update.mutateAsync({
        llm_provider_id: providerId || null,
        model: model || null,
      })
      onClose()
    } catch (cause) {
      setError(
        cause instanceof ApiError || cause instanceof Error
          ? cause.message
          : String(cause),
      )
    }
  }

  const setAutoApprove = async (next: boolean) => {
    setError(null)
    try {
      await updateSystemSettings.mutateAsync({ assistant_auto_approve: next })
    } catch (cause) {
      setError(
        cause instanceof ApiError || cause instanceof Error
          ? cause.message
          : String(cause),
      )
    }
  }

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-4">
      <div className="flex items-start gap-3">
        <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
          <Settings2 className="h-4 w-4" aria-hidden />
        </span>
        <div className="min-w-0">
          <h2 className="font-serif text-lg font-semibold tracking-tight">
            {t('settings.title')}
          </h2>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {t('settings.description')}
          </p>
        </div>
      </div>

      <div className="space-y-3 rounded-xl border border-border/80 bg-card p-3.5 shadow-xs">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="assistant-provider">{t('settings.provider')}</Label>
          <select
            id="assistant-provider"
            className={SELECT_CLASS}
            value={providerId}
            onChange={(event) => changeProvider(event.target.value)}
          >
            <option value="">{t('settings.noProvider')}</option>
            {available.map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.name}
              </option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="assistant-model">{t('settings.model')}</Label>
          <select
            id="assistant-model"
            className={SELECT_CLASS}
            value={model}
            disabled={!providerId}
            onChange={(event) => setModel(event.target.value)}
          >
            <option value="">
              {selected?.default_model
                ? t('settings.providerDefaultNamed', { model: selected.default_model })
                : t('settings.providerDefault')}
            </option>
            {models.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.id}
              </option>
            ))}
          </select>
        </div>

        <Button asChild size="sm" variant="ghost" className="h-auto px-0 py-0 text-muted-foreground hover:bg-transparent hover:text-primary">
          <Link to="/providers/new">
            {t('setup.addProvider')}
            <ExternalLink className="h-3.5 w-3.5" aria-hidden />
          </Link>
        </Button>
      </div>

      <div className="flex items-start gap-3 rounded-xl border border-border/80 bg-muted/30 p-3.5">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-background text-muted-foreground shadow-xs">
          <ShieldCheck className="h-4 w-4" aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-3">
            <Label htmlFor="assistant-auto-approve">{t('settings.autoApprove')}</Label>
            <Switch
              id="assistant-auto-approve"
              checked={autoApprove}
              disabled={systemSettings.isLoading || updateSystemSettings.isPending}
              onCheckedChange={(next) => void setAutoApprove(next)}
              aria-label={t('settings.autoApprove')}
            />
          </div>
          <p className="mt-1.5 text-xs leading-5 text-muted-foreground">
            {t('settings.autoApproveDescription')}
          </p>
        </div>
      </div>

      {error ? <p role="alert" className="text-xs text-destructive">{error}</p> : null}

      <div className="mt-auto grid grid-cols-2 gap-2 pt-1">
        <Button size="sm" variant="outline" onClick={onClose}>
          {t('settings.cancel')}
        </Button>
        <Button size="sm" disabled={update.isPending} onClick={() => void save()}>
          {update.isPending ? t('settings.saving') : t('settings.save')}
        </Button>
      </div>
    </div>
  )
}
