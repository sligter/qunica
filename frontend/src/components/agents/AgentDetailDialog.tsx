import { useState } from 'react'

import { EditAgentForm } from '@/components/agents/EditAgentForm'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import { useDeleteAgent } from '@/hooks/useDeleteAgent'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import type { AgentRead } from '@/types/api'

interface AgentDetailDialogProps {
  agent: AgentRead | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function AgentDetailDialog({ agent, open, onOpenChange }: AgentDetailDialogProps) {
  const [editing, setEditing] = useState(false)
  const providers = useProviders()
  const skills = useSkills()
  const del = useDeleteAgent()

  if (!agent) return null

  const provider = agent.llm_provider_id
    ? providers.data?.find((p) => p.id === agent.llm_provider_id)
    : null
  const mountedSkills = (skills.data ?? []).filter((s) => agent.skill_ids.includes(s.id))
  const runtimeText =
    agent.runtime_kind === 'acp'
      ? `ACP - ${agent.acp_runtime?.command ?? 'not configured'}`
      : provider
        ? `LLM chat - ${provider.name} - ${provider.kind} - ${provider.default_model}`
        : 'LLM chat - Default (env settings)'

  const handleClose = (v: boolean) => {
    if (!v) setEditing(false)
    onOpenChange(v)
  }

  const onDelete = async () => {
    if (!confirm(`Delete agent "${agent.name}"? This will remove it from active agent lists.`)) {
      return
    }
    await del.mutateAsync(agent.id)
    handleClose(false)
  }

  if (editing) {
    return (
      <Dialog open={open} onOpenChange={handleClose}>
        <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Edit {agent.name}</DialogTitle>
          </DialogHeader>
          <EditAgentForm
            agent={agent}
            onSaved={() => {
              setEditing(false)
            }}
          />
        </DialogContent>
      </Dialog>
    )
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-center justify-between pr-8">
            <div className="space-y-1">
              <DialogTitle>{agent.name}</DialogTitle>
              {agent.description && (
                <p className="text-sm text-muted-foreground">{agent.description}</p>
              )}
            </div>
            <div className="flex items-center gap-2">
              <Button size="sm" variant="outline" onClick={() => setEditing(true)}>
                Edit
              </Button>
              <Button
                size="sm"
                variant="destructive"
                onClick={onDelete}
                disabled={del.isPending}
              >
                {del.isPending ? 'Deleting...' : 'Delete'}
              </Button>
            </div>
          </div>
        </DialogHeader>

        <Separator />

        <section className="space-y-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            System prompt
          </h3>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 text-sm max-h-48 overflow-y-auto">
            {agent.system_prompt}
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
            <Badge variant={agent.status === 'active' ? 'default' : 'secondary'}>
              {agent.status}
            </Badge>
          </div>
        </section>

        {agent.llm_config && Object.keys(agent.llm_config).length > 0 && (
          <section className="space-y-2">
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Model parameters
            </h3>
            <div className="flex flex-wrap gap-2">
              {Object.entries(agent.llm_config).map(([k, v]) => (
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
      </DialogContent>
    </Dialog>
  )
}
