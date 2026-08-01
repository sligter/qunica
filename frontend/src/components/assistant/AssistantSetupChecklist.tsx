/**
 * What the dock shows before the Assistant can talk.
 *
 * Deliberately not model-driven. The Assistant is itself an LLM agent: with no
 * provider configured it cannot say a word, so the one thing it most needs to
 * help with — first-run setup — is the one thing it cannot do conversationally.
 * This is a scripted checklist for exactly that gap, and the dock switches to
 * the real chat the moment a provider exists.
 */

import { Check, CircleDashed } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { PageState } from '@/components/ui/page-state'
import { useAgents } from '@/hooks/useAgents'
import { useProviders } from '@/hooks/useProviders'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { cn } from '@/lib/utils'

interface AssistantSetupChecklistProps {
  loading?: boolean
  error?: boolean
}

export function AssistantSetupChecklist({ loading, error }: AssistantSetupChecklistProps) {
  const { t } = useTranslation('assistant')
  const providers = useProviders()
  const workspaces = useWorkspaces()
  const agents = useAgents()

  if (loading) return <PageState variant="loading" title={t('setup.loading')} />
  if (error) return <PageState variant="error" title={t('setup.error')} />

  const steps = [
    {
      key: 'provider',
      done: (providers.data?.length ?? 0) > 0,
      label: t('setup.provider'),
      hint: t('setup.providerHint'),
      to: '/providers/new',
    },
    {
      key: 'workspace',
      done: (workspaces.data?.length ?? 0) > 0,
      label: t('setup.workspace'),
      hint: t('setup.workspaceHint'),
      to: '/workspaces/new',
    },
    {
      key: 'agent',
      done: (agents.data?.length ?? 0) > 0,
      label: t('setup.agent'),
      hint: t('setup.agentHint'),
      to: '/agents/new',
    },
  ]

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4">
      <div>
        <h2 className="text-sm font-medium">{t('setup.title')}</h2>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">{t('setup.description')}</p>
      </div>

      <ol className="flex flex-col gap-2">
        {steps.map((step) => (
          <li key={step.key}>
            <Link
              to={step.to}
              className={cn(
                'flex items-start gap-2.5 rounded-lg border border-border/70 p-2.5 text-left transition hover:bg-accent',
                step.done && 'opacity-60',
              )}
            >
              {step.done ? (
                <Check className="mt-0.5 h-4 w-4 shrink-0 text-success" aria-hidden />
              ) : (
                <CircleDashed className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
              )}
              <span className="min-w-0">
                <span className="block text-sm">{step.label}</span>
                <span className="mt-0.5 block text-xs leading-5 text-muted-foreground">
                  {step.hint}
                </span>
              </span>
            </Link>
          </li>
        ))}
      </ol>

      <p className="mt-auto text-xs leading-5 text-muted-foreground">{t('setup.afterProvider')}</p>
    </div>
  )
}
