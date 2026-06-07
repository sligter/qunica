import { useState } from 'react'
import { Bot, Plus } from 'lucide-react'

import { AgentDetailDialog } from '@/components/agents/AgentDetailDialog'
import { CreateAgentDialog } from '@/components/agents/CreateAgentDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { useAgents } from '@/hooks/useAgents'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import type { AgentRead } from '@/types/api'

export function AgentsPage() {
  const agents = useAgents()
  const providers = useProviders()
  const skills = useSkills()
  const [createOpen, setCreateOpen] = useState(false)
  const [selectedAgent, setSelectedAgent] = useState<AgentRead | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)

  const getProvider = (id: string | null) =>
    id ? providers.data?.find((p) => p.id === id) : null
  // Count only skills that still exist — deleted skills linger in skill_ids
  // until pruned, which otherwise inflates the card count vs. the detail view.
  const existingSkillIds = new Set((skills.data ?? []).map((s) => s.id))

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-6">
        <div className="flex items-center gap-2">
          <Bot className="h-5 w-5 text-muted-foreground" />
          <h1 className="text-base font-semibold tracking-tight">Agents</h1>
          {agents.data && (
            <span className="text-xs text-muted-foreground">({agents.data.length})</span>
          )}
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="mr-1 h-4 w-4" />
          New Agent
        </Button>
      </header>

      <div className="flex-1 overflow-y-auto p-6">
        {agents.isLoading && (
          <p className="text-sm text-muted-foreground">Loading agents…</p>
        )}
        {agents.error && (
          <p className="text-sm text-red-600">Failed to load agents.</p>
        )}
        {agents.data && agents.data.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-3 py-20 text-center">
            <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <Bot className="h-7 w-7" />
            </div>
            <h2 className="text-base font-medium">No agents yet</h2>
            <p className="max-w-sm text-sm text-muted-foreground">
              Create your first agent to start building AI-powered group collaborations.
            </p>
            <Button size="sm" onClick={() => setCreateOpen(true)}>
              <Plus className="mr-1 h-4 w-4" />
              Create Agent
            </Button>
          </div>
        )}

        {agents.data && agents.data.length > 0 && (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {agents.data.map((a) => {
              const prov = getProvider(a.llm_provider_id)
              const mountedSkillCount = a.skill_ids.filter((id) =>
                existingSkillIds.has(id),
              ).length
              return (
                <Card
                  key={a.id}
                  className="cursor-pointer transition-shadow hover:shadow-md"
                  onClick={() => {
                    setSelectedAgent(a)
                    setDetailOpen(true)
                  }}
                >
                  <CardHeader className="flex flex-row items-start gap-3 space-y-0 pb-3">
                    <Avatar className="h-10 w-10 shrink-0">
                      <AvatarFallback className="bg-emerald-500/90 text-white font-semibold">
                        {a.name.slice(0, 1).toUpperCase()}
                      </AvatarFallback>
                    </Avatar>
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <h3 className="truncate text-sm font-semibold">{a.name}</h3>
                      <p className="line-clamp-1 text-xs text-muted-foreground">
                        {a.description || 'No description'}
                      </p>
                    </div>
                  </CardHeader>
                  <CardContent className="space-y-2">
                    <div className="flex flex-wrap gap-1.5">
                      {prov && (
                        <Badge variant="outline" className="text-[10px]">
                          {prov.kind} · {prov.default_model}
                        </Badge>
                      )}
                      {!prov && (
                        <Badge variant="outline" className="text-[10px]">
                          Default LLM
                        </Badge>
                      )}
                      <Badge
                        variant={a.status === 'active' ? 'default' : 'secondary'}
                        className="text-[10px]"
                      >
                        {a.status}
                      </Badge>
                    </div>
                    {mountedSkillCount > 0 && (
                      <p className="text-[10px] text-muted-foreground">
                        {mountedSkillCount} skill{mountedSkillCount > 1 ? 's' : ''} mounted
                      </p>
                    )}
                  </CardContent>
                </Card>
              )
            })}
          </div>
        )}
      </div>

      <CreateAgentDialog open={createOpen} onOpenChange={setCreateOpen} />
      <AgentDetailDialog
        agent={selectedAgent}
        open={detailOpen}
        onOpenChange={(v) => {
          setDetailOpen(v)
          if (!v) setSelectedAgent(null)
        }}
      />
    </div>
  )
}
