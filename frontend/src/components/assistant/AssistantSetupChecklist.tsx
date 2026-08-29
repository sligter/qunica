/**
 * What the dock shows before the Assistant can talk.
 *
 * Deliberately not model-driven. The Assistant is itself an LLM agent: with no
 * provider bound it cannot say a word, so the one thing it most needs to help
 * with — its own setup — is the one thing it cannot do conversationally.
 *
 * There are only two states worth distinguishing, and both are about the
 * Assistant's own provider binding. Nothing else is a setup step: its scratch
 * workspace is created for it on first load, and it *is* its agent.
 */

import { lazy, Suspense, useState } from 'react'
import { ArrowLeft, ChevronRight, Server, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { useUpdateAssistant } from '@/hooks/useAssistant'
import { useProviders } from '@/hooks/useProviders'
import type { LLMProviderRead } from '@/types/api'

const CreateProviderForm = lazy(() =>
  import('@/components/providers/CreateProviderForm').then((module) => ({
    default: module.CreateProviderForm,
  })),
)

interface AssistantSetupChecklistProps {
  loading?: boolean
  error?: boolean
}

export function AssistantSetupChecklist({ loading, error }: AssistantSetupChecklistProps) {
  const { t } = useTranslation('assistant')
  const providers = useProviders()
  const bind = useUpdateAssistant()
  const [bindError, setBindError] = useState<string | null>(null)
  const [creatingProvider, setCreatingProvider] = useState(false)

  if (loading || providers.isLoading) {
    return <PageState variant="loading" title={t('setup.loading')} />
  }
  if (error) return <PageState variant="error" title={t('setup.error')} />

  const available = providers.data ?? []

  const choose = async (provider: LLMProviderRead) => {
    setBindError(null)
    try {
      await bind.mutateAsync({
        llm_provider_id: provider.id,
        model: provider.default_model,
      })
    } catch (cause) {
      setBindError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  // Provider creation lives in the dock. The first assistant interaction must
  // not throw a new user into the resource library just to make the assistant
  // capable of answering.
  if (available.length === 0 || creatingProvider) {
    return (
      <div className="flex h-full flex-col overflow-y-auto p-5">
        <div>
          <span className="flex h-11 w-11 items-center justify-center rounded-2xl bg-primary/10 text-primary">
            <Sparkles className="h-5 w-5" aria-hidden />
          </span>
          <p className="mt-4 text-2xs font-semibold uppercase tracking-[0.16em] text-primary">
            {t('setup.step')}
          </p>
          <h2 className="mt-1 font-serif text-lg font-semibold tracking-tight">
            {t('setup.title')}
          </h2>
          <p className="mt-2 max-w-sm text-sm leading-6 text-muted-foreground">
            {t('setup.description')}
          </p>
        </div>
        <div className="mt-5 border-t border-border/70 pt-5">
          <Suspense fallback={<PageState inset variant="loading" title={t('setup.loadingProviderForm')} />}>
            <CreateProviderForm onCreated={(provider) => void choose(provider)} />
          </Suspense>
        </div>
        {bindError ? <p role="alert" className="mt-3 text-xs text-destructive">{bindError}</p> : null}
        <div className="mt-5 flex items-start gap-2.5 border-t border-border/70 pt-4 text-xs leading-5 text-muted-foreground">
          <Sparkles className="mt-0.5 h-3.5 w-3.5 shrink-0 text-primary" aria-hidden />
          <p>{t('setup.afterProvider')}</p>
        </div>
        {available.length > 0 ? (
          <Button size="sm" variant="ghost" className="mt-3 self-start px-0" onClick={() => setCreatingProvider(false)}>
            <ArrowLeft className="h-3.5 w-3.5" aria-hidden />
            {t('setup.backToProviders')}
          </Button>
        ) : null}
      </div>
    )
  }

  // Providers exist but none is bound to the Assistant. Binding is the whole
  // remaining step, so offer it right here rather than sending the user away.
  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-5">
      <div>
        <span className="flex h-10 w-10 items-center justify-center rounded-xl bg-primary/10 text-primary">
          <Server className="h-4 w-4" aria-hidden />
        </span>
        <h2 className="mt-3 font-serif text-lg font-semibold tracking-tight">
          {t('setup.chooseTitle')}
        </h2>
        <p className="mt-1.5 text-sm leading-6 text-muted-foreground">
          {t('setup.chooseDescription')}
        </p>
      </div>

      <ul className="flex flex-col gap-2" aria-busy={bind.isPending || undefined}>
        {available.map((provider) => (
          <li key={provider.id}>
            <button
              type="button"
              disabled={bind.isPending}
              onClick={() => void choose(provider)}
              className="group flex w-full items-center gap-3 rounded-xl border border-border/80 bg-card p-3 text-left shadow-xs transition-[border-color,background-color,transform] hover:-translate-y-px hover:border-primary/40 hover:bg-primary/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:translate-y-0 disabled:opacity-60"
            >
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground transition-colors group-hover:text-foreground">
                <Server className="h-4 w-4" aria-hidden />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{provider.name}</span>
                <span className="block truncate text-xs text-muted-foreground">
                  {provider.kind} · {provider.default_model}
                </span>
              </span>
              <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-primary" aria-hidden />
            </button>
          </li>
        ))}
      </ul>

      {bindError ? <p role="alert" className="text-xs text-destructive">{bindError}</p> : null}

      <Button size="sm" variant="ghost" className="mt-auto self-start px-0 hover:bg-transparent hover:text-primary" onClick={() => setCreatingProvider(true)}>
        {t('setup.addProvider')}
      </Button>
    </div>
  )
}
