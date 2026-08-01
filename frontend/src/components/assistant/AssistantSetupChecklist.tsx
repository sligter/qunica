/**
 * What the dock shows before the Assistant can talk.
 *
 * Deliberately not model-driven. The Assistant is itself an LLM agent: with no
 * provider bound it cannot say a word, so the one thing it most needs to help
 * with — its own setup — is the one thing it cannot do conversationally.
 *
 * There are only two states worth distinguishing, and both are about the
 * Assistant's own provider binding. A workspace and other agents are unrelated:
 * the Assistant has no workspace, and it *is* its agent.
 */

import { useState } from 'react'
import { ExternalLink } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { useBindAssistantProvider } from '@/hooks/useAssistant'
import { useProviders } from '@/hooks/useProviders'

interface AssistantSetupChecklistProps {
  loading?: boolean
  error?: boolean
}

export function AssistantSetupChecklist({ loading, error }: AssistantSetupChecklistProps) {
  const { t } = useTranslation('assistant')
  const providers = useProviders()
  const bind = useBindAssistantProvider()
  const [bindError, setBindError] = useState<string | null>(null)

  if (loading || providers.isLoading) {
    return <PageState variant="loading" title={t('setup.loading')} />
  }
  if (error) return <PageState variant="error" title={t('setup.error')} />

  const available = providers.data ?? []

  // No provider anywhere: there is nothing to bind yet, so point at creating
  // one. This is the only step that has to happen outside the dock.
  if (available.length === 0) {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div>
          <h2 className="text-sm font-medium">{t('setup.title')}</h2>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {t('setup.description')}
          </p>
        </div>
        <Button asChild size="sm" variant="outline" className="self-start">
          <Link to="/providers/new">
            <ExternalLink className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t('setup.provider')}
          </Link>
        </Button>
        <p className="mt-auto text-xs leading-5 text-muted-foreground">
          {t('setup.afterProvider')}
        </p>
      </div>
    )
  }

  // Providers exist but none is bound to the Assistant. Binding is the whole
  // remaining step, so offer it right here rather than sending the user away.
  const choose = async (providerId: string) => {
    setBindError(null)
    try {
      await bind.mutateAsync(providerId)
    } catch (cause) {
      setBindError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4">
      <div>
        <h2 className="text-sm font-medium">{t('setup.chooseTitle')}</h2>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">
          {t('setup.chooseDescription')}
        </p>
      </div>

      <ul className="flex flex-col gap-2">
        {available.map((provider) => (
          <li key={provider.id}>
            <button
              type="button"
              disabled={bind.isPending}
              onClick={() => void choose(provider.id)}
              className="flex w-full flex-col items-start gap-0.5 rounded-lg border border-border/70 p-2.5 text-left transition hover:bg-accent disabled:opacity-60"
            >
              <span className="text-sm">{provider.name}</span>
              <span className="text-xs text-muted-foreground">
                {provider.kind} · {provider.default_model}
              </span>
            </button>
          </li>
        ))}
      </ul>

      {bindError ? <p className="text-xs text-destructive">{bindError}</p> : null}

      <Button asChild size="sm" variant="ghost" className="mt-auto self-start">
        <Link to="/providers/new">
          <ExternalLink className="mr-1.5 h-3.5 w-3.5" aria-hidden />
          {t('setup.provider')}
        </Link>
      </Button>
    </div>
  )
}
