import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'

import { EditAgentForm } from '@/components/agents/EditAgentForm'
import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { useAgent } from '@/hooks/useAgents'
import { useDeleteAgent } from '@/hooks/useDeleteAgent'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'

export function AgentDetailPage() {
  const { agentId } = useParams<{ agentId: string }>()
  const agent = useAgent(agentId)
  const providers = useProviders()
  const skills = useSkills()
  const navigate = useNavigate()
  const del = useDeleteAgent()
  const [editing, setEditing] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)

  if (agent.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading agent…</div>
  }
  if (agent.error) {
    return (
      <div className="p-6 text-sm text-destructive">
        Failed to load agent: {String(agent.error)}
      </div>
    )
  }
  if (!agent.data) {
    return <div className="p-6 text-sm text-muted-foreground">Agent not found.</div>
  }

  const a = agent.data
  const provider = a.llm_provider_id
    ? providers.data?.find((p) => p.id === a.llm_provider_id)
    : null
  const mountedSkills = (skills.data ?? []).filter((s) => a.skill_ids.includes(s.id))

  if (editing) {
    return (
      <DetailShell
        title={`Edit ${a.name}`}
        actions={
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
            Cancel
          </Button>
        }
      >
        <EditAgentForm agent={a} onSaved={() => setEditing(false)} />
      </DetailShell>
    )
  }

  const runtimeText =
    a.runtime_kind === 'acp'
      ? `ACP - ${a.acp_runtime?.command ?? 'not configured'}`
      : provider
        ? `LLM chat - ${provider.name} - ${provider.kind} - ${provider.default_model}`
        : 'LLM chat - Default (env settings)'

  return (
    <DetailShell
      title={a.name}
      subtitle={a.description || undefined}
      actions={
        <>
          <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>
            Edit
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => setConfirmOpen(true)}
            disabled={del.isPending}
          >
            {del.isPending ? 'Deleting…' : 'Delete'}
          </Button>
        </>
      }
    >
      <div className="space-y-8">
        <section className="grid grid-cols-1 gap-x-8 gap-y-4 text-sm sm:grid-cols-2 xl:grid-cols-3">
          <div>
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Runtime
            </h3>
            <p className="mt-1">{runtimeText}</p>
          </div>
          <div>
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Status
            </h3>
            <Badge
              variant={a.status === 'active' ? 'default' : 'secondary'}
              className="mt-1"
            >
              {a.status}
            </Badge>
          </div>
        </section>

        <section className="space-y-2">
          <h2 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            System prompt
          </h2>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 text-sm">
            {a.system_prompt}
          </pre>
        </section>

        {a.llm_config && Object.keys(a.llm_config).length > 0 && (
          <section className="space-y-2">
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Model parameters
            </h3>
            <div className="flex flex-wrap gap-2">
              {Object.entries(a.llm_config).map(([k, v]) => (
                <Badge key={k} variant="outline">
                  {k}: {String(v)}
                </Badge>
              ))}
            </div>
          </section>
        )}

        <section className="space-y-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Mounted skills
          </h3>
          {mountedSkills.length === 0 ? (
            <p className="text-sm text-muted-foreground">No skills mounted.</p>
          ) : (
            <div className="flex flex-wrap gap-2">
              {mountedSkills.map((s) => (
                <Badge key={s.id} variant="secondary">
                  {s.name}
                </Badge>
              ))}
            </div>
          )}
        </section>
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={`Delete agent "${a.name}"?`}
        description="This will remove it from active agent lists."
        confirmLabel="Delete"
        destructive
        onConfirm={async () => {
          await del.mutateAsync(a.id)
          void navigate('/agents')
        }}
      />
    </DetailShell>
  )
}
