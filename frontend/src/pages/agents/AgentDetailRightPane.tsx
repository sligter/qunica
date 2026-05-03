import { useParams } from 'react-router-dom'

import { useAgent } from '@/hooks/useAgents'

export function AgentDetailRightPane() {
  const { agentId } = useParams<{ agentId: string }>()
  const agent = useAgent(agentId)

  if (agent.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading agent…</div>
  }
  if (agent.error) {
    return (
      <div className="p-6 text-sm text-red-600">
        Failed to load agent: {String(agent.error)}
      </div>
    )
  }
  if (!agent.data) {
    return <div className="p-6 text-sm text-muted-foreground">Agent not found.</div>
  }

  const a = agent.data
  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-2xl space-y-6 p-8">
        <header className="space-y-1">
          <h1 className="text-xl font-semibold tracking-tight">{a.name}</h1>
          {a.description && (
            <p className="text-sm text-muted-foreground">{a.description}</p>
          )}
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
              Visibility
            </h3>
            <p>{a.visibility}</p>
          </div>
          <div>
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Status
            </h3>
            <p>{a.status}</p>
          </div>
          <div className="col-span-2">
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Provider
            </h3>
            <p className="text-muted-foreground">
              Inherits the backend default. Per-agent provider config is not yet
              available in the UI.
            </p>
          </div>
        </section>
      </div>
    </div>
  )
}
