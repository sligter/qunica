import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'

import { EditAgentForm } from '@/components/agents/EditAgentForm'
import { Button } from '@/components/ui/button'
import { useAgent } from '@/hooks/useAgents'
import { useDeleteAgent } from '@/hooks/useDeleteAgent'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'

export function AgentDetailRightPane() {
  const { agentId } = useParams<{ agentId: string }>()
  const agent = useAgent(agentId)
  const providers = useProviders()
  const skills = useSkills()
  const navigate = useNavigate()
  const del = useDeleteAgent()
  const [editing, setEditing] = useState(false)

  if (agent.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading agent...</div>
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

  const onDelete = async () => {
    if (!confirm(`Delete agent "${a.name}"? This will remove it from active agent lists.`)) {
      return
    }
    await del.mutateAsync(a.id)
    void navigate('/agents')
  }

  if (editing) {
    return (
      <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
        <div className="mx-auto w-full max-w-2xl space-y-4 p-8">
          <header className="flex items-baseline justify-between gap-4">
            <h1 className="font-serif text-xl font-semibold tracking-tight">Edit {a.name}</h1>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              Cancel
            </Button>
          </header>
          <EditAgentForm
            agent={a}
            onSaved={() => {
              setEditing(false)
              void navigate(`/agents/${a.id}`)
            }}
          />
        </div>
      </div>
    )
  }

  const runtimeText =
    a.runtime_kind === 'acp'
      ? `ACP - ${a.acp_runtime?.command ?? 'not configured'}`
      : provider
        ? `LLM chat - ${provider.name} - ${provider.kind} - ${provider.default_model}`
        : 'LLM chat - Default (env settings)'

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-2xl space-y-6 p-8">
        <header className="flex items-baseline justify-between gap-4">
          <div className="space-y-1">
            <h1 className="font-serif text-xl font-semibold tracking-tight">{a.name}</h1>
            {a.description && (
              <p className="text-sm text-muted-foreground">{a.description}</p>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="outline" onClick={() => setEditing(true)}>
              Edit
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={onDelete}
              disabled={del.isPending}
            >
              {del.isPending ? 'Deleting...' : 'Delete'}
            </Button>
          </div>
        </header>

        <section className="space-y-2">
          <h2 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            System prompt
          </h2>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 text-sm">
            {a.system_prompt}
          </pre>
        </section>

        <section className="grid grid-cols-2 gap-4 text-sm">
          <div>
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Runtime
            </h3>
            <p>{runtimeText}</p>
          </div>
          <div>
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Status
            </h3>
            <p>{a.status}</p>
          </div>
        </section>

        <section className="space-y-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Mounted skills
          </h3>
          {mountedSkills.length === 0 ? (
            <p className="text-sm text-muted-foreground">No skills mounted.</p>
          ) : (
            <ul className="flex flex-wrap gap-2">
              {mountedSkills.map((s) => (
                <li key={s.id}>
                  <span className="inline-flex items-center gap-1 rounded-md border border-border bg-card px-2.5 py-1 text-xs">
                    <span className="font-medium">{s.name}</span>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  )
}
