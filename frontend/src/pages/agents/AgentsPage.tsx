import { CreateAgentForm } from '@/components/agents/CreateAgentForm'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { useAgents } from '@/hooks/useAgents'

export function AgentsPage() {
  const agents = useAgents()
  return (
    <div className="mx-auto max-w-5xl space-y-8 p-6">
      <section className="space-y-3">
        <h1 className="text-xl font-semibold tracking-tight">Your agents</h1>
        {agents.isLoading && <p className="text-sm text-muted-foreground">Loading…</p>}
        {agents.error && (
          <p className="text-sm text-red-600">Failed to load: {String(agents.error)}</p>
        )}
        {agents.data && agents.data.length === 0 && (
          <p className="text-sm text-muted-foreground">
            No agents yet — create your first one below.
          </p>
        )}
        {agents.data && agents.data.length > 0 && (
          <ul className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {agents.data.map((a) => (
              <li key={a.id}>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">{a.name}</CardTitle>
                    {a.description && <CardDescription>{a.description}</CardDescription>}
                  </CardHeader>
                  <CardContent>
                    <p className="line-clamp-3 whitespace-pre-wrap text-xs text-muted-foreground">
                      {a.system_prompt}
                    </p>
                  </CardContent>
                </Card>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section className="space-y-3">
        <h2 className="text-lg font-semibold tracking-tight">New agent</h2>
        <Card>
          <CardContent className="pt-6">
            <CreateAgentForm />
          </CardContent>
        </Card>
      </section>
    </div>
  )
}
