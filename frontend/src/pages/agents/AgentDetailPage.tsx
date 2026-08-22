import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import {
  ArrowRight,
  Check,
  Copy,
  Folder,
  MessageSquare,
  Sparkles,
  Wrench,
} from 'lucide-react'
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
import { DetailSkeleton } from '@/components/ui/skeleton'
import { ProseBlock } from '@/components/ui/prose-block'
import { Section } from '@/components/ui/section'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAgent } from '@/hooks/useAgents'
import { useDeleteAgent } from '@/hooks/useDeleteAgent'
import { useCreateDirectChat } from '@/hooks/useDirectChats'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import { useUnsavedChangesGuard } from '@/hooks/useUnsavedChangesGuard'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { avatarColorClass } from '@/lib/avatarColor'
import { cn } from '@/lib/utils'

export function AgentDetailPage() {
  const { t } = useTranslation(['agents', 'common'])
  const { agentId } = useParams<{ agentId: string }>()
  const agent = useAgent(agentId)
  const providers = useProviders()
  const skills = useSkills()
  const workspaces = useWorkspaces()
  const createDirectChat = useCreateDirectChat()
  const navigate = useNavigate()
  const del = useDeleteAgent()
  const [searchParams, setSearchParams] = useSearchParams()
  // Deep link: /agents/:id?edit=1 opens straight into the edit form.
  const [editing, setEditing] = useState(searchParams.get('edit') === '1')
  const [saving, setSaving] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [copiedId, setCopiedId] = useState(false)
  const [copiedPrompt, setCopiedPrompt] = useState(false)
  const saveReady = useEditSaveGuard(editing)
  useUnsavedChangesGuard(editing && dirty)

  useEffect(() => {
    if (editing) {
      setSearchParams(new URLSearchParams({ edit: '1' }), { replace: true })
    } else {
      setSearchParams({}, { replace: true })
    }
  }, [editing, setSearchParams])

  if (agent.isLoading) {
    return <DetailSkeleton label={t('agents:detail.loading')} />
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
  const workspace = workspaces.data?.find((w) => w.id === a.workspace_id)
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
          onDirtyChange={setDirty}
          onSavingChange={setSaving}
          onSaved={() => {
            setDirty(false)
            setEditing(false)
          }}
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

  const onStartChat = async () => {
    try {
      const created = await createDirectChat.mutateAsync({ agent_id: a.id })
      void navigate(`/chats/${created.id}`)
    } catch {
      // fallback
    }
  }

  const onCopyId = () => {
    if (!navigator.clipboard) return
    void navigator.clipboard.writeText(a.id).then(() => {
      setCopiedId(true)
      setTimeout(() => setCopiedId(false), 2000)
    })
  }

  const onCopyPrompt = () => {
    if (!navigator.clipboard || !a.system_prompt) return
    void navigator.clipboard.writeText(a.system_prompt).then(() => {
      setCopiedPrompt(true)
      setTimeout(() => setCopiedPrompt(false), 2000)
    })
  }

  return (
    <DetailShell
      title={a.name}
      subtitle={
        <div className="flex flex-wrap items-center gap-2">
          {a.description ? <span className="text-foreground/80">{a.description}</span> : null}
          <Badge
            variant={a.status === 'active' ? 'default' : 'secondary'}
            className="text-[10px]"
          >
            {formatResourceStatus(a.status, t)}
          </Badge>
          <Badge variant="outline" className="text-[10px] font-mono">
            {a.runtime_kind === 'acp' ? 'ACP' : 'LLM Chat'}
          </Badge>
        </div>
      }
      actions={
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => void onStartChat()}
            disabled={createDirectChat.isPending}
          >
            <MessageSquare className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">{t('common:actions.startChat', '发起对话')}</span>
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setSaving(false)
              setDirty(false)
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
        </div>
      }
    >
      <div className="space-y-6">
        {/* Quick Hero Banner */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
          <div className="flex items-center gap-3.5">
            <span
              className={cn(
                'flex h-12 w-12 shrink-0 select-none items-center justify-center rounded-2xl text-base font-semibold shadow-xs',
                avatarColorClass(a.id),
              )}
            >
              {a.name.slice(0, 1).toUpperCase()}
            </span>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">{a.name}</h2>
                <Badge variant="outline" className="text-[10px] font-mono">
                  {a.runtime_kind === 'acp' ? 'ACP' : 'LLM Chat'}
                </Badge>
              </div>
              <p className="text-xs text-muted-foreground mt-0.5">{runtimeText}</p>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={onCopyId}
            className="h-8 gap-1.5 text-xs text-muted-foreground"
          >
            {copiedId ? <Check className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
            <span className="font-mono text-2xs">{a.id}</span>
          </Button>
        </div>

        {/* Tabbed Navigation */}
        <Tabs defaultValue="overview" className="space-y-4">
          <TabsList variant="underline">
            <TabsTrigger value="overview">{t('agents:fields.runtime', '运行时与概览')}</TabsTrigger>
            <TabsTrigger value="prompt">{t('agents:detail.systemPrompt', '系统提示词')}</TabsTrigger>
            <TabsTrigger value="capabilities">{t('agents:fields.builtInTools', '工具与能力')}</TabsTrigger>
            <TabsTrigger value="skills">
              {t('agents:detail.mountedSkills', '挂载技能')} ({mountedSkills.length})
            </TabsTrigger>
          </TabsList>

          {/* Overview Tab */}
          <TabsContent value="overview" className="space-y-6 pt-2">
            <FieldGrid columns={2}>
              <Field label={t('agents:detail.runtime')} value={runtimeText} />
              <Field label={t('agents:fields.workspace')}>
                {workspace ? (
                  <Link
                    to={`/workspaces/${workspace.id}`}
                    className="inline-flex items-center gap-1.5 font-medium text-primary hover:underline"
                  >
                    <Folder className="h-3.5 w-3.5" />
                    {workspace.name}
                  </Link>
                ) : (
                  <span className="text-muted-foreground">{t('agents:states.inheritProvider')}</span>
                )}
              </Field>
            </FieldGrid>

            {a.llm_config && Object.keys(a.llm_config).length > 0 && (
              <Section title={t('agents:detail.modelParameters')} as="h3">
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  {Object.entries(a.llm_config).map(([k, v]) => (
                    <div key={k} className="rounded-lg border border-border/70 bg-card p-3 shadow-xs">
                      <span className="text-2xs font-medium uppercase text-muted-foreground">{k}</span>
                      <p className="mt-1 font-mono text-sm font-semibold">{String(v)}</p>
                    </div>
                  ))}
                </div>
              </Section>
            )}
          </TabsContent>

          {/* Prompt Tab */}
          <TabsContent value="prompt" className="space-y-4 pt-2">
            <div className="flex items-center justify-between">
              <span className="text-xs text-muted-foreground">
                {a.system_prompt?.length ?? 0} {t('common:characters', '字符')}
              </span>
              <Button
                variant="outline"
                size="sm"
                className="h-7 gap-1.5 text-xs"
                onClick={onCopyPrompt}
              >
                {copiedPrompt ? <Check className="h-3 w-3 text-success" /> : <Copy className="h-3 w-3" />}
                {copiedPrompt ? t('common:actions.copied', '已复制') : t('common:actions.copy', '复制提示词')}
              </Button>
            </div>
            <ProseBlock maxHeight="lg" className="rounded-xl border border-border/80 p-4 font-mono text-xs">
              {a.system_prompt}
            </ProseBlock>
          </TabsContent>

          {/* Capabilities Tab */}
          <TabsContent value="capabilities" className="space-y-4 pt-2">
            <Section title={t('agents:fields.builtInTools')} as="h3">
              <div className="flex flex-wrap gap-2">
                {a.tool_config && Object.keys(a.tool_config).length > 0 ? (
                  Object.entries(a.tool_config).map(([toolName, enabled]) => (
                    <Badge
                      key={toolName}
                      variant={enabled ? 'default' : 'secondary'}
                      className="text-xs"
                    >
                      <Wrench className="mr-1 h-3 w-3" />
                      {toolName}
                    </Badge>
                  ))
                ) : (
                  <p className="text-xs text-muted-foreground">
                    {t('agents:tools.states.executable', '已启用默认工具集')}
                  </p>
                )}
              </div>
            </Section>
          </TabsContent>

          {/* Skills Tab */}
          <TabsContent value="skills" className="space-y-4 pt-2">
            {mountedSkills.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border p-8 text-center">
                <Sparkles className="h-6 w-6 text-muted-foreground/60 mb-2" />
                <p className="text-sm font-medium">{t('agents:detail.noMountedSkills')}</p>
                <p className="text-xs text-muted-foreground mt-1">
                  {t('agents:form.skillsDescription')}
                </p>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
                {mountedSkills.map((s) => (
                  <Link
                    key={s.id}
                    to={`/skills/${s.id}`}
                    className="flex items-center justify-between rounded-xl border border-border/80 bg-card p-3 text-xs transition-colors hover:border-primary/40 hover:bg-card-hover"
                  >
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Sparkles className="h-4 w-4 text-primary shrink-0" />
                      <span className="truncate font-medium text-foreground">{s.name}</span>
                    </div>
                    <ArrowRight className="h-3 w-3 text-muted-foreground shrink-0" />
                  </Link>
                ))}
              </div>
            )}
          </TabsContent>
        </Tabs>
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
