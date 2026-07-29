import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import {
  EDIT_AGENT_FORM_ID,
  EditAgentForm,
} from '@/components/agents/EditAgentForm'
import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Field, FieldGrid } from '@/components/ui/field'
import { PageState } from '@/components/ui/page-state'
import { ProseBlock } from '@/components/ui/prose-block'
import { Section } from '@/components/ui/section'
import { useAgent } from '@/hooks/useAgents'
import { useDeleteAgent } from '@/hooks/useDeleteAgent'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import { formatResourceStatus } from '@/i18n/resourceStatus'

/** Mounted skills stay a glanceable summary even when an agent mounts dozens. */
const SKILL_BADGE_LIMIT = 24

export function AgentDetailPage() {
  const { t } = useTranslation(['agents', 'common'])
  const { agentId } = useParams<{ agentId: string }>()
  const agent = useAgent(agentId)
  const providers = useProviders()
  const skills = useSkills()
  const navigate = useNavigate()
  const del = useDeleteAgent()
  const [editing, setEditing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [showAllSkills, setShowAllSkills] = useState(false)
  const saveReady = useEditSaveGuard(editing)

  if (agent.isLoading) {
    return <PageState variant="loading" title={t('agents:detail.loading')} />
  }
  if (agent.error) {
    return (
      <PageState
        variant="error"
        title={t('agents:detail.loadError', { error: String(agent.error) })}
      />
    )
  }
  if (!agent.data) {
    return <PageState title={t('agents:detail.notFound')} />
  }

  const a = agent.data
  const provider = a.llm_provider_id
    ? providers.data?.find((p) => p.id === a.llm_provider_id)
    : null
  const mountedSkills = (skills.data ?? []).filter((s) => a.skill_ids.includes(s.id))

  if (editing) {
    return (
      <DetailShell
        title={t('agents:detail.editTitle', { name: a.name })}
        actions={
          <>
            <Button
              size="sm"
              type="submit"
              form={EDIT_AGENT_FORM_ID}
              disabled={!saveReady || saving}
            >
              {saving ? t('common:actions.saving') : t('common:actions.save')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              {t('common:actions.cancel')}
            </Button>
          </>
        }
      >
        <EditAgentForm
          agent={a}
          onSavingChange={setSaving}
          onSaved={() => setEditing(false)}
        />
      </DetailShell>
    )
  }

  const runtimeText =
    a.runtime_kind === 'acp'
      ? t('agents:detail.runtimeAcp', { command: a.acp_runtime?.command ?? t('agents:detail.notConfigured') })
      : provider
        ? t('agents:detail.runtimeChat', { provider: provider.name, kind: provider.kind, model: provider.default_model })
        : t('agents:detail.runtimeDefault')

  return (
    <DetailShell
      title={a.name}
      subtitle={a.description || undefined}
      actions={
        <>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setSaving(false)
              setEditing(true)
            }}
          >
            {t('common:actions.edit')}
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => setConfirmOpen(true)}
            disabled={del.isPending}
          >
            {del.isPending ? t('common:actions.deleting') : t('common:actions.delete')}
          </Button>
        </>
      }
    >
      <div className="space-y-8">
        <FieldGrid>
          <Field label={t('agents:detail.runtime')} value={runtimeText} />
          <Field label={t('agents:detail.status')}>
            <Badge
              variant={a.status === 'active' ? 'default' : 'secondary'}
              className="mt-1"
            >
              {formatResourceStatus(a.status, t)}
            </Badge>
          </Field>
        </FieldGrid>

        <Section title={t('agents:detail.systemPrompt')}>
          <ProseBlock maxHeight="lg">{a.system_prompt}</ProseBlock>
        </Section>

        {a.llm_config && Object.keys(a.llm_config).length > 0 && (
          <Section title={t('agents:detail.modelParameters')} as="h3">
            <div className="flex flex-wrap gap-2">
              {Object.entries(a.llm_config).map(([k, v]) => (
                <Badge key={k} variant="outline">
                  {k}: {String(v)}
                </Badge>
              ))}
            </div>
          </Section>
        )}

        <Section title={t('agents:detail.mountedSkills')} as="h3">
          {mountedSkills.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('agents:detail.noMountedSkills')}</p>
          ) : (
            <div className="flex flex-wrap items-center gap-2">
              {(showAllSkills ? mountedSkills : mountedSkills.slice(0, SKILL_BADGE_LIMIT)).map(
                (s) => (
                  <Badge key={s.id} variant="secondary">
                    {s.name}
                  </Badge>
                ),
              )}
              {!showAllSkills && mountedSkills.length > SKILL_BADGE_LIMIT && (
                <button
                  type="button"
                  onClick={() => setShowAllSkills(true)}
                  className="text-xs font-medium text-muted-foreground underline-offset-2 transition-colors hover:text-foreground hover:underline"
                >
                  {t('common:picker.moreChips', {
                    count: mountedSkills.length - SKILL_BADGE_LIMIT,
                  })}
                </button>
              )}
            </div>
          )}
        </Section>
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('agents:detail.deleteTitle', { name: a.name })}
        description={t('agents:detail.deleteDescription')}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          await del.mutateAsync(a.id)
          void navigate('/agents')
        }}
      />
    </DetailShell>
  )
}
