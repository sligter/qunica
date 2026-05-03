import { useNavigate } from 'react-router-dom'

import { CreateAgentForm } from '@/components/agents/CreateAgentForm'

export function CreateAgentRightPane() {
  const navigate = useNavigate()
  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-xl space-y-4 p-8">
        <header className="space-y-1">
          <h1 className="text-xl font-semibold tracking-tight">New agent</h1>
          <p className="text-sm text-muted-foreground">
            Define an agent's name and system prompt. The agent will use the backend's
            default LLM provider until per-agent provider config is added.
          </p>
        </header>
        <CreateAgentForm onCreated={(id) => void navigate(`/agents/${id}`)} />
      </div>
    </div>
  )
}
